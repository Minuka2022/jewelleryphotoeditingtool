//! Finds the photos, plans which styles apply to each, runs the generations
//! in parallel and records what happened.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use studio_core::gemini::{GenerateError, GenerateRequest, ImageGenerator, Usage};
use studio_core::library::{JewelryCategory, Library, StyleTemplate, FALLBACK_CATEGORY};
use studio_core::prompt::{build_prompt, BuiltPrompt, PromptError, PromptRequest};
use studio_core::reference::{prepare_reference, PrepareOptions, ReferenceImage};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::report;

const PHOTO_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// One jewelry photo to process.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub source: PathBuf,
    pub category: JewelryCategory,
}

pub struct PlannedStyle {
    pub style: StyleTemplate,
    pub prompt: BuiltPrompt,
}

pub struct PlannedItem {
    pub item: Item,
    pub styles: Vec<PlannedStyle>,
}

pub struct Plan {
    pub items: Vec<PlannedItem>,
    /// Item and style pairs left out, with the reason.
    pub skipped: Vec<String>,
}

impl Plan {
    pub fn image_count(&self, variants: u32) -> usize {
        let pairs: usize = self.items.iter().map(|item| item.styles.len()).sum();
        pairs * variants as usize
    }
}

pub struct RunSettings {
    pub started_at: String,
    pub model: String,
    pub image_size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub variants: u32,
    pub concurrency: usize,
    pub per_image_usd: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunManifest {
    pub started_at: String,
    pub model: String,
    pub image_size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub master_version: u32,
    pub per_image_usd: Option<f64>,
    pub note: Option<String>,
    pub items: Vec<ItemRecord>,
}

#[derive(Debug, Serialize)]
pub struct ItemRecord {
    pub id: String,
    pub source: String,
    pub category: String,
    pub category_label: String,
    /// Path of the prepared reference photo, relative to the run folder.
    pub reference: Option<String>,
    pub error: Option<String>,
    pub styles: Vec<StyleRecord>,
}

#[derive(Debug, Serialize)]
pub struct StyleRecord {
    pub style_id: String,
    pub style_name: String,
    pub style_version: u32,
    pub prompt_file: String,
    pub variants: Vec<VariantRecord>,
}

#[derive(Debug, Default, Serialize)]
pub struct VariantRecord {
    pub number: u32,
    /// Path of the generated image, relative to the run folder.
    pub file: Option<String>,
    pub error: Option<String>,
    pub seconds: f64,
    pub usage: Usage,
    pub notes: Option<String>,
}

impl RunManifest {
    fn variants(&self) -> impl Iterator<Item = &VariantRecord> {
        self.items
            .iter()
            .flat_map(|item| &item.styles)
            .flat_map(|style| &style.variants)
    }

    pub fn saved(&self) -> usize {
        self.variants().filter(|v| v.file.is_some()).count()
    }

    pub fn failed(&self) -> usize {
        self.variants().filter(|v| v.file.is_none()).count()
    }

    /// The error message of every image that could not be generated.
    pub fn errors(&self) -> impl Iterator<Item = &str> {
        self.variants().filter_map(|v| v.error.as_deref())
    }

    /// Cost of the images that were produced, when the price is on file.
    pub fn cost_usd(&self) -> Option<f64> {
        self.per_image_usd.map(|price| price * self.saved() as f64)
    }
}

/// Photos directly inside `input` get the default category. Photos inside a
/// sub-folder named after a category id (for example `samples/ring/`) get
/// that category.
pub fn discover_items(
    input: &Path,
    library: &Library,
    default_category: Option<&str>,
) -> Result<Vec<Item>, String> {
    let default_id = default_category.unwrap_or(FALLBACK_CATEGORY);
    let default = library.category(default_id).ok_or_else(|| {
        let known: Vec<&str> = library
            .master
            .categories
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        format!(
            "unknown category \"{default_id}\". Available: {}",
            known.join(", ")
        )
    })?;

    let mut items = Vec::new();
    let mut used_ids = BTreeSet::new();
    let mut add = |source: PathBuf, category: &JewelryCategory| {
        let stem = source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        items.push(Item {
            id: unique_id(&stem, &mut used_ids),
            source,
            category: category.clone(),
        });
    };

    for path in sorted_entries(input)? {
        if path.is_dir() {
            let folder = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let category = library.category(&folder).unwrap_or(default);
            for photo in sorted_entries(&path)?.into_iter().filter(|p| is_photo(p)) {
                add(photo, category);
            }
        } else if is_photo(&path) {
            add(path, default);
        }
    }
    Ok(items)
}

/// Builds the prompt for every item and style pair. A style that is not set
/// up for an item's category is skipped; a rejected note stops the run.
pub fn plan(
    items: Vec<Item>,
    styles: &[&StyleTemplate],
    library: &Library,
    note: Option<&str>,
) -> Result<Plan, String> {
    let mut planned = Vec::new();
    let mut skipped = Vec::new();
    for item in items {
        let mut item_styles = Vec::new();
        for &style in styles {
            let request = PromptRequest {
                master: &library.master,
                category: &item.category,
                style,
                reference_count: 1,
                user_note: note,
                design_notes: None,
            };
            match build_prompt(&request) {
                Ok(prompt) => item_styles.push(PlannedStyle {
                    style: style.clone(),
                    prompt,
                }),
                Err(PromptError::StyleNotForCategory { .. }) => skipped.push(format!(
                    "{} with {}: this style is not set up for {}",
                    item.id, style.id, item.category.label
                )),
                Err(other) => return Err(other.to_string()),
            }
        }
        if !item_styles.is_empty() {
            planned.push(PlannedItem {
                item,
                styles: item_styles,
            });
        }
    }
    Ok(Plan {
        items: planned,
        skipped,
    })
}

/// Generates every planned image into `run_dir`, then writes `run.json` and
/// `report.html` there. Individual failures are recorded, not fatal.
pub async fn execute<G: ImageGenerator + 'static>(
    generator: Arc<G>,
    plan: &Plan,
    master_version: u32,
    settings: &RunSettings,
    run_dir: &Path,
) -> Result<RunManifest, String> {
    fs::create_dir_all(run_dir).map_err(|e| cannot_write(run_dir, e))?;

    let semaphore = Arc::new(Semaphore::new(settings.concurrency.max(1)));
    let mut jobs = JoinSet::new();
    let mut items: Vec<ItemRecord> = Vec::new();

    for planned in &plan.items {
        let item = &planned.item;
        let item_index = items.len();
        let mut record = ItemRecord {
            id: item.id.clone(),
            source: item.source.display().to_string(),
            category: item.category.id.clone(),
            category_label: item.category.label.clone(),
            reference: None,
            error: None,
            styles: Vec::new(),
        };

        let item_dir = run_dir.join(&item.id);
        let reference = match prepare_item(item, &item_dir) {
            Ok(reference) => reference,
            Err(message) => {
                println!("{}: skipped - {message}", item.id);
                record.error = Some(message);
                items.push(record);
                continue;
            }
        };
        record.reference = Some(format!("{}/reference.jpg", item.id));

        for planned_style in &planned.styles {
            let style = &planned_style.style;
            let style_dir = item_dir.join(&style.id);
            let prompt_path = style_dir.join("prompt.txt");
            fs::create_dir_all(&style_dir).map_err(|e| cannot_write(&style_dir, e))?;
            fs::write(&prompt_path, &planned_style.prompt.text)
                .map_err(|e| cannot_write(&prompt_path, e))?;

            let style_index = record.styles.len();
            record.styles.push(StyleRecord {
                style_id: style.id.clone(),
                style_name: style.name.clone(),
                style_version: style.version,
                prompt_file: format!("{}/{}/prompt.txt", item.id, style.id),
                variants: (1..=settings.variants)
                    .map(|number| VariantRecord {
                        number,
                        ..VariantRecord::default()
                    })
                    .collect(),
            });

            for variant_index in 0..settings.variants as usize {
                let request = GenerateRequest {
                    model: settings.model.clone(),
                    prompt: planned_style.prompt.text.clone(),
                    references: vec![reference.clone()],
                    aspect_ratio: settings.aspect_ratio.clone(),
                    image_size: settings.image_size.clone(),
                };
                let generator = generator.clone();
                let semaphore = semaphore.clone();
                jobs.spawn(async move {
                    let _permit = semaphore.acquire_owned().await;
                    let started = Instant::now();
                    let result = generator.generate(&request).await;
                    let seconds = started.elapsed().as_secs_f64();
                    ((item_index, style_index, variant_index), result, seconds)
                });
            }
        }
        items.push(record);
    }

    let total = jobs.len();
    let mut done = 0;
    while let Some(joined) = jobs.join_next().await {
        let ((item_index, style_index, variant_index), result, seconds) =
            joined.map_err(|e| format!("a generation task stopped unexpectedly: {e}"))?;
        done += 1;

        let item_id = items[item_index].id.clone();
        let style_id = items[item_index].styles[style_index].style_id.clone();
        let number = variant_index + 1;
        let label = format!("[{done}/{total}] {item_id} / {style_id} / option {number}");
        let variant = &mut items[item_index].styles[style_index].variants[variant_index];
        variant.seconds = seconds;

        match result {
            Ok(output) => {
                let relative = format!(
                    "{item_id}/{style_id}/option-{number}.{}",
                    output.image.extension()
                );
                match fs::write(run_dir.join(&relative), &output.image.bytes) {
                    Ok(()) => {
                        println!("{label}: saved ({seconds:.0}s)");
                        variant.file = Some(relative);
                    }
                    Err(error) => {
                        println!("{label}: FAILED - could not save the image: {error}");
                        variant.error = Some(format!("could not save the image: {error}"));
                    }
                }
                variant.usage = output.usage;
                variant.notes = output.notes;
            }
            Err(error) => {
                println!("{label}: FAILED - {error}");
                if let GenerateError::NoImage { response, .. } = &error {
                    save_debug_response(run_dir, &item_id, &style_id, number, response);
                }
                variant.error = Some(error.to_string());
            }
        }
    }

    let manifest = RunManifest {
        started_at: settings.started_at.clone(),
        model: settings.model.clone(),
        image_size: settings.image_size.clone(),
        aspect_ratio: settings.aspect_ratio.clone(),
        master_version,
        per_image_usd: settings.per_image_usd,
        note: settings.note.clone(),
        items,
    };

    let manifest_path = run_dir.join("run.json");
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&manifest_path, json).map_err(|e| cannot_write(&manifest_path, e))?;
    let report_path = run_dir.join("report.html");
    fs::write(&report_path, report::render(&manifest))
        .map_err(|e| cannot_write(&report_path, e))?;

    Ok(manifest)
}

/// Prepares the reference photo and saves the exact copy that is sent to the
/// model, so the report compares against what the model actually saw.
fn prepare_item(item: &Item, item_dir: &Path) -> Result<Arc<ReferenceImage>, String> {
    let reference =
        prepare_reference(&item.source, &PrepareOptions::default()).map_err(|e| e.to_string())?;
    fs::create_dir_all(item_dir).map_err(|e| cannot_write(item_dir, e))?;
    let path = item_dir.join("reference.jpg");
    fs::write(&path, &reference.bytes).map_err(|e| cannot_write(&path, e))?;
    Ok(Arc::new(reference))
}

/// Best effort: a failure to save troubleshooting data must not hide the
/// original error.
fn save_debug_response(run_dir: &Path, item_id: &str, style_id: &str, number: usize, body: &str) {
    let debug_dir = run_dir.join("debug");
    if fs::create_dir_all(&debug_dir).is_ok() {
        let name = format!("{item_id}-{style_id}-option-{number}.json");
        let _ = fs::write(debug_dir.join(name), body);
    }
}

fn cannot_write(path: &Path, error: std::io::Error) -> String {
    format!("cannot write {}: {error}", path.display())
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read folder {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();
    Ok(paths)
}

fn is_photo(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                PHOTO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            })
}

/// A folder-safe id from a file name, made unique within the run.
fn unique_id(stem: &str, used: &mut BTreeSet<String>) -> String {
    let cleaned: String = stem
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-');
    let base = if cleaned.is_empty() { "item" } else { cleaned };

    let mut id = base.to_owned();
    let mut suffix = 2;
    while !used.insert(id.clone()) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use studio_core::gemini::{GenerateOutput, GeneratedImage};

    /// Succeeds with a fake PNG, except for every third call.
    #[derive(Default)]
    struct FakeGenerator {
        calls: AtomicUsize,
    }

    impl ImageGenerator for FakeGenerator {
        async fn generate(
            &self,
            request: &GenerateRequest,
        ) -> Result<GenerateOutput, GenerateError> {
            assert_eq!(request.references.len(), 1);
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call.is_multiple_of(3) {
                return Err(GenerateError::NoImage {
                    reason: "status \"failed\"".into(),
                    response: "{}".into(),
                });
            }
            Ok(GenerateOutput {
                image: GeneratedImage {
                    mime_type: "image/png".into(),
                    bytes: b"\x89PNG fake".to_vec(),
                },
                notes: None,
                usage: Usage::default(),
            })
        }
    }

    fn library() -> Library {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library");
        Library::load(&dir).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poc-cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_photo(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([200, 180, 90]))
            .save(path)
            .unwrap();
    }

    fn settings(variants: u32) -> RunSettings {
        RunSettings {
            started_at: "2026-11-12 10:00:00".into(),
            model: "fake-model".into(),
            image_size: Some("2K".into()),
            aspect_ratio: None,
            variants,
            concurrency: 2,
            per_image_usd: Some(0.05),
            note: None,
        }
    }

    #[test]
    fn photos_get_their_category_from_the_folder_name() {
        let input = temp_dir("discover");
        write_photo(&input.join("Loose Photo.JPG"));
        write_photo(&input.join("ring").join("solitaire.png"));
        write_photo(&input.join("ring").join("Solitaire.jpg"));
        write_photo(&input.join("misc").join("thing.webp.png"));
        fs::write(input.join("notes.txt"), "not a photo").unwrap();

        let library = library();
        let items = discover_items(&input, &library, None).unwrap();
        let summary: Vec<(&str, &str)> = items
            .iter()
            .map(|item| (item.id.as_str(), item.category.id.as_str()))
            .collect();

        assert_eq!(summary.len(), 4);
        assert!(summary.contains(&("loose-photo", "jewelry")));
        assert!(summary.contains(&("solitaire", "ring")));
        assert!(summary.contains(&("solitaire-2", "ring")));
        assert!(summary.contains(&("thing-webp", "jewelry")));

        let as_pendants = discover_items(&input, &library, Some("pendant")).unwrap();
        assert!(as_pendants
            .iter()
            .any(|item| item.id == "loose-photo" && item.category.id == "pendant"));
        assert!(discover_items(&input, &library, Some("tiara")).is_err());

        fs::remove_dir_all(&input).unwrap();
    }

    #[test]
    fn a_note_goes_into_every_prompt_as_typed() {
        let input = temp_dir("plan-note");
        write_photo(&input.join("ring").join("a.jpg"));
        let library = library();
        let styles: Vec<&StyleTemplate> = library.styles.iter().collect();

        let items = discover_items(&input, &library, None).unwrap();
        let planned = plan(items, &styles, &library, Some("show a side view")).unwrap();
        assert_eq!(planned.image_count(3), library.styles.len() * 3);
        assert!(planned.items[0]
            .styles
            .iter()
            .all(|style| style.prompt.text.contains("show a side view")));

        fs::remove_dir_all(&input).unwrap();
    }

    #[tokio::test]
    async fn a_run_saves_images_and_records_failures() {
        let input = temp_dir("run-input");
        let run_dir = temp_dir("run-output");
        write_photo(&input.join("ring").join("solitaire.jpg"));
        fs::write(input.join("ring").join("broken.jpg"), "not a photo").unwrap();

        let library = library();
        let styles = vec![
            library.style("black-velvet").unwrap(),
            library.style("studio-white").unwrap(),
        ];
        let items = discover_items(&input, &library, None).unwrap();
        let plan = plan(items, &styles, &library, None).unwrap();
        assert_eq!(plan.image_count(3), 12);

        let generator = Arc::new(FakeGenerator::default());
        let manifest = execute(generator, &plan, 1, &settings(3), &run_dir)
            .await
            .unwrap();

        // The broken photo is skipped before any request is made; of the six
        // requests for the good photo, calls three and six fail.
        assert_eq!(manifest.saved(), 4);
        assert_eq!(manifest.failed(), 2);
        assert!((manifest.cost_usd().unwrap() - 0.20).abs() < 1e-9);

        let broken = manifest.items.iter().find(|i| i.id == "broken").unwrap();
        assert!(broken.error.is_some() && broken.styles.is_empty());

        let report = fs::read_to_string(run_dir.join("report.html")).unwrap();
        assert!(report.contains("Black Velvet"));

        let good = manifest.items.iter().find(|i| i.id == "solitaire").unwrap();
        assert!(run_dir.join(good.reference.as_ref().unwrap()).is_file());
        for style in &good.styles {
            assert!(run_dir.join(&style.prompt_file).is_file());
            for variant in &style.variants {
                match (&variant.file, &variant.error) {
                    (Some(file), None) => {
                        assert!(run_dir.join(file).is_file());
                        assert!(report.contains(file.as_str()));
                    }
                    (None, Some(error)) => assert!(error.contains("no image")),
                    other => panic!("unexpected variant state: {other:?}"),
                }
            }
        }

        assert!(run_dir.join("run.json").is_file());
        assert!(run_dir.join("debug").is_dir());

        fs::remove_dir_all(&input).unwrap();
        fs::remove_dir_all(&run_dir).unwrap();
    }
}
