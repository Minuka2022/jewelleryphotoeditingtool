// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod generate;
mod secrets;
mod store;
mod studio;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use studio_core::gemini::GeminiClient;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

use crate::store::OutputStatus;
use crate::studio::{
    AddItemsResult, GenerateArgs, ImportStyle, ItemInfo, LibraryInfo, OutputRef, PromptCheck,
    Settings, Studio,
};

type AppState = Mutex<Studio>;

const NO_KEY: &str = "Add your Gemini API key first: File > Settings.";
const BILLING_HELP: &str = "Google refused the request because this API key's project is on \
    the free tier, which does not include image models. Open https://ai.dev/rate-limit with the \
    Google account that owns the key, set up billing, then try again. Nothing was charged.";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsInfo {
    #[serde(flatten)]
    settings: Settings,
    has_api_key: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportedStyle {
    style_id: String,
    library: LibraryInfo,
}

fn lock(state: &AppState) -> Result<MutexGuard<'_, Studio>, String> {
    state
        .lock()
        .map_err(|_| "The app's data is unavailable. Please restart Jewelry Studio.".to_owned())
}

fn settings_info(studio: &Studio) -> SettingsInfo {
    SettingsInfo {
        settings: studio.settings().clone(),
        has_api_key: secrets::load().is_some(),
    }
}

/// `GEMINI_BASE_URL` redirects requests to a local test server or a proxy.
fn new_client(api_key: String) -> Result<GeminiClient, String> {
    let client = GeminiClient::new(api_key).map_err(|e| e.to_string())?;
    Ok(match std::env::var("GEMINI_BASE_URL") {
        Ok(base_url) if !base_url.trim().is_empty() => client.with_base_url(base_url.trim()),
        _ => client,
    })
}

/// Google words a missing billing setup as a rate limit of zero.
fn explain(error: String) -> String {
    if error.contains("Free Tier") {
        BILLING_HELP.to_owned()
    } else {
        error
    }
}

#[tauri::command]
fn get_library(state: State<'_, AppState>) -> Result<LibraryInfo, String> {
    Ok(lock(&state)?.library_info())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SettingsInfo, String> {
    Ok(settings_info(&*lock(&state)?))
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<SettingsInfo, String> {
    let mut studio = lock(&state)?;
    studio.save_settings(settings)?;
    Ok(settings_info(&studio))
}

#[tauri::command]
fn set_api_key(state: State<'_, AppState>, key: String) -> Result<SettingsInfo, String> {
    secrets::save(&key)?;
    Ok(settings_info(&*lock(&state)?))
}

/// Asks Google which models the saved key can use. Free of charge.
#[tauri::command]
async fn check_api_key(state: State<'_, AppState>) -> Result<String, String> {
    let api_key = secrets::load().ok_or(NO_KEY)?;
    let model = lock(&state)?.settings().model.clone();
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

#[tauri::command]
fn list_items(state: State<'_, AppState>) -> Result<Vec<ItemInfo>, String> {
    Ok(lock(&state)?.items_info())
}

#[tauri::command]
fn add_items(
    state: State<'_, AppState>,
    paths: Vec<String>,
    category: String,
) -> Result<AddItemsResult, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    lock(&state)?.add_items(&paths, &category)
}

#[tauri::command]
fn update_item(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    category: Option<String>,
) -> Result<ItemInfo, String> {
    lock(&state)?.update_item(&id, name, category)
}

#[tauri::command]
fn remove_item(state: State<'_, AppState>, id: String) -> Result<(), String> {
    lock(&state)?.remove_item(&id)
}

#[tauri::command]
fn set_output_status(
    state: State<'_, AppState>,
    item_id: String,
    output_id: String,
    status: OutputStatus,
) -> Result<ItemInfo, String> {
    lock(&state)?.set_output_status(&item_id, &output_id, status)
}

#[tauri::command]
fn remove_output(
    state: State<'_, AppState>,
    item_id: String,
    output_id: String,
) -> Result<ItemInfo, String> {
    lock(&state)?.remove_output(&item_id, &output_id)
}

#[tauri::command]
fn export_outputs(
    state: State<'_, AppState>,
    selections: Vec<OutputRef>,
    destination: String,
) -> Result<usize, String> {
    lock(&state)?.export(&selections, &PathBuf::from(destination))
}

#[tauri::command]
fn check_style_prompt(state: State<'_, AppState>, prompt: String) -> Result<PromptCheck, String> {
    Ok(lock(&state)?.check_style_prompt(&prompt))
}

#[tauri::command]
fn import_style(state: State<'_, AppState>, style: ImportStyle) -> Result<ImportedStyle, String> {
    let mut studio = lock(&state)?;
    let imported = studio.import_style(style)?;
    Ok(ImportedStyle {
        style_id: imported.id,
        library: studio.library_info(),
    })
}

#[tauri::command]
fn update_style(
    state: State<'_, AppState>,
    id: String,
    style: ImportStyle,
) -> Result<ImportedStyle, String> {
    let mut studio = lock(&state)?;
    let updated = studio.update_style(&id, style)?;
    Ok(ImportedStyle {
        style_id: updated.id,
        library: studio.library_info(),
    })
}

#[tauri::command]
fn remove_style(state: State<'_, AppState>, id: String) -> Result<LibraryInfo, String> {
    let mut studio = lock(&state)?;
    studio.remove_style(&id)?;
    Ok(studio.library_info())
}

/// Generates every style and option for one photo. Progress arrives in the
/// window as `generation-progress` events; the finished photo is returned.
#[tauri::command]
async fn generate(
    app: AppHandle,
    state: State<'_, AppState>,
    args: GenerateArgs,
) -> Result<ItemInfo, String> {
    let api_key = secrets::load().ok_or(NO_KEY)?;
    let plan = lock(&state)?.plan_generation(&args)?;
    let item_id = plan.item_id.clone();
    let client = Arc::new(new_client(api_key)?);

    let summary = generate::run(client, state.inner(), plan, |progress| {
        // A closed window simply stops listening.
        let _ = app.emit("generation-progress", progress);
    })
    .await;

    if summary.saved == 0 {
        let error = summary
            .first_error
            .unwrap_or_else(|| "No image could be generated.".to_owned());
        return Err(explain(error));
    }
    lock(&state)?.item_info(&item_id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    current_version: String,
    notes: Option<String>,
}

fn update_error(error: tauri_plugin_updater::Error) -> String {
    format!("Could not check for updates: {error}")
}

/// Looks for a newer version on the releases page. `None` means up to date.
#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?;
    Ok(update.map(|update| UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
    }))
}

/// Downloads the newer version, checks its signature, installs it and
/// restarts the app.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?
        .ok_or("Jewelry Studio is already up to date.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| format!("The update could not be installed: {error}"))?;
    app.restart()
}

/// `--store-api-key` saves the key from the `GEMINI_API_KEY` environment
/// variable into the Windows Credential Manager and exits, so a key can be
/// set up without opening the window.
fn store_api_key_and_exit() -> ! {
    let result = std::env::var("GEMINI_API_KEY")
        .map_err(|_| "GEMINI_API_KEY is not set".to_owned())
        .and_then(|key| secrets::save(&key));
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}

fn main() {
    if std::env::args().any(|argument| argument == "--store-api-key") {
        store_api_key_and_exit();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // JEWELRY_STUDIO_DATA_DIR keeps the data somewhere else, which is
            // how tests avoid touching the real workspace.
            let data_dir = match std::env::var_os("JEWELRY_STUDIO_DATA_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => app.path().app_local_data_dir()?,
            };
            let studio = Studio::open(data_dir)?;
            // Lets the window display photos straight from the data folder.
            app.asset_protocol_scope()
                .allow_directory(studio.data_dir(), true)?;
            app.manage(Mutex::new(studio));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_library,
            get_settings,
            save_settings,
            set_api_key,
            check_api_key,
            list_items,
            add_items,
            update_item,
            remove_item,
            set_output_status,
            remove_output,
            export_outputs,
            check_style_prompt,
            import_style,
            update_style,
            remove_style,
            generate,
            check_for_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("Jewelry Studio could not start");
}
