//! The editable library: master prompt, categories, styles and prices.
//!
//! Everything here is data in `library/*.toml`, so wording can be tuned
//! without rebuilding. Loading validates it, including running every style
//! prompt through the guardrails.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::cost::Pricing;
use crate::guardrails;
use crate::reference::{prepare_reference, PrepareOptions};

/// Replaced with the category's noun when a prompt is built.
pub const ITEM_PLACEHOLDER: &str = "{item}";

/// Category used for photos that were not given one.
pub const FALLBACK_CATEGORY: &str = "jewelry";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MasterPrompts {
    pub version: u32,
    pub global: GlobalPrompt,
    #[serde(rename = "category")]
    pub categories: Vec<JewelryCategory>,
}

/// Layer 1: sent with every request.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalPrompt {
    pub role: String,
    pub task: String,
    /// Used instead of `task` when several reference photos are attached.
    pub task_multi: String,
    pub rules: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
}

/// Layer 2: what to protect for one kind of jewelry.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JewelryCategory {
    pub id: String,
    pub label: String,
    /// How the item is named inside prompts, e.g. "pair of earrings".
    pub noun: String,
    pub focus: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SceneTone {
    Bright,
    Neutral,
    Dark,
}

/// Layer 3: one output style. It describes the scene and nothing else.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StyleTemplate {
    pub id: String,
    pub name: String,
    /// Bumped on every wording change so results stay traceable.
    pub version: u32,
    #[serde(default)]
    pub description: String,
    pub tone: SceneTone,
    /// Category ids this style may be used for. Empty means every category.
    #[serde(default)]
    pub categories: Vec<String>,
    pub prompt: String,
    #[serde(default)]
    pub avoid: Vec<String>,
    /// File name of an example picture of the look, stored next to the style
    /// file. It is shown in the style picker and never sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl StyleTemplate {
    pub fn supports(&self, category_id: &str) -> bool {
        self.categories.is_empty() || self.categories.iter().any(|id| id == category_id)
    }
}

/// What a user supplies to add a style of their own.
#[derive(Debug, Clone)]
pub struct NewStyle {
    pub name: String,
    pub description: String,
    /// The scene. `{item}` marks where the jewelry goes; it is added when missing.
    pub prompt: String,
    pub tone: SceneTone,
    /// Category ids the style is for. Empty means every category.
    pub categories: Vec<String>,
    /// Example picture of the look, copied into the library as a thumbnail.
    pub image: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub master: MasterPrompts,
    pub styles: Vec<StyleTemplate>,
    pub pricing: Pricing,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{} is not valid TOML: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("{}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
    /// A style a user tried to add was not acceptable. Worded for the user.
    #[error("{0}")]
    Rejected(String),
}

impl Library {
    /// Loads and validates `master.toml`, `pricing.toml` and `styles/*.toml`
    /// from `dir`. Any invalid file is an error.
    pub fn load(dir: &Path) -> Result<Self, LibraryError> {
        let (library, problems) = Self::load_lenient(dir)?;
        match problems.into_iter().next() {
            Some(problem) => Err(problem),
            None => Ok(library),
        }
    }

    /// Like [`Library::load`], but an invalid style file is left out and
    /// reported instead of failing the whole library, so one bad style cannot
    /// stop the app from starting.
    pub fn load_lenient(dir: &Path) -> Result<(Self, Vec<LibraryError>), LibraryError> {
        let master_path = dir.join("master.toml");
        let master: MasterPrompts = read_toml(&master_path)?;
        validate_master(&master).map_err(invalid(&master_path))?;

        let pricing_path = dir.join("pricing.toml");
        let pricing: Pricing = read_toml(&pricing_path)?;
        pricing.validate().map_err(invalid(&pricing_path))?;

        let styles_dir = dir.join("styles");
        let entries = fs::read_dir(&styles_dir).map_err(|source| LibraryError::Io {
            path: styles_dir.clone(),
            source,
        })?;
        let mut style_paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        style_paths.sort();

        let category_ids: BTreeSet<&str> =
            master.categories.iter().map(|c| c.id.as_str()).collect();
        let mut styles = Vec::with_capacity(style_paths.len());
        let mut problems = Vec::new();
        for path in &style_paths {
            let loaded = read_toml::<StyleTemplate>(path).and_then(|style| {
                let file_stem = path.file_stem().and_then(|stem| stem.to_str());
                validate_style(&style, file_stem, &category_ids).map_err(invalid(path))?;
                Ok(style)
            });
            match loaded {
                Ok(style) => styles.push(style),
                Err(problem) => problems.push(problem),
            }
        }
        if styles.is_empty() {
            return Err(problems
                .into_iter()
                .next()
                .unwrap_or(LibraryError::Invalid {
                    path: styles_dir,
                    message: "no style files found".into(),
                }));
        }
        drop(category_ids);

        let library = Self {
            master,
            styles,
            pricing,
        };
        Ok((library, problems))
    }

    pub fn category(&self, id: &str) -> Option<&JewelryCategory> {
        self.master.categories.iter().find(|c| c.id == id)
    }

    pub fn style(&self, id: &str) -> Option<&StyleTemplate> {
        self.styles.iter().find(|s| s.id == id)
    }

    /// Checks a user's style, saves it under `dir/styles/` and adds it to
    /// this library. `dir` must be the folder this library was loaded from.
    pub fn import_style(
        &mut self,
        dir: &Path,
        new: NewStyle,
    ) -> Result<StyleTemplate, LibraryError> {
        let reject = |message: &str| LibraryError::Rejected(message.to_owned());

        let name = new.name.trim();
        let id = slug(name);
        if id.is_empty() {
            return Err(reject(
                "Give the style a name that contains letters or digits.",
            ));
        }
        let styles_dir = dir.join("styles");
        let style_path = styles_dir.join(format!("{id}.toml"));
        if self.style(&id).is_some() || style_path.exists() {
            return Err(LibraryError::Rejected(format!(
                "A style named \"{name}\" already exists. Choose another name."
            )));
        }

        let scene = new.prompt.trim();
        if scene.is_empty() {
            return Err(reject("Describe the scene in the prompt."));
        }
        let prompt = if scene.contains(ITEM_PLACEHOLDER) {
            scene.to_owned()
        } else {
            format!("Place the {ITEM_PLACEHOLDER} in this scene. {scene}")
        };

        let mut style = StyleTemplate {
            id: id.clone(),
            name: name.to_owned(),
            version: 1,
            description: new.description.trim().to_owned(),
            tone: new.tone,
            categories: new.categories,
            prompt,
            avoid: Vec::new(),
            image: None,
        };
        let category_ids: BTreeSet<&str> = self
            .master
            .categories
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        validate_style(&style, Some(&id), &category_ids).map_err(|message| {
            LibraryError::Rejected(format!("This style cannot be saved: {message}"))
        })?;

        let write_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| LibraryError::Io { path, source }
        };
        fs::create_dir_all(&styles_dir).map_err(write_error(&styles_dir))?;
        if let Some(source) = &new.image {
            let thumbnail = prepare_reference(source, &THUMBNAIL)
                .map_err(|error| LibraryError::Rejected(error.to_string()))?;
            let file_name = format!("{id}.jpg");
            let image_path = styles_dir.join(&file_name);
            fs::write(&image_path, thumbnail.bytes).map_err(write_error(&image_path))?;
            style.image = Some(file_name);
        }
        let text = toml::to_string_pretty(&style).map_err(|error| LibraryError::Parse {
            path: style_path.clone(),
            message: error.to_string(),
        })?;
        fs::write(&style_path, text).map_err(write_error(&style_path))?;

        self.styles.push(style.clone());
        self.styles.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(style)
    }

    /// Deletes a style's files from `dir/styles/` and drops it from this library.
    pub fn remove_style(&mut self, dir: &Path, id: &str) -> Result<(), LibraryError> {
        let Some(index) = self.styles.iter().position(|style| style.id == id) else {
            return Err(LibraryError::Rejected(format!(
                "There is no style \"{id}\"."
            )));
        };
        let styles_dir = dir.join("styles");
        let style_path = styles_dir.join(format!("{id}.toml"));
        fs::remove_file(&style_path).map_err(|source| LibraryError::Io {
            path: style_path,
            source,
        })?;
        if let Some(image) = &self.styles[index].image {
            // The style is already gone; a leftover picture is harmless.
            let _ = fs::remove_file(styles_dir.join(image));
        }
        self.styles.remove(index);
        Ok(())
    }
}

/// Size and quality of the example picture kept with an imported style.
const THUMBNAIL: PrepareOptions = PrepareOptions {
    max_edge: 640,
    jpeg_quality: 85,
};

/// A file-name-safe id from a style's name: "Rose Petals #2" -> "rose-petals-2".
fn slug(name: &str) -> String {
    let mut id = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_end_matches('-').to_owned()
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T, LibraryError> {
    let text = fs::read_to_string(path).map_err(|source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|error| LibraryError::Parse {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn invalid(path: &Path) -> impl Fn(String) -> LibraryError + '_ {
    move |message| LibraryError::Invalid {
        path: path.to_path_buf(),
        message,
    }
}

fn validate_master(master: &MasterPrompts) -> Result<(), String> {
    let global = &master.global;
    if master.version == 0 {
        return Err("version must be 1 or higher".into());
    }
    if global.rules.is_empty() {
        return Err("[global] needs at least one rule".into());
    }
    for (name, text) in [("task", &global.task), ("task_multi", &global.task_multi)] {
        if !text.contains(ITEM_PLACEHOLDER) {
            return Err(format!("[global] {name} must mention {{item}}"));
        }
    }
    let global_texts = [&global.role, &global.task, &global.task_multi]
        .into_iter()
        .chain(&global.rules);
    for text in global_texts {
        check_placeholders(text).map_err(|message| format!("[global] {message}"))?;
    }

    let mut seen = BTreeSet::new();
    for category in &master.categories {
        let id = &category.id;
        if !is_kebab_case(id) {
            return Err(format!(
                "category id \"{id}\" must be lowercase letters, digits and dashes"
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("category \"{id}\" is defined twice"));
        }
        if category.label.trim().is_empty()
            || category.noun.trim().is_empty()
            || category.focus.trim().is_empty()
        {
            return Err(format!("category \"{id}\" needs a label, noun and focus"));
        }
        check_placeholders(&category.focus).map_err(|m| format!("category \"{id}\" {m}"))?;
    }
    if !seen.contains(FALLBACK_CATEGORY) {
        return Err(format!(
            "the fallback category \"{FALLBACK_CATEGORY}\" is missing"
        ));
    }
    Ok(())
}

fn validate_style(
    style: &StyleTemplate,
    file_stem: Option<&str>,
    category_ids: &BTreeSet<&str>,
) -> Result<(), String> {
    if !is_kebab_case(&style.id) {
        return Err(format!(
            "id \"{}\" must be lowercase letters, digits and dashes",
            style.id
        ));
    }
    if file_stem != Some(style.id.as_str()) {
        return Err(format!("id \"{}\" must match the file name", style.id));
    }
    if style.version == 0 {
        return Err("version must be 1 or higher".into());
    }
    if style.name.trim().is_empty() {
        return Err("name is empty".into());
    }
    if !style.prompt.contains(ITEM_PLACEHOLDER) {
        return Err("prompt must mention {item}".into());
    }
    check_placeholders(&style.prompt)?;
    for category in &style.categories {
        if !category_ids.contains(category.as_str()) {
            return Err(format!("unknown category \"{category}\""));
        }
    }
    let violations = guardrails::check(&style.prompt);
    if !violations.is_empty() {
        return Err(format!(
            "prompt wording is not allowed: {}. A style may describe only the scene around the item.",
            guardrails::describe(&violations)
        ));
    }
    Ok(())
}

/// Catches typos such as `{Item}` or `{items}` that would reach the model
/// as literal braces.
fn check_placeholders(text: &str) -> Result<(), String> {
    let rest = text.replace(ITEM_PLACEHOLDER, "");
    if rest.contains('{') || rest.contains('}') {
        return Err("contains a placeholder other than {item}".into());
    }
    Ok(())
}

fn is_kebab_case(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The library that ships in the repository.
    pub(crate) fn shipped_library() -> Library {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library");
        Library::load(&dir).expect("the shipped library must be valid")
    }

    fn style(prompt: &str) -> StyleTemplate {
        StyleTemplate {
            id: "test-style".into(),
            name: "Test".into(),
            version: 1,
            description: String::new(),
            tone: SceneTone::Neutral,
            categories: Vec::new(),
            prompt: prompt.into(),
            avoid: Vec::new(),
            image: None,
        }
    }

    /// A private copy of the shipped library that a test may write to.
    fn writable_library(name: &str) -> (PathBuf, Library) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library");
        let dir =
            std::env::temp_dir().join(format!("studio-library-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("styles")).unwrap();
        for file in ["master.toml", "pricing.toml"] {
            fs::copy(source.join(file), dir.join(file)).unwrap();
        }
        for entry in fs::read_dir(source.join("styles")).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, dir.join("styles").join(path.file_name().unwrap())).unwrap();
        }
        let library = Library::load(&dir).unwrap();
        (dir, library)
    }

    fn new_style(name: &str, prompt: &str) -> NewStyle {
        NewStyle {
            name: name.into(),
            description: "A test look".into(),
            prompt: prompt.into(),
            tone: SceneTone::Bright,
            categories: vec!["ring".into()],
            image: None,
        }
    }

    #[test]
    fn an_imported_style_is_saved_with_its_picture_and_loads_again() {
        let (dir, mut library) = writable_library("import");
        let picture = dir.join("look.png");
        image::RgbImage::from_pixel(1200, 800, image::Rgb([240, 200, 210]))
            .save(&picture)
            .unwrap();

        let mut new = new_style(
            "  Rose Petals #2 ",
            "Place the {item} on scattered rose petals.",
        );
        new.image = Some(picture);
        let style = library.import_style(&dir, new).unwrap();

        assert_eq!(style.id, "rose-petals-2");
        assert_eq!(style.name, "Rose Petals #2");
        assert_eq!(style.image.as_deref(), Some("rose-petals-2.jpg"));
        assert!(dir.join("styles/rose-petals-2.jpg").is_file());
        assert!(style.supports("ring") && !style.supports("necklace"));

        let reloaded = Library::load(&dir).unwrap();
        let saved = reloaded.style("rose-petals-2").unwrap();
        assert_eq!(saved.prompt, "Place the {item} on scattered rose petals.");
        assert_eq!(saved.image, style.image);

        library.remove_style(&dir, "rose-petals-2").unwrap();
        assert!(library.style("rose-petals-2").is_none());
        assert!(!dir.join("styles/rose-petals-2.toml").exists());
        assert!(!dir.join("styles/rose-petals-2.jpg").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_prompt_without_the_item_gets_it_added() {
        let (dir, mut library) = writable_library("import-prefix");
        let style = library
            .import_style(&dir, new_style("Linen", "Grey linen, soft daylight."))
            .unwrap();
        assert_eq!(
            style.prompt,
            "Place the {item} in this scene. Grey linen, soft daylight."
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unacceptable_imports_are_rejected_and_nothing_is_saved() {
        let (dir, mut library) = writable_library("import-reject");
        let rejected = [
            new_style("Worn", "The {item} worn on a hand, soft light."),
            new_style("Black Velvet", "Place the {item} on velvet."),
            new_style("???", "Place the {item} on velvet."),
            new_style("Empty", "   "),
            NewStyle {
                categories: vec!["tiara".into()],
                ..new_style("Tiaras", "Place the {item} on velvet.")
            },
            NewStyle {
                image: Some(dir.join("missing.png")),
                ..new_style("No Picture", "Place the {item} on velvet.")
            },
        ];
        let before = library.styles.len();
        for new in rejected {
            let name = new.name.clone();
            let error = library.import_style(&dir, new).unwrap_err();
            assert!(
                matches!(error, LibraryError::Rejected(_)),
                "{name}: {error}"
            );
        }
        assert_eq!(library.styles.len(), before);
        assert_eq!(Library::load(&dir).unwrap().styles.len(), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_broken_style_file_is_reported_but_does_not_stop_lenient_loading() {
        let (dir, library) = writable_library("lenient");
        fs::write(dir.join("styles/broken.toml"), "id = \"broken\"").unwrap();

        assert!(Library::load(&dir).is_err());
        let (lenient, problems) = Library::load_lenient(&dir).unwrap();
        assert_eq!(lenient.styles.len(), library.styles.len());
        assert_eq!(problems.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_become_file_safe_ids() {
        assert_eq!(slug("Rose Petals #2"), "rose-petals-2");
        assert_eq!(slug("  --Velvet--  "), "velvet");
        assert_eq!(slug("???"), "");
    }

    fn check(style: &StyleTemplate) -> Result<(), String> {
        let categories = BTreeSet::from(["ring", "jewelry"]);
        validate_style(style, Some("test-style"), &categories)
    }

    #[test]
    fn shipped_library_is_valid() {
        let library = shipped_library();
        assert!(library.styles.len() >= 4);
        assert!(library.category(FALLBACK_CATEGORY).is_some());
        assert!(library.style("black-velvet").is_some());
    }

    #[test]
    fn accepts_a_scene_only_style() {
        assert_eq!(check(&style("Place the {item} on grey linen.")), Ok(()));
    }

    #[test]
    fn rejects_a_style_that_changes_the_viewpoint() {
        let error = check(&style("Show the {item} in a side view on linen.")).unwrap_err();
        assert!(error.contains("side view"), "{error}");
    }

    #[test]
    fn rejects_a_style_without_the_item() {
        assert!(check(&style("Grey linen, soft light.")).is_err());
    }

    #[test]
    fn rejects_unknown_placeholders() {
        assert!(check(&style("Place the {item} on {surface}.")).is_err());
    }

    #[test]
    fn rejects_unknown_categories_and_mismatched_file_names() {
        let mut bad_category = style("Place the {item} on grey linen.");
        bad_category.categories = vec!["tiara".into()];
        assert!(check(&bad_category).is_err());

        let categories = BTreeSet::from(["ring"]);
        let renamed = style("Place the {item} on grey linen.");
        assert!(validate_style(&renamed, Some("other-name"), &categories).is_err());
    }

    #[test]
    fn empty_category_list_means_every_category() {
        let mut template = style("Place the {item} on grey linen.");
        assert!(template.supports("ring"));
        template.categories = vec!["necklace".into()];
        assert!(!template.supports("ring"));
    }
}
