//! Everything the app does, without any window or network code: the prompt
//! library, the settings, the workspace, and planning what to generate.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use studio_core::cost::Pricing;
use studio_core::gemini::{GenerateRequest, GeneratedImage};
use studio_core::guardrails;
use studio_core::library::{Library, NewStyle, SceneTone, StyleTemplate};
use studio_core::prompt::{build_prompt, PromptRequest};
use studio_core::reference::ReferenceImage;

use crate::store::{new_id, now, ItemRecord, OutputRecord, OutputStatus, Store};

/// The library that ships with the app. These files are owned by the app and
/// rewritten on every start, so updates can improve them. Styles the user
/// imports live next to them under other names and are never touched.
const BUILTIN_FILES: [(&str, &str); 6] = [
    ("master.toml", include_str!("../../../library/master.toml")),
    (
        "pricing.toml",
        include_str!("../../../library/pricing.toml"),
    ),
    (
        "styles/black-velvet.toml",
        include_str!("../../../library/styles/black-velvet.toml"),
    ),
    (
        "styles/dark-slate.toml",
        include_str!("../../../library/styles/dark-slate.toml"),
    ),
    (
        "styles/studio-white.toml",
        include_str!("../../../library/styles/studio-white.toml"),
    ),
    (
        "styles/white-marble.toml",
        include_str!("../../../library/styles/white-marble.toml"),
    ),
];

const MAX_VARIANTS: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub model: String,
    pub image_size: String,
    /// `None` lets the model follow the reference photo's shape.
    pub aspect_ratio: Option<String>,
    pub variants: u32,
}

impl Settings {
    fn defaults(pricing: &Pricing) -> Self {
        Self {
            model: pricing.default_model.clone(),
            image_size: pricing.default_size.clone(),
            aspect_ratio: None,
            variants: 2,
        }
    }

    /// Replaces anything the current price list no longer offers.
    fn normalized(mut self, pricing: &Pricing) -> Self {
        if pricing.model(&self.model).is_none() {
            self.model = pricing.default_model.clone();
        }
        if pricing.per_image(&self.model, &self.image_size).is_none() {
            let sizes = pricing.model(&self.model).map(|model| &model.per_image);
            self.image_size = match sizes {
                Some(sizes) if sizes.contains_key(&pricing.default_size) => {
                    pricing.default_size.clone()
                }
                Some(sizes) => sizes.keys().next().cloned().unwrap_or_default(),
                None => pricing.default_size.clone(),
            };
        }
        self.aspect_ratio = self
            .aspect_ratio
            .map(|ratio| ratio.trim().to_owned())
            .filter(|ratio| !ratio.is_empty());
        self.variants = self.variants.clamp(1, MAX_VARIANTS);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub categories: Vec<CategoryInfo>,
    pub styles: Vec<StyleInfo>,
    pub models: Vec<ModelInfo>,
    pub prices_checked_on: String,
    /// Style files that were skipped because they are invalid.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryInfo {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleInfo {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub description: String,
    pub tone: SceneTone,
    pub categories: Vec<String>,
    pub prompt: String,
    /// Built-in styles cannot be deleted.
    pub builtin: bool,
    /// Full path of the example picture, if the style has one.
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub note: String,
    /// US dollars per image, by image size.
    pub prices: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub width: u32,
    pub height: u32,
    pub created_at: String,
    /// Full path of the prepared reference photo.
    pub reference: String,
    pub outputs: Vec<OutputInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputInfo {
    #[serde(flatten)]
    pub record: OutputRecord,
    /// Full path of the image file.
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddItemsResult {
    pub items: Vec<ItemInfo>,
    /// One message per photo that could not be added.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStyle {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    pub tone: SceneTone,
    #[serde(default)]
    pub categories: Vec<String>,
    pub image_path: Option<String>,
}

/// Feedback on a style prompt while the user is still typing it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCheck {
    /// Wording that will stop the style from being saved.
    pub errors: Vec<String>,
    /// Words that may describe the jewelry instead of the scene.
    pub item_words: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateArgs {
    pub item_id: String,
    pub style_ids: Vec<String>,
    pub variants: u32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRef {
    pub item_id: String,
    pub output_id: String,
}

/// One image to generate.
#[derive(Debug, Clone)]
pub struct Job {
    pub style_id: String,
    pub style_name: String,
    pub style_version: u32,
    /// 1-based option number within the style.
    pub number: u32,
    pub request: GenerateRequest,
}

/// Everything one press of Generate will do.
#[derive(Debug, Clone)]
pub struct GenerationPlan {
    pub item_id: String,
    pub run_id: String,
    pub jobs: Vec<Job>,
    pub model: String,
    pub image_size: String,
    pub per_image_usd: Option<f64>,
    pub note: Option<String>,
}

pub struct Studio {
    data_dir: PathBuf,
    library: Library,
    problems: Vec<String>,
    store: Store,
    settings: Settings,
}

impl Studio {
    /// Opens the app's data folder, creating and refreshing it as needed.
    pub fn open(data_dir: PathBuf) -> Result<Self, String> {
        let library_dir = data_dir.join("library");
        seed_library(&library_dir)?;
        let (library, problems) = Library::load_lenient(&library_dir).map_err(|e| e.to_string())?;
        let store = Store::open(data_dir.join("items"))?;
        let settings = fs::read_to_string(data_dir.join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_else(|| Settings::defaults(&library.pricing))
            .normalized(&library.pricing);
        Ok(Self {
            data_dir,
            library,
            problems: problems.iter().map(ToString::to_string).collect(),
            store,
            settings,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn library_dir(&self) -> PathBuf {
        self.data_dir.join("library")
    }

    pub fn library_info(&self) -> LibraryInfo {
        let styles_dir = self.library_dir().join("styles");
        let pricing = &self.library.pricing;
        LibraryInfo {
            categories: self
                .library
                .master
                .categories
                .iter()
                .map(|category| CategoryInfo {
                    id: category.id.clone(),
                    label: category.label.clone(),
                })
                .collect(),
            styles: self
                .library
                .styles
                .iter()
                .map(|style| StyleInfo {
                    id: style.id.clone(),
                    name: style.name.clone(),
                    version: style.version,
                    description: style.description.clone(),
                    tone: style.tone,
                    categories: style.categories.clone(),
                    prompt: style.prompt.clone(),
                    builtin: is_builtin(&style.id),
                    thumbnail: style
                        .image
                        .as_ref()
                        .map(|image| styles_dir.join(image))
                        .filter(|path| path.is_file())
                        .map(|path| display(&path)),
                })
                .collect(),
            models: pricing
                .models
                .iter()
                .map(|model| ModelInfo {
                    id: model.id.clone(),
                    label: model.label.clone(),
                    note: model.note.clone(),
                    prices: model.per_image.clone(),
                })
                .collect(),
            prices_checked_on: pricing.checked_on.clone(),
            problems: self.problems.clone(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn save_settings(&mut self, settings: Settings) -> Result<(), String> {
        let settings = settings.normalized(&self.library.pricing);
        let path = self.data_dir.join("settings.json");
        let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| format!("Cannot save settings: {e}"))?;
        self.settings = settings;
        Ok(())
    }

    pub fn items_info(&self) -> Vec<ItemInfo> {
        self.store
            .items()
            .iter()
            .map(|item| self.to_info(item))
            .collect()
    }

    pub fn item_info(&self, id: &str) -> Result<ItemInfo, String> {
        self.store.item(id).map(|item| self.to_info(item))
    }

    fn to_info(&self, item: &ItemRecord) -> ItemInfo {
        ItemInfo {
            id: item.id.clone(),
            name: item.name.clone(),
            category: item.category.clone(),
            width: item.width,
            height: item.height,
            created_at: item.created_at.clone(),
            reference: display(&self.store.reference_path(&item.id)),
            outputs: item
                .outputs
                .iter()
                .map(|output| self.output_info(&item.id, output))
                .collect(),
        }
    }

    fn output_info(&self, item_id: &str, output: &OutputRecord) -> OutputInfo {
        OutputInfo {
            path: display(&self.store.output_path(item_id, &output.file)),
            record: output.clone(),
        }
    }

    /// Adds photos to the workspace. A photo that cannot be read is reported
    /// and the rest are still added.
    pub fn add_items(
        &mut self,
        paths: &[PathBuf],
        category: &str,
    ) -> Result<AddItemsResult, String> {
        self.require_category(category)?;
        let mut added = Vec::new();
        let mut errors = Vec::new();
        for path in paths {
            match self.store.add_item(path, category) {
                Ok(item) => added.push(item.id.clone()),
                Err(error) => errors.push(error),
            }
        }
        let items = added
            .iter()
            .filter_map(|id| self.item_info(id).ok())
            .collect();
        Ok(AddItemsResult { items, errors })
    }

    pub fn update_item(
        &mut self,
        id: &str,
        name: Option<String>,
        category: Option<String>,
    ) -> Result<ItemInfo, String> {
        if let Some(category) = &category {
            self.require_category(category)?;
        }
        self.store.update_item(id, name, category)?;
        self.item_info(id)
    }

    pub fn remove_item(&mut self, id: &str) -> Result<(), String> {
        self.store.remove_item(id)
    }

    pub fn set_output_status(
        &mut self,
        item_id: &str,
        output_id: &str,
        status: OutputStatus,
    ) -> Result<ItemInfo, String> {
        self.store.set_output_status(item_id, output_id, status)?;
        self.item_info(item_id)
    }

    pub fn remove_output(&mut self, item_id: &str, output_id: &str) -> Result<ItemInfo, String> {
        self.store.remove_output(item_id, output_id)?;
        self.item_info(item_id)
    }

    pub fn check_style_prompt(&self, prompt: &str) -> PromptCheck {
        PromptCheck {
            errors: guardrails::check(prompt)
                .iter()
                .map(ToString::to_string)
                .collect(),
            item_words: guardrails::item_words(prompt)
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }

    pub fn import_style(&mut self, input: ImportStyle) -> Result<StyleTemplate, String> {
        let new = NewStyle {
            name: input.name,
            description: input.description,
            prompt: input.prompt,
            tone: input.tone,
            categories: input.categories,
            image: input
                .image_path
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from),
        };
        let library_dir = self.library_dir();
        self.library
            .import_style(&library_dir, new)
            .map_err(|e| e.to_string())
    }

    pub fn remove_style(&mut self, id: &str) -> Result<(), String> {
        if is_builtin(id) {
            return Err("Built-in styles cannot be deleted.".into());
        }
        let library_dir = self.library_dir();
        self.library
            .remove_style(&library_dir, id)
            .map_err(|e| e.to_string())
    }

    /// Works out every request one press of Generate will send, or explains
    /// why it cannot go ahead. Nothing is sent or charged here.
    pub fn plan_generation(&self, args: &GenerateArgs) -> Result<GenerationPlan, String> {
        let item = self.store.item(&args.item_id)?;
        let category = self
            .library
            .category(&item.category)
            .ok_or("This photo's category no longer exists. Choose another one.")?;
        if args.style_ids.is_empty() {
            return Err("Choose at least one style.".into());
        }
        if !(1..=MAX_VARIANTS).contains(&args.variants) {
            return Err(format!(
                "Options per style must be between 1 and {MAX_VARIANTS}."
            ));
        }

        let reference_path = self.store.reference_path(&item.id);
        let bytes = fs::read(&reference_path)
            .map_err(|e| format!("Cannot read this photo's reference image: {e}"))?;
        let reference = Arc::new(ReferenceImage {
            mime_type: "image/jpeg".into(),
            bytes,
            width: item.width,
            height: item.height,
        });

        let note = args
            .note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty());
        let settings = &self.settings;
        let mut jobs = Vec::new();
        for style_id in &args.style_ids {
            let style = self
                .library
                .style(style_id)
                .ok_or_else(|| format!("The style \"{style_id}\" no longer exists."))?;
            let prompt = build_prompt(&PromptRequest {
                master: &self.library.master,
                category,
                style,
                reference_count: 1,
                user_note: note,
            })
            .map_err(|error| match error {
                studio_core::prompt::PromptError::StyleNotForCategory { .. } => format!(
                    "\"{}\" is not set up for {}. Deselect it or change this photo's category.",
                    style.name, category.label
                ),
                other => other.to_string(),
            })?;
            for number in 1..=args.variants {
                jobs.push(Job {
                    style_id: style.id.clone(),
                    style_name: style.name.clone(),
                    style_version: style.version,
                    number,
                    request: GenerateRequest {
                        model: settings.model.clone(),
                        prompt: prompt.text.clone(),
                        references: vec![reference.clone()],
                        aspect_ratio: settings.aspect_ratio.clone(),
                        image_size: Some(settings.image_size.clone()),
                    },
                });
            }
        }

        Ok(GenerationPlan {
            item_id: item.id.clone(),
            run_id: new_id("r"),
            jobs,
            model: settings.model.clone(),
            image_size: settings.image_size.clone(),
            per_image_usd: self
                .library
                .pricing
                .per_image(&settings.model, &settings.image_size),
            note: note.map(str::to_owned),
        })
    }

    /// Saves one generated image into the workspace.
    pub fn record_output(
        &mut self,
        plan: &GenerationPlan,
        job: &Job,
        image: &GeneratedImage,
    ) -> Result<OutputInfo, String> {
        let id = new_id("o");
        let record = OutputRecord {
            file: format!("{id}.{}", image.extension()),
            id,
            run_id: plan.run_id.clone(),
            style_id: job.style_id.clone(),
            style_name: job.style_name.clone(),
            style_version: job.style_version,
            status: OutputStatus::None,
            created_at: now(),
            model: plan.model.clone(),
            image_size: plan.image_size.clone(),
            cost_usd: plan.per_image_usd,
            note: plan.note.clone(),
        };
        self.store
            .add_output(&plan.item_id, record.clone(), &image.bytes)?;
        Ok(self.output_info(&plan.item_id, &record))
    }

    /// Copies the chosen images into `destination` with readable names and
    /// returns how many were copied. Existing files are never overwritten.
    pub fn export(&self, selections: &[OutputRef], destination: &Path) -> Result<usize, String> {
        if selections.is_empty() {
            return Err("There is nothing to export. Approve some images first.".into());
        }
        fs::create_dir_all(destination)
            .map_err(|e| format!("Cannot use the folder {}: {e}", destination.display()))?;
        let mut copied = 0;
        for selection in selections {
            let item = self.store.item(&selection.item_id)?;
            let output = item
                .outputs
                .iter()
                .find(|output| output.id == selection.output_id)
                .ok_or("One of the images no longer exists.")?;
            let source = self.store.output_path(&item.id, &output.file);
            let extension = Path::new(&output.file)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("png");
            let base = format!("{}-{}", file_safe(&item.name), output.style_id);
            let target = unused_path(destination, &base, extension);
            fs::copy(&source, &target)
                .map_err(|e| format!("Cannot copy to {}: {e}", target.display()))?;
            copied += 1;
        }
        Ok(copied)
    }

    fn require_category(&self, id: &str) -> Result<(), String> {
        match self.library.category(id) {
            Some(_) => Ok(()),
            None => Err(format!("There is no category \"{id}\".")),
        }
    }
}

fn seed_library(dir: &Path) -> Result<(), String> {
    for (relative, content) in BUILTIN_FILES {
        let path = dir.join(relative);
        if fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&path, content).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn is_builtin(style_id: &str) -> bool {
    let file = format!("styles/{style_id}.toml");
    BUILTIN_FILES.iter().any(|(relative, _)| *relative == file)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A file name made from a photo's display name.
fn file_safe(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() {
        "jewelry".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// `base-1.ext`, `base-2.ext`, ... : the first that does not exist yet.
fn unused_path(dir: &Path, base: &str, extension: &str) -> PathBuf {
    (1..)
        .map(|number| dir.join(format!("{base}-{number}.{extension}")))
        .find(|path| !path.exists())
        .expect("an unused file name always exists")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jewelry-studio-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) fn photo(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(64, 48, image::Rgb([200, 180, 90]))
            .save(&path)
            .unwrap();
        path
    }

    pub(crate) fn studio_with_ring(name: &str) -> (PathBuf, Studio, String) {
        let dir = temp_dir(name);
        let mut studio = Studio::open(dir.join("data")).unwrap();
        let added = studio
            .add_items(&[photo(&dir, "Gold Ring 01.jpg")], "ring")
            .unwrap();
        assert!(added.errors.is_empty(), "{:?}", added.errors);
        let item_id = added.items[0].id.clone();
        (dir, studio, item_id)
    }

    fn args(item_id: &str, styles: &[&str], variants: u32) -> GenerateArgs {
        GenerateArgs {
            item_id: item_id.to_owned(),
            style_ids: styles.iter().map(|s| s.to_string()).collect(),
            variants,
            note: None,
        }
    }

    #[test]
    fn a_new_data_folder_gets_the_built_in_library() {
        let dir = temp_dir("seed");
        let studio = Studio::open(dir.join("data")).unwrap();
        let library = studio.library_info();

        assert_eq!(library.styles.len(), 4);
        assert!(library.styles.iter().all(|style| style.builtin));
        assert!(library.categories.iter().any(|c| c.id == "ring"));
        assert!(library.problems.is_empty());
        assert_eq!(studio.settings().model, "gemini-nano-banana-2.1");
        assert_eq!(studio.settings().variants, 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn photos_and_settings_survive_a_restart() {
        let (dir, mut studio, item_id) = studio_with_ring("restart");
        studio
            .update_item(&item_id, Some("Solitaire".into()), Some("pendant".into()))
            .unwrap();
        let mut settings = studio.settings().clone();
        settings.variants = 3;
        settings.aspect_ratio = Some("1:1".into());
        studio.save_settings(settings).unwrap();
        drop(studio);

        let reopened = Studio::open(dir.join("data")).unwrap();
        let item = reopened.item_info(&item_id).unwrap();
        assert_eq!(
            (item.name.as_str(), item.category.as_str()),
            ("Solitaire", "pendant")
        );
        assert_eq!((item.width, item.height), (64, 48));
        assert!(Path::new(&item.reference).is_file());
        assert_eq!(reopened.settings().variants, 3);
        assert_eq!(reopened.settings().aspect_ratio.as_deref(), Some("1:1"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_photos_are_reported_and_the_rest_are_added() {
        let dir = temp_dir("bad-photo");
        let mut studio = Studio::open(dir.join("data")).unwrap();
        let broken = dir.join("broken.jpg");
        fs::write(&broken, "not a photo").unwrap();

        let result = studio
            .add_items(&[broken, photo(&dir, "good.png")], "ring")
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(studio.add_items(&[], "tiara").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_that_are_no_longer_offered_fall_back_to_defaults() {
        let dir = temp_dir("settings");
        let mut studio = Studio::open(dir.join("data")).unwrap();
        studio
            .save_settings(Settings {
                model: "retired-model".into(),
                image_size: "9K".into(),
                aspect_ratio: Some("  ".into()),
                variants: 99,
            })
            .unwrap();
        let settings = studio.settings();
        assert_eq!(settings.model, "gemini-nano-banana-2.1");
        assert_eq!(settings.image_size, "2K");
        assert_eq!(settings.aspect_ratio, None);
        assert_eq!(settings.variants, MAX_VARIANTS);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_plan_has_one_request_per_style_and_option() {
        let (dir, studio, item_id) = studio_with_ring("plan");
        let plan = studio
            .plan_generation(&args(&item_id, &["black-velvet", "studio-white"], 3))
            .unwrap();

        assert_eq!(plan.jobs.len(), 6);
        assert_eq!(plan.per_image_usd, Some(0.0504));
        let first = &plan.jobs[0];
        assert_eq!((first.style_id.as_str(), first.number), ("black-velvet", 1));
        assert_eq!(first.request.references.len(), 1);
        assert_eq!(first.request.image_size.as_deref(), Some("2K"));
        assert!(first
            .request
            .prompt
            .contains("Place the ring on rich black velvet"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_plan_is_refused_with_a_reason() {
        let (dir, mut studio, item_id) = studio_with_ring("plan-refused");
        let refused =
            |studio: &Studio, args: GenerateArgs| studio.plan_generation(&args).unwrap_err();

        assert!(refused(&studio, args(&item_id, &[], 1)).contains("at least one style"));
        assert!(refused(&studio, args(&item_id, &["black-velvet"], 0)).contains("between 1 and"));
        assert!(refused(&studio, args(&item_id, &["gone"], 1)).contains("no longer exists"));
        assert!(refused(&studio, args("missing", &["black-velvet"], 1)).contains("no longer"));

        let mut with_note = args(&item_id, &["black-velvet"], 1);
        with_note.note = Some("show it on a hand".into());
        assert!(refused(&studio, with_note).contains("not allowed"));

        studio
            .import_style(ImportStyle {
                name: "Necklace Bust".into(),
                description: String::new(),
                prompt: "Place the {item} on pale linen.".into(),
                tone: SceneTone::Bright,
                categories: vec!["necklace".into()],
                image_path: None,
            })
            .unwrap();
        let wrong_category = refused(&studio, args(&item_id, &["necklace-bust"], 1));
        assert!(
            wrong_category.contains("not set up for Ring"),
            "{wrong_category}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn imported_styles_can_be_deleted_but_built_in_ones_cannot() {
        let dir = temp_dir("styles");
        let mut studio = Studio::open(dir.join("data")).unwrap();
        let picture = photo(&dir, "look.png");

        let style = studio
            .import_style(ImportStyle {
                name: "Rose Petals".into(),
                description: "Romantic".into(),
                prompt: "scattered rose petals, soft light".into(),
                tone: SceneTone::Bright,
                categories: vec!["ring".into()],
                image_path: Some(picture.to_string_lossy().into_owned()),
            })
            .unwrap();
        assert_eq!(style.id, "rose-petals");

        let info = studio.library_info();
        let imported = info.styles.iter().find(|s| s.id == "rose-petals").unwrap();
        assert!(!imported.builtin);
        assert!(Path::new(imported.thumbnail.as_ref().unwrap()).is_file());

        // The imported style is still there after a restart.
        drop(studio);
        let mut studio = Studio::open(dir.join("data")).unwrap();
        assert_eq!(studio.library_info().styles.len(), 5);

        assert!(studio.remove_style("black-velvet").is_err());
        studio.remove_style("rose-petals").unwrap();
        assert_eq!(studio.library_info().styles.len(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prompt_feedback_separates_errors_from_advice() {
        let dir = temp_dir("prompt-check");
        let studio = Studio::open(dir.join("data")).unwrap();

        let check = studio.check_style_prompt("A gold ring worn on a hand");
        assert_eq!(check.errors.len(), 2);
        assert_eq!(check.item_words, vec!["ring", "gold"]);

        let clean = studio.check_style_prompt("Place the {item} on pale linen, soft light");
        assert!(clean.errors.is_empty() && clean.item_words.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_names_are_safe_and_never_reused() {
        assert_eq!(file_safe(" Gold Ring: #1 "), "Gold-Ring---1");
        assert_eq!(file_safe("///"), "jewelry");

        let dir = temp_dir("unused");
        assert_eq!(unused_path(&dir, "ring", "png"), dir.join("ring-1.png"));
        fs::write(dir.join("ring-1.png"), "x").unwrap();
        assert_eq!(unused_path(&dir, "ring", "png"), dir.join("ring-2.png"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
