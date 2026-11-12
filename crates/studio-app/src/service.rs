//! The operations the interface can ask for. The desktop app and the web
//! version both call these, so they behave identically.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use studio_core::export::{Crop, ExportOptions};
use studio_core::gemini::GeminiClient;

use crate::generate::{self, Progress};
use crate::secrets;
use crate::store::OutputStatus;
use crate::studio::{
    crop_preview, render_export, run_export, ExportEstimate, GenerateArgs, ImportStyle, ItemInfo,
    LibraryInfo, OutputRef, Settings, Studio,
};

pub type Shared = Mutex<Studio>;

pub const NO_KEY: &str = "Add your Gemini API key first: File > Settings.";
const BILLING_HELP: &str = "Google refused the request because this API key's project is on \
    the free tier, which does not include image models. Open https://ai.dev/rate-limit with the \
    Google account that owns the key, set up billing, then try again. Nothing was charged.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInfo {
    #[serde(flatten)]
    pub settings: Settings,
    pub has_api_key: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedStyle {
    pub style_id: String,
    pub library: LibraryInfo,
}

pub fn lock(studio: &Shared) -> Result<MutexGuard<'_, Studio>, String> {
    studio
        .lock()
        .map_err(|_| "The app's data is unavailable. Please restart Jewelry Studio.".to_owned())
}

pub fn settings_info(studio: &Studio) -> SettingsInfo {
    SettingsInfo {
        settings: studio.settings().clone(),
        has_api_key: secrets::load().is_some(),
    }
}

/// `GEMINI_BASE_URL` redirects requests to a local test server or a proxy.
pub fn new_client(api_key: String) -> Result<GeminiClient, String> {
    let client = GeminiClient::new(api_key).map_err(|e| e.to_string())?;
    Ok(match std::env::var("GEMINI_BASE_URL") {
        Ok(base_url) if !base_url.trim().is_empty() => client.with_base_url(base_url.trim()),
        _ => client,
    })
}

/// Google words a missing billing setup as a rate limit of zero.
pub fn explain(error: String) -> String {
    if error.contains("Free Tier") {
        BILLING_HELP.to_owned()
    } else {
        error
    }
}

/// Runs slow image work on a background thread so the interface stays responsive.
async fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| format!("The work stopped unexpectedly: {e}"))?
}

pub fn set_api_key(studio: &Shared, key: &str) -> Result<SettingsInfo, String> {
    secrets::save(key)?;
    Ok(settings_info(&*lock(studio)?))
}

/// Asks Google which models the saved key can use. Free of charge.
pub async fn check_api_key(studio: &Shared) -> Result<String, String> {
    let api_key = secrets::load().ok_or(NO_KEY)?;
    let model = lock(studio)?.settings().model.clone();
    let models = new_client(api_key)?
        .list_models()
        .await
        .map_err(|e| explain(e.to_string()))?;
    if models.contains(&model) {
        Ok(format!("The key works and {model} is available."))
    } else {
        Err(format!(
            "The key works, but it cannot use {model}. Choose another model in Settings."
        ))
    }
}

/// Generates every style and option for one photo, reporting each image as
/// it arrives, and returns the finished photo.
pub async fn generate(
    studio: &Shared,
    args: GenerateArgs,
    on_progress: impl FnMut(Progress) + Send,
) -> Result<ItemInfo, String> {
    let api_key = secrets::load().ok_or(NO_KEY)?;
    let plan = lock(studio)?.plan_generation(&args)?;
    let item_id = plan.item_id.clone();
    let client = Arc::new(new_client(api_key)?);

    let summary = generate::run(client, studio, plan, on_progress).await;
    if summary.saved == 0 {
        let error = summary
            .first_error
            .unwrap_or_else(|| "No image could be generated.".to_owned());
        return Err(explain(error));
    }
    lock(studio)?.item_info(&item_id)
}

/// Saves a crop (or removes it with `None`) and makes the copy shown on screen.
pub async fn set_output_crop(
    studio: &Shared,
    item_id: &str,
    output_id: &str,
    crop: Option<Crop>,
) -> Result<ItemInfo, String> {
    let source = lock(studio)?.output_source(item_id, output_id)?;
    let cropped = in_background(move || crop_preview(&source, crop)).await?;
    lock(studio)?.set_output_crop(item_id, output_id, cropped)
}

/// Size in pixels and in bytes that exporting one image would produce.
pub async fn estimate_export(
    studio: &Shared,
    selection: OutputRef,
    options: ExportOptions,
) -> Result<ExportEstimate, String> {
    let job = lock(studio)?
        .export_jobs(std::slice::from_ref(&selection))?
        .remove(0);
    in_background(move || job.estimate(&options)).await
}

/// Crops, resizes and encodes the chosen images into a folder.
pub async fn export_to_folder(
    studio: &Shared,
    selections: Vec<OutputRef>,
    options: ExportOptions,
    destination: PathBuf,
) -> Result<usize, String> {
    let jobs = lock(studio)?.export_jobs(&selections)?;
    in_background(move || run_export(&jobs, &options, &destination)).await
}

/// Like [`export_to_folder`], but returns the files instead of writing them:
/// each with a readable file name, unique within the list.
pub async fn export_files(
    studio: &Shared,
    selections: Vec<OutputRef>,
    options: ExportOptions,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let jobs = lock(studio)?.export_jobs(&selections)?;
    in_background(move || render_export(&jobs, &options)).await
}

pub fn import_style(studio: &Shared, style: ImportStyle) -> Result<ImportedStyle, String> {
    let mut studio = lock(studio)?;
    let imported = studio.import_style(style)?;
    Ok(ImportedStyle {
        style_id: imported.id,
        library: studio.library_info(),
    })
}

pub fn update_style(
    studio: &Shared,
    id: &str,
    style: ImportStyle,
) -> Result<ImportedStyle, String> {
    let mut studio = lock(studio)?;
    let updated = studio.update_style(id, style)?;
    Ok(ImportedStyle {
        style_id: updated.id,
        library: studio.library_info(),
    })
}

pub fn remove_style(studio: &Shared, id: &str) -> Result<LibraryInfo, String> {
    let mut studio = lock(studio)?;
    studio.remove_style(id)?;
    Ok(studio.library_info())
}

// --- Calling an operation by name ------------------------------------------
//
// The web version receives `POST /api/invoke/<name>` with the arguments as
// JSON, exactly as the interface sends them to the desktop app.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemArgs {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutputArgs {
    item_id: String,
    output_id: String,
    #[serde(default)]
    status: Option<OutputStatus>,
    #[serde(default)]
    crop: Option<Crop>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StyleArgs {
    #[serde(default)]
    id: String,
    style: ImportStyle,
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("The request's \"{name}\" is not valid: {e}"))
}

fn whole<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("The request is not valid: {e}"))
}

fn json(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// Runs the operation called `command`. Returns `None` for names this layer
/// does not know, so the caller can handle its own (such as updates).
pub async fn dispatch(
    studio: &Shared,
    command: &str,
    args: Value,
    on_progress: impl FnMut(Progress) + Send,
) -> Option<Result<Value, String>> {
    let result = match command {
        "get_library" => lock(studio).and_then(|s| json(s.library_info())),
        "get_settings" => lock(studio).and_then(|s| json(settings_info(&s))),
        "save_settings" => match arg::<Settings>(&args, "settings") {
            Ok(settings) => lock(studio).and_then(|mut s| {
                s.save_settings(settings)?;
                json(settings_info(&s))
            }),
            Err(e) => Err(e),
        },
        "set_api_key" => arg::<String>(&args, "key")
            .and_then(|key| set_api_key(studio, &key))
            .and_then(json),
        "check_api_key" => check_api_key(studio).await.and_then(json),
        "list_items" => lock(studio).and_then(|s| json(s.items_info())),
        "add_items" => match (
            arg::<Vec<PathBuf>>(&args, "paths"),
            arg::<String>(&args, "category"),
        ) {
            (Ok(paths), Ok(category)) => {
                lock(studio).and_then(|mut s| json(s.add_items(&paths, &category)?))
            }
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "update_item" => whole::<ItemArgs>(args).and_then(|a| {
            lock(studio).and_then(|mut s| json(s.update_item(&a.id, a.name, a.category)?))
        }),
        "remove_item" => whole::<ItemArgs>(args)
            .and_then(|a| lock(studio).and_then(|mut s| s.remove_item(&a.id)))
            .and_then(json),
        "set_output_status" => whole::<OutputArgs>(args).and_then(|a| {
            let status = a.status.ok_or("The request has no status.")?;
            lock(studio)
                .and_then(|mut s| json(s.set_output_status(&a.item_id, &a.output_id, status)?))
        }),
        "remove_output" => whole::<OutputArgs>(args).and_then(|a| {
            lock(studio).and_then(|mut s| json(s.remove_output(&a.item_id, &a.output_id)?))
        }),
        "set_output_crop" => match whole::<OutputArgs>(args) {
            Ok(a) => set_output_crop(studio, &a.item_id, &a.output_id, a.crop)
                .await
                .and_then(json),
            Err(e) => Err(e),
        },
        "estimate_export" => match (
            arg::<OutputRef>(&args, "selection"),
            arg::<ExportOptions>(&args, "options"),
        ) {
            (Ok(selection), Ok(options)) => estimate_export(studio, selection, options)
                .await
                .and_then(json),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "check_style_prompt" => arg::<String>(&args, "prompt")
            .and_then(|prompt| lock(studio).and_then(|s| json(s.check_style_prompt(&prompt)))),
        "import_style" => arg::<ImportStyle>(&args, "style")
            .and_then(|style| import_style(studio, style))
            .and_then(json),
        "update_style" => whole::<StyleArgs>(args)
            .and_then(|a| update_style(studio, &a.id, a.style))
            .and_then(json),
        "remove_style" => arg::<String>(&args, "id")
            .and_then(|id| remove_style(studio, &id))
            .and_then(json),
        "generate" => match arg::<GenerateArgs>(&args, "args") {
            Ok(generate_args) => generate(studio, generate_args, on_progress)
                .await
                .and_then(json),
            Err(e) => Err(e),
        },
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::tests::{photo, temp_dir};
    use serde_json::json;

    fn call(studio: &Shared, command: &str, args: Value) -> Result<Value, String> {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(dispatch(studio, command, args, |_| {}))
            .unwrap_or_else(|| Err(format!("unknown command {command}")))
    }

    #[test]
    fn operations_can_be_called_by_name_with_the_interfaces_arguments() {
        let dir = temp_dir("dispatch");
        let studio = Mutex::new(Studio::open(dir.join("data")).unwrap());

        let library = call(&studio, "get_library", json!({})).unwrap();
        assert_eq!(library["styles"].as_array().unwrap().len(), 4);

        let added = call(
            &studio,
            "add_items",
            json!({ "paths": [photo(&dir, "ring.png")], "category": "ring" }),
        )
        .unwrap();
        let item_id = added["items"][0]["id"].as_str().unwrap().to_owned();

        let renamed = call(
            &studio,
            "update_item",
            json!({ "id": item_id, "name": "Solitaire" }),
        )
        .unwrap();
        assert_eq!(renamed["name"], "Solitaire");

        let style = json!({ "name": "Linen", "prompt": "grey linen", "tone": "bright", "look": "render", "categories": ["ring"], "imagePath": null });
        let imported = call(&studio, "import_style", json!({ "style": style })).unwrap();
        assert_eq!(imported["styleId"], "linen");
        let edited = call(
            &studio,
            "update_style",
            json!({ "id": "linen", "style": { "name": "Linen 2", "prompt": "grey linen", "tone": "dark", "look": "render", "categories": [], "imagePath": null } }),
        )
        .unwrap();
        assert!(edited["library"]["styles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "Linen 2"));
        call(&studio, "remove_style", json!({ "id": "linen" })).unwrap();

        let check = call(
            &studio,
            "check_style_prompt",
            json!({ "prompt": "worn on a hand" }),
        )
        .unwrap();
        assert_eq!(check["warnings"].as_array().unwrap().len(), 2);

        // Mistakes come back as readable messages, not crashes.
        let error = call(&studio, "add_items", json!({ "category": "ring" })).unwrap_err();
        assert!(error.contains("\"paths\""), "{error}");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime
            .block_on(dispatch(&studio, "no_such_command", json!({}), |_| {}))
            .is_none());

        call(&studio, "remove_item", json!({ "id": item_id })).unwrap();
        assert_eq!(call(&studio, "list_items", json!({})).unwrap(), json!([]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
