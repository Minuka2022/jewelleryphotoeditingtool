//! Everything the app does, without any window or network code: the prompt
//! library, the settings, the workspace, and planning what to generate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use studio_core::cost::Pricing;
use studio_core::export::{self, Crop, ExportOptions};
use studio_core::gemini::{GenerateRequest, GeneratedImage};
use studio_core::guardrails;
use studio_core::library::{Library, Look, NewStyle, SceneTone, StyleTemplate};
use studio_core::prompt::{build_prompt, PromptRequest};
use studio_core::reference::ReferenceImage;

use crate::store::{new_id, now, ItemRecord, OutputRecord, OutputStatus, Store};

/// Files owned by the app and rewritten on every start, so that updates can
/// improve the master prompt and the price list.
const APP_FILES: [(&str, &str); 2] = [
    ("master.toml", include_str!("../../../library/master.toml")),
    (
        "pricing.toml",
        include_str!("../../../library/pricing.toml"),
    ),
];

/// Styles a new installation starts with, by id. Each is written once; from
/// then on it belongs to the user, who may edit or delete it like any other.
const STARTER_STYLES: [(&str, &str); 4] = [
    (
        "black-velvet",
        include_str!("../../../library/styles/black-velvet.toml"),
    ),
    (
        "dark-slate",
        include_str!("../../../library/styles/dark-slate.toml"),
    ),
    (
        "studio-white",
        include_str!("../../../library/styles/studio-white.toml"),
    ),
    (
        "white-marble",
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
    /// Whether results look photographed or like a 3D render.
    pub look: Look,
    pub categories: Vec<String>,
    pub prompt: String,
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
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    #[serde(flatten)]
    pub record: OutputRecord,
    /// Full path of the image to show: the cropped copy when there is a crop.
    pub path: String,
    /// Full path of the uncropped original, which the crop tool works on.
    pub original_path: String,
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
    pub look: Look,
    #[serde(default)]
    pub categories: Vec<String>,
    pub image_path: Option<String>,
}

/// Advice on a style prompt while the user is still typing it. Nothing here
/// stops the style from being saved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCheck {
    /// Wording that may lead the AI to change the jewelry.
    pub warnings: Vec<String>,
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

/// One image to export, gathered while the workspace is locked so that the
/// slow part (decoding, resizing, encoding) can run without holding it.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub source: PathBuf,
    pub crop: Option<Crop>,
    /// File name without number or extension, e.g. "solitaire-01-black-velvet".
    pub base_name: String,
}

/// What an export would produce, shown before exporting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportEstimate {
    pub width: u32,
    pub height: u32,
    /// Size of the image after cropping and before resizing.
    pub source_width: u32,
    pub source_height: u32,
    pub bytes: usize,
}

impl ExportJob {
    pub fn estimate(&self, options: &ExportOptions) -> Result<ExportEstimate, String> {
        let rendered = export::render(&self.source, self.crop.as_ref(), options)?;
        Ok(ExportEstimate {
            width: rendered.width,
            height: rendered.height,
            source_width: rendered.source_width,
            source_height: rendered.source_height,
            bytes: rendered.bytes.len(),
        })
    }
}

/// Checks a crop and renders the copy shown on screen. Returns `None` when
/// the crop keeps the whole image, which means "no crop".
pub fn crop_preview(source: &Path, crop: Option<Crop>) -> Result<Option<(Crop, Vec<u8>)>, String> {
    let Some(crop) = crop.filter(|crop| !crop.is_full()) else {
        return Ok(None);
    };
    crop.validate()?;
    // Near-lossless JPEG with full colour detail: fine for close inspection
    // and quick to write. Exports always start again from the original.
    let options = ExportOptions {
        quality: 95,
        ..ExportOptions::default()
    };
    let rendered = export::render(source, Some(&crop), &options)?;
    Ok(Some((crop, rendered.bytes)))
}

/// Renders every job in memory with a readable file name, unique within the
/// list, for handing to a browser as a download.
pub fn render_export(
    jobs: &[ExportJob],
    options: &ExportOptions,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if jobs.is_empty() {
        return Err("There is nothing to export. Approve some images first.".into());
    }
    let extension = options.format.extension();
    let mut taken = BTreeSet::new();
    jobs.iter()
        .map(|job| {
            let rendered = export::render(&job.source, job.crop.as_ref(), options)?;
            let name = (1..)
                .map(|number| format!("{}-{number}.{extension}", job.base_name))
                .find(|name| taken.insert(name.clone()))
                .expect("an unused name always exists");
            Ok((name, rendered.bytes))
        })
        .collect()
}

/// Writes every job into `destination` with readable names, never replacing
/// an existing file, and returns how many images were written.
pub fn run_export(
    jobs: &[ExportJob],
    options: &ExportOptions,
    destination: &Path,
) -> Result<usize, String> {
    if jobs.is_empty() {
        return Err("There is nothing to export. Approve some images first.".into());
    }
    fs::create_dir_all(destination)
        .map_err(|e| format!("Cannot use the folder {}: {e}", destination.display()))?;
    for job in jobs {
        let rendered = export::render(&job.source, job.crop.as_ref(), options)?;
        let target = unused_path(destination, &job.base_name, options.format.extension());
        fs::write(&target, rendered.bytes)
            .map_err(|e| format!("Cannot write {}: {e}", target.display()))?;
    }
    Ok(jobs.len())
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
        seed_library(&data_dir)?;
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
                    look: style.look,
                    categories: style.categories.clone(),
                    prompt: style.prompt.clone(),
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
        let original = self.store.output_path(item_id, &output.file);
        let shown = output
            .crop_preview
            .as_ref()
            .map(|preview| self.store.output_path(item_id, preview))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| original.clone());
        OutputInfo {
            path: display(&shown),
            original_path: display(&original),
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
            warnings: guardrails::check(prompt)
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
        let library_dir = self.library_dir();
        self.library
            .import_style(&library_dir, input.into())
            .map_err(|e| e.to_string())
    }

    /// Saves changes to an existing style. Leaving `image_path` empty keeps
    /// the style's current picture.
    pub fn update_style(&mut self, id: &str, input: ImportStyle) -> Result<StyleTemplate, String> {
        let library_dir = self.library_dir();
        self.library
            .update_style(&library_dir, id, input.into())
            .map_err(|e| e.to_string())
    }

    pub fn remove_style(&mut self, id: &str) -> Result<(), String> {
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
            crop: None,
            crop_preview: None,
        };
        self.store
            .add_output(&plan.item_id, record.clone(), &image.bytes)?;
        Ok(self.output_info(&plan.item_id, &record))
    }

    /// Saves, or with `None` removes, the crop of a generated image.
    /// Where a generated image's original file is.
    pub fn output_source(&self, item_id: &str, output_id: &str) -> Result<PathBuf, String> {
        self.store.output_source(item_id, output_id)
    }

    /// Saves a crop together with its display copy from [`crop_preview`], or
    /// with `None` removes the crop.
    pub fn set_output_crop(
        &mut self,
        item_id: &str,
        output_id: &str,
        crop: Option<(Crop, Vec<u8>)>,
    ) -> Result<ItemInfo, String> {
        self.store.set_output_crop(item_id, output_id, crop)?;
        self.item_info(item_id)
    }

    /// Looks up the files to export. Rendering them is left to [`run_export`]
    /// so the workspace is not locked while it runs.
    pub fn export_jobs(&self, selections: &[OutputRef]) -> Result<Vec<ExportJob>, String> {
        selections
            .iter()
            .map(|selection| {
                let item = self.store.item(&selection.item_id)?;
                let output = item
                    .outputs
                    .iter()
                    .find(|output| output.id == selection.output_id)
                    .ok_or("One of the images no longer exists.")?;
                Ok(ExportJob {
                    source: self.store.output_path(&item.id, &output.file),
                    crop: output.crop,
                    base_name: format!("{}-{}", file_safe(&item.name), output.style_id),
                })
            })
            .collect()
    }

    fn require_category(&self, id: &str) -> Result<(), String> {
        match self.library.category(id) {
            Some(_) => Ok(()),
            None => Err(format!("There is no category \"{id}\".")),
        }
    }
}

impl From<ImportStyle> for NewStyle {
    fn from(input: ImportStyle) -> Self {
        Self {
            name: input.name,
            description: input.description,
            prompt: input.prompt,
            tone: input.tone,
            look: input.look,
            categories: input.categories,
            image: input
                .image_path
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from),
        }
    }
}

/// Prepares the library inside the data folder: refreshes the app's own
/// files, and gives a starter style to any data folder that has never had it.
fn seed_library(data_dir: &Path) -> Result<(), String> {
    let write = |path: &Path, content: &str| -> Result<(), String> {
        fs::write(path, content).map_err(|e| format!("Cannot write {}: {e}", path.display()))
    };
    let library_dir = data_dir.join("library");
    let styles_dir = library_dir.join("styles");
    fs::create_dir_all(&styles_dir)
        .map_err(|e| format!("Cannot create {}: {e}", styles_dir.display()))?;

    for (name, content) in APP_FILES {
        let path = library_dir.join(name);
        if !fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
            write(&path, content)?;
        }
    }

    // Remembering which starter styles were handed out is what lets the user
    // delete one without it coming back at the next start.
    let record_path = data_dir.join("starter-styles.json");
    let mut given: BTreeSet<String> = fs::read_to_string(&record_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut changed = false;
    for (id, content) in STARTER_STYLES {
        if given.insert(id.to_owned()) {
            changed = true;
            let path = styles_dir.join(format!("{id}.toml"));
            // An existing file may already carry the user's edits.
            if !path.exists() {
                write(&path, content)?;
            }
        }
    }
    if changed {
        let json = serde_json::to_string(&given).map_err(|e| e.to_string())?;
        write(&record_path, &json)?;
    }
    Ok(())
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
        assert!(library.styles.iter().all(|style| style.look == Look::Photo));
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

        // A note is never refused; it is sent as typed.
        let mut with_note = args(&item_id, &["black-velvet"], 1);
        with_note.note = Some("show it on a hand".into());
        let plan = studio.plan_generation(&with_note).unwrap();
        assert!(plan.jobs[0].request.prompt.contains("show it on a hand"));

        studio
            .import_style(ImportStyle {
                name: "Necklace Bust".into(),
                description: String::new(),
                prompt: "Place the {item} on pale linen.".into(),
                tone: SceneTone::Bright,
                look: Look::Photo,
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

    fn style_input(name: &str, prompt: &str, look: Look) -> ImportStyle {
        ImportStyle {
            name: name.into(),
            description: String::new(),
            prompt: prompt.into(),
            tone: SceneTone::Dark,
            look,
            categories: vec!["ring".into()],
            image_path: None,
        }
    }

    #[test]
    fn starter_styles_can_be_edited_and_deleted_and_stay_that_way() {
        let dir = temp_dir("starter");
        let mut studio = Studio::open(dir.join("data")).unwrap();

        let edited = studio
            .update_style(
                "black-velvet",
                style_input(
                    "Black Velvet 3D",
                    "Place the {item} on black velvet.",
                    Look::Render,
                ),
            )
            .unwrap();
        assert_eq!((edited.id.as_str(), edited.version), ("black-velvet", 2));
        studio.remove_style("dark-slate").unwrap();
        assert!(studio.remove_style("dark-slate").is_err());
        drop(studio);

        // After a restart the edit is kept and the deleted style stays gone.
        let mut studio = Studio::open(dir.join("data")).unwrap();
        let library = studio.library_info();
        let ids: Vec<&str> = library.styles.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["black-velvet", "studio-white", "white-marble"]);
        let velvet = &library.styles[0];
        assert_eq!(
            (velvet.name.as_str(), velvet.look),
            ("Black Velvet 3D", Look::Render)
        );
        assert_eq!(velvet.categories, ["ring"]);

        // Every style can go; the app still opens, and new ones can be added.
        for id in ids {
            studio.remove_style(id).unwrap();
        }
        drop(studio);
        let mut studio = Studio::open(dir.join("data")).unwrap();
        assert!(studio.library_info().styles.is_empty());
        studio
            .import_style(style_input(
                "My Look",
                "pale linen, soft light",
                Look::Photo,
            ))
            .unwrap();
        assert_eq!(studio.library_info().styles.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_data_folder_from_an_older_version_keeps_its_styles() {
        // Version 0.2.0 wrote the starter styles but kept no record of it.
        let dir = temp_dir("older");
        let styles = dir.join("data/library/styles");
        fs::create_dir_all(&styles).unwrap();
        let edited = STARTER_STYLES[0].1.replace("Black Velvet", "My Velvet");
        fs::write(styles.join("black-velvet.toml"), edited).unwrap();

        let studio = Studio::open(dir.join("data")).unwrap();
        let library = studio.library_info();
        assert_eq!(library.styles.len(), 4);
        assert_eq!(library.styles[0].name, "My Velvet");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn imported_styles_keep_their_picture_and_can_be_deleted() {
        let dir = temp_dir("styles");
        let mut studio = Studio::open(dir.join("data")).unwrap();
        let picture = photo(&dir, "look.png");

        let style = studio
            .import_style(ImportStyle {
                name: "Rose Petals".into(),
                description: "Romantic".into(),
                prompt: "scattered rose petals, soft light".into(),
                tone: SceneTone::Bright,
                look: Look::Render,
                categories: vec!["ring".into()],
                image_path: Some(picture.to_string_lossy().into_owned()),
            })
            .unwrap();
        assert_eq!(style.id, "rose-petals");

        let info = studio.library_info();
        let imported = info.styles.iter().find(|s| s.id == "rose-petals").unwrap();
        assert_eq!(imported.look, Look::Render);
        let thumbnail = imported.thumbnail.clone().unwrap();
        assert!(Path::new(&thumbnail).is_file());

        // The imported style is still there after a restart, and editing it
        // without choosing a new picture keeps the old one.
        drop(studio);
        let mut studio = Studio::open(dir.join("data")).unwrap();
        assert_eq!(studio.library_info().styles.len(), 5);
        studio
            .update_style(
                "rose-petals",
                style_input("Rose Petals", "ivory silk", Look::Render),
            )
            .unwrap();
        let info = studio.library_info();
        let edited = info.styles.iter().find(|s| s.id == "rose-petals").unwrap();
        assert_eq!(edited.thumbnail.as_deref(), Some(thumbnail.as_str()));
        assert_eq!(edited.version, 2);

        studio.remove_style("rose-petals").unwrap();
        assert_eq!(studio.library_info().styles.len(), 4);
        assert!(!Path::new(&thumbnail).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prompt_advice_points_out_risky_words() {
        let dir = temp_dir("prompt-check");
        let mut studio = Studio::open(dir.join("data")).unwrap();

        let check = studio.check_style_prompt("A gold ring worn on a hand");
        assert_eq!(check.warnings.len(), 2);
        assert_eq!(check.item_words, vec!["ring", "gold"]);

        let clean = studio.check_style_prompt("Place the {item} on pale linen, soft light");
        assert!(clean.warnings.is_empty() && clean.item_words.is_empty());

        // Advice only: the same wording saves without complaint.
        studio
            .import_style(style_input(
                "On A Hand",
                "A gold ring worn on a hand",
                Look::Photo,
            ))
            .unwrap();
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
