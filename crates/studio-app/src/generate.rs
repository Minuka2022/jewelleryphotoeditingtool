//! Runs a generation plan: sends the requests a few at a time, saves each
//! image as it arrives and reports progress.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use studio_core::gemini::ImageGenerator;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::studio::{GenerationPlan, OutputInfo, Studio};

/// How many requests are in flight at once. Kept low to stay inside the
/// rate limits of a new Google account.
const CONCURRENCY: usize = 2;

/// Sent to the window each time one image finishes, successfully or not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub item_id: String,
    pub run_id: String,
    pub done: usize,
    pub total: usize,
    pub style_id: String,
    pub style_name: String,
    pub number: u32,
    pub output: Option<OutputInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct Summary {
    pub saved: usize,
    pub failed: usize,
    pub first_error: Option<String>,
}

pub async fn run<G, F>(
    generator: Arc<G>,
    studio: &Mutex<Studio>,
    plan: GenerationPlan,
    mut on_progress: F,
) -> Summary
where
    G: ImageGenerator + 'static,
    F: FnMut(Progress) + Send,
{
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let mut tasks = JoinSet::new();
    for (index, job) in plan.jobs.iter().enumerate() {
        let request = job.request.clone();
        let generator = generator.clone();
        let semaphore = semaphore.clone();
        tasks.spawn(async move {
            let _permit = semaphore.acquire_owned().await;
            (index, generator.generate(&request).await)
        });
    }

    let total = plan.jobs.len();
    let mut summary = Summary::default();
    while let Some(joined) = tasks.join_next().await {
        let (index, result) = match joined {
            Ok(finished) => finished,
            Err(error) => {
                summary.failed += 1;
                summary.first_error.get_or_insert(error.to_string());
                continue;
            }
        };
        let job = &plan.jobs[index];
        let saved = result
            .map_err(|error| error.to_string())
            .and_then(|output| {
                let mut studio = studio
                    .lock()
                    .map_err(|_| "The app's data is unavailable.".to_owned())?;
                studio.record_output(&plan, job, &output.image)
            });

        let (output, error) = match saved {
            Ok(output) => {
                summary.saved += 1;
                (Some(output), None)
            }
            Err(error) => {
                summary.failed += 1;
                summary.first_error.get_or_insert(error.clone());
                (None, Some(error))
            }
        };
        on_progress(Progress {
            item_id: plan.item_id.clone(),
            run_id: plan.run_id.clone(),
            done: summary.saved + summary.failed,
            total,
            style_id: job.style_id.clone(),
            style_name: job.style_name.clone(),
            number: job.number,
            output,
            error,
        });
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::tests::studio_with_ring;
    use crate::studio::{crop_preview, run_export, GenerateArgs, OutputRef};
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use studio_core::export::{Crop, ExportOptions};
    use studio_core::gemini::{
        GenerateError, GenerateOutput, GenerateRequest, GeneratedImage, Usage,
    };

    /// Succeeds with a 400 x 300 PNG, except for every third request.
    #[derive(Default)]
    struct FakeGenerator {
        calls: AtomicUsize,
    }

    fn fake_png() -> Vec<u8> {
        let image = image::RgbImage::from_fn(400, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 120])
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    impl ImageGenerator for FakeGenerator {
        async fn generate(
            &self,
            _request: &GenerateRequest,
        ) -> Result<GenerateOutput, GenerateError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call.is_multiple_of(3) {
                return Err(GenerateError::Api {
                    status: 500,
                    message: "try later".into(),
                });
            }
            Ok(GenerateOutput {
                image: GeneratedImage {
                    mime_type: "image/png".into(),
                    bytes: fake_png(),
                },
                notes: None,
                usage: Usage::default(),
            })
        }
    }

    #[tokio::test]
    async fn a_run_saves_images_reports_progress_and_exports_approved_ones() {
        let (dir, studio, item_id) = studio_with_ring("generate");
        let plan = studio
            .plan_generation(&GenerateArgs {
                item_id: item_id.clone(),
                style_ids: vec!["black-velvet".into(), "studio-white".into()],
                variants: 3,
                note: Some("slightly warmer light".into()),
            })
            .unwrap();
        let studio = Mutex::new(studio);

        let mut events = Vec::new();
        let summary = run(
            Arc::new(FakeGenerator::default()),
            &studio,
            plan,
            |progress| events.push(progress),
        )
        .await;

        assert_eq!((summary.saved, summary.failed), (4, 2));
        assert!(summary.first_error.unwrap().contains("try later"));
        assert_eq!(events.len(), 6);
        assert_eq!(events.last().unwrap().done, 6);
        assert!(events.iter().all(|event| event.total == 6));
        assert_eq!(events.iter().filter(|e| e.output.is_some()).count(), 4);

        let mut studio = studio.into_inner().unwrap();
        let item = studio.item_info(&item_id).unwrap();
        assert_eq!(item.outputs.len(), 4);
        for output in &item.outputs {
            assert!(Path::new(&output.path).is_file());
            assert_eq!(output.record.cost_usd, Some(0.0504));
            assert_eq!(output.record.note.as_deref(), Some("slightly warmer light"));
        }

        // Approve one, delete another, then export the approved one twice.
        let approved = item.outputs[0].record.id.clone();
        let deleted = &item.outputs[1];
        studio
            .set_output_status(&item_id, &approved, crate::store::OutputStatus::Approved)
            .unwrap();
        let item = studio.remove_output(&item_id, &deleted.record.id).unwrap();
        assert_eq!(item.outputs.len(), 3);
        assert!(!Path::new(&deleted.path).exists());

        // Crop the right half; the crop is kept with the image.
        let crop = Crop {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let source = studio.output_source(&item_id, &approved).unwrap();
        let cropped = crop_preview(&source, Some(crop)).unwrap();
        let item = studio
            .set_output_crop(&item_id, &approved, cropped)
            .unwrap();
        let shown = &item.outputs[0];
        assert_eq!(shown.record.crop, Some(crop));
        // The screen shows a cropped copy; the original file is untouched.
        assert_ne!(shown.path, shown.original_path);
        assert_eq!(image::open(&shown.path).unwrap().width(), 200);
        assert_eq!(image::open(&shown.original_path).unwrap().width(), 400);
        let bad = Crop { width: 0.9, ..crop };
        assert!(crop_preview(&source, Some(bad)).is_err());
        let whole = Crop {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        assert!(crop_preview(&source, Some(whole)).unwrap().is_none());

        // Cropping again replaces the display copy instead of adding another.
        let first_copy = shown.path.clone();
        let tighter = Crop {
            height: 0.5,
            ..crop
        };
        studio
            .set_output_crop(
                &item_id,
                &approved,
                crop_preview(&source, Some(tighter)).unwrap(),
            )
            .unwrap();
        assert!(!Path::new(&first_copy).exists());
        let item = studio
            .set_output_crop(
                &item_id,
                &approved,
                crop_preview(&source, Some(crop)).unwrap(),
            )
            .unwrap();
        let preview_path = item.outputs[0].path.clone();

        // Export it twice as a 100 px wide JPEG: the crop and the size apply,
        // and the second file gets the next number instead of replacing the first.
        let export_dir = dir.join("export");
        let selection = [OutputRef {
            item_id: item_id.clone(),
            output_id: approved.clone(),
        }];
        let jobs = studio.export_jobs(&selection).unwrap();
        let options = ExportOptions {
            width: Some(100),
            ..ExportOptions::default()
        };
        let estimate = jobs[0].estimate(&options).unwrap();
        assert_eq!((estimate.source_width, estimate.source_height), (200, 300));
        assert_eq!((estimate.width, estimate.height), (100, 150));
        assert!(estimate.bytes > 0);

        assert_eq!(run_export(&jobs, &options, &export_dir).unwrap(), 1);
        assert_eq!(run_export(&jobs, &options, &export_dir).unwrap(), 1);
        let style = &item.outputs[0].record.style_id;
        let first = export_dir.join(format!("Gold-Ring-01-{style}-1.jpg"));
        let exported = image::open(&first).unwrap();
        assert_eq!((exported.width(), exported.height()), (100, 150));
        assert!(export_dir
            .join(format!("Gold-Ring-01-{style}-2.jpg"))
            .is_file());
        assert!(run_export(&[], &options, &export_dir).is_err());

        // Removing the crop brings back the whole image and its copy is deleted.
        let item = studio.set_output_crop(&item_id, &approved, None).unwrap();
        assert_eq!(item.outputs[0].record.crop, None);
        assert_eq!(item.outputs[0].path, item.outputs[0].original_path);
        assert!(!Path::new(&preview_path).exists());
        let full = studio.export_jobs(&selection).unwrap()[0]
            .estimate(&ExportOptions::default())
            .unwrap();
        assert_eq!((full.width, full.height), (400, 300));

        studio.remove_item(&item_id).unwrap();
        assert!(studio.items_info().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
