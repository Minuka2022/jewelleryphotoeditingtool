// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop app: a window around the shared logic in `studio_app`.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use studio_app::secrets;
use studio_app::service::{self, lock, settings_info, ImportedStyle, SettingsInfo, Shared};
use studio_app::store::OutputStatus;
use studio_app::studio::{
    AddItemsResult, ExportEstimate, GenerateArgs, ImportStyle, ItemInfo, LibraryInfo, OutputRef,
    PromptCheck, Settings, Studio,
};
use studio_core::export::{Crop, ExportOptions};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[tauri::command]
fn get_library(state: State<'_, Shared>) -> Result<LibraryInfo, String> {
    Ok(lock(&state)?.library_info())
}

#[tauri::command]
fn get_settings(state: State<'_, Shared>) -> Result<SettingsInfo, String> {
    Ok(settings_info(&*lock(&state)?))
}

#[tauri::command]
fn save_settings(state: State<'_, Shared>, settings: Settings) -> Result<SettingsInfo, String> {
    let mut studio = lock(&state)?;
    studio.save_settings(settings)?;
    Ok(settings_info(&studio))
}

#[tauri::command]
fn set_api_key(state: State<'_, Shared>, key: String) -> Result<SettingsInfo, String> {
    service::set_api_key(&state, &key)
}

#[tauri::command]
async fn check_api_key(state: State<'_, Shared>) -> Result<String, String> {
    service::check_api_key(&state).await
}

#[tauri::command]
fn list_items(state: State<'_, Shared>) -> Result<Vec<ItemInfo>, String> {
    Ok(lock(&state)?.items_info())
}

#[tauri::command]
fn add_items(
    state: State<'_, Shared>,
    paths: Vec<String>,
    category: String,
) -> Result<AddItemsResult, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    lock(&state)?.add_items(&paths, &category)
}

#[tauri::command]
fn update_item(
    state: State<'_, Shared>,
    id: String,
    name: Option<String>,
    category: Option<String>,
) -> Result<ItemInfo, String> {
    lock(&state)?.update_item(&id, name, category)
}

#[tauri::command]
fn remove_item(state: State<'_, Shared>, id: String) -> Result<(), String> {
    lock(&state)?.remove_item(&id)
}

#[tauri::command]
fn set_output_status(
    state: State<'_, Shared>,
    item_id: String,
    output_id: String,
    status: OutputStatus,
) -> Result<ItemInfo, String> {
    lock(&state)?.set_output_status(&item_id, &output_id, status)
}

#[tauri::command]
fn remove_output(
    state: State<'_, Shared>,
    item_id: String,
    output_id: String,
) -> Result<ItemInfo, String> {
    lock(&state)?.remove_output(&item_id, &output_id)
}

#[tauri::command]
async fn set_output_crop(
    state: State<'_, Shared>,
    item_id: String,
    output_id: String,
    crop: Option<Crop>,
) -> Result<ItemInfo, String> {
    service::set_output_crop(&state, &item_id, &output_id, crop).await
}

#[tauri::command]
async fn estimate_export(
    state: State<'_, Shared>,
    selection: OutputRef,
    options: ExportOptions,
) -> Result<ExportEstimate, String> {
    service::estimate_export(&state, selection, options).await
}

#[tauri::command]
async fn export_images(
    state: State<'_, Shared>,
    selections: Vec<OutputRef>,
    options: ExportOptions,
    destination: String,
) -> Result<usize, String> {
    service::export_to_folder(&state, selections, options, PathBuf::from(destination)).await
}

#[tauri::command]
fn check_style_prompt(state: State<'_, Shared>, prompt: String) -> Result<PromptCheck, String> {
    Ok(lock(&state)?.check_style_prompt(&prompt))
}

#[tauri::command]
fn import_style(state: State<'_, Shared>, style: ImportStyle) -> Result<ImportedStyle, String> {
    service::import_style(&state, style)
}

#[tauri::command]
fn update_style(
    state: State<'_, Shared>,
    id: String,
    style: ImportStyle,
) -> Result<ImportedStyle, String> {
    service::update_style(&state, &id, style)
}

#[tauri::command]
fn remove_style(state: State<'_, Shared>, id: String) -> Result<LibraryInfo, String> {
    service::remove_style(&state, &id)
}

/// Progress arrives in the window as `generation-progress` events.
#[tauri::command]
async fn generate(
    app: AppHandle,
    state: State<'_, Shared>,
    args: GenerateArgs,
) -> Result<ItemInfo, String> {
    service::generate(&state, args, |progress| {
        // A closed window simply stops listening.
        let _ = app.emit("generation-progress", progress);
    })
    .await
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
            set_output_crop,
            estimate_export,
            export_images,
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
