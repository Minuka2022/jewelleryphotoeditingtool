//! The workspace on disk: one folder per jewelry photo, holding the prepared
//! reference photo, the generated images and an `item.json` describing them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use serde::{Deserialize, Serialize};
use studio_core::reference::{prepare_reference, PrepareOptions};

const REFERENCE_FILE: &str = "reference.jpg";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRecord {
    pub id: String,
    pub name: String,
    pub category: String,
    pub width: u32,
    pub height: u32,
    pub created_at: String,
    #[serde(default)]
    pub outputs: Vec<OutputRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStatus {
    None,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRecord {
    pub id: String,
    /// Outputs made by one press of Generate share a run id.
    pub run_id: String,
    pub style_id: String,
    pub style_name: String,
    pub style_version: u32,
    /// File name inside the item's `outputs` folder.
    pub file: String,
    pub status: OutputStatus,
    pub created_at: String,
    pub model: String,
    pub image_size: String,
    pub cost_usd: Option<f64>,
    pub note: Option<String>,
}

pub struct Store {
    dir: PathBuf,
    items: Vec<ItemRecord>,
}

impl Store {
    /// Opens the workspace, reading every item already saved in `dir`.
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| cannot("create", &dir, e))?;
        let entries = fs::read_dir(&dir).map_err(|e| cannot("read", &dir, e))?;
        let mut items: Vec<ItemRecord> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| fs::read_to_string(entry.path().join("item.json")).ok())
            // A damaged item file hides that one item instead of blocking the app.
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect();
        items.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
        Ok(Self { dir, items })
    }

    pub fn items(&self) -> &[ItemRecord] {
        &self.items
    }

    pub fn item(&self, id: &str) -> Result<&ItemRecord, String> {
        self.index(id).map(|index| &self.items[index])
    }

    pub fn reference_path(&self, item_id: &str) -> PathBuf {
        self.dir.join(item_id).join(REFERENCE_FILE)
    }

    pub fn output_path(&self, item_id: &str, file: &str) -> PathBuf {
        self.dir.join(item_id).join("outputs").join(file)
    }

    /// Copies a photo into the workspace as a new item.
    pub fn add_item(&mut self, source: &Path, category: &str) -> Result<&ItemRecord, String> {
        let reference =
            prepare_reference(source, &PrepareOptions::default()).map_err(|e| e.to_string())?;
        let id = new_id("i");
        let item_dir = self.dir.join(&id);
        fs::create_dir_all(item_dir.join("outputs")).map_err(|e| cannot("create", &item_dir, e))?;
        let reference_path = item_dir.join(REFERENCE_FILE);
        fs::write(&reference_path, &reference.bytes)
            .map_err(|e| cannot("write", &reference_path, e))?;

        let name = source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        let item = ItemRecord {
            id,
            name,
            category: category.to_owned(),
            width: reference.width,
            height: reference.height,
            created_at: now(),
            outputs: Vec::new(),
        };
        self.save(&item)?;
        self.items.push(item);
        Ok(&self.items[self.items.len() - 1])
    }

    pub fn update_item(
        &mut self,
        id: &str,
        name: Option<String>,
        category: Option<String>,
    ) -> Result<(), String> {
        let index = self.index(id)?;
        if let Some(name) = name.map(|name| name.trim().to_owned()) {
            if !name.is_empty() {
                self.items[index].name = name;
            }
        }
        if let Some(category) = category {
            self.items[index].category = category;
        }
        self.save(&self.items[index])
    }

    /// Deletes the item, its reference photo and everything generated from it.
    pub fn remove_item(&mut self, id: &str) -> Result<(), String> {
        let index = self.index(id)?;
        let item_dir = self.dir.join(id);
        fs::remove_dir_all(&item_dir).map_err(|e| cannot("delete", &item_dir, e))?;
        self.items.remove(index);
        Ok(())
    }

    /// Saves a generated image. `output.file` must be a bare file name.
    pub fn add_output(
        &mut self,
        item_id: &str,
        output: OutputRecord,
        bytes: &[u8],
    ) -> Result<(), String> {
        let index = self.index(item_id)?;
        let path = self.output_path(item_id, &output.file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| cannot("create", parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| cannot("write", &path, e))?;
        self.items[index].outputs.push(output);
        self.save(&self.items[index])
    }

    pub fn set_output_status(
        &mut self,
        item_id: &str,
        output_id: &str,
        status: OutputStatus,
    ) -> Result<(), String> {
        let index = self.index(item_id)?;
        let output = self.items[index]
            .outputs
            .iter_mut()
            .find(|output| output.id == output_id)
            .ok_or("That image no longer exists.")?;
        output.status = status;
        self.save(&self.items[index])
    }

    pub fn remove_output(&mut self, item_id: &str, output_id: &str) -> Result<(), String> {
        let index = self.index(item_id)?;
        let outputs = &self.items[index].outputs;
        let position = outputs
            .iter()
            .position(|output| output.id == output_id)
            .ok_or("That image no longer exists.")?;
        let path = self.output_path(item_id, &outputs[position].file);
        // The record is what matters; a file that is already gone is fine.
        let _ = fs::remove_file(path);
        self.items[index].outputs.remove(position);
        self.save(&self.items[index])
    }

    fn index(&self, id: &str) -> Result<usize, String> {
        self.items
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| "That photo is no longer in the workspace.".to_owned())
    }

    fn save(&self, item: &ItemRecord) -> Result<(), String> {
        let path = self.dir.join(&item.id).join("item.json");
        let json = serde_json::to_string_pretty(item).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| cannot("write", &path, e))
    }
}

/// A short id that is unique on this machine and sorts by creation time.
pub fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed) % 0x1000;
    format!("{prefix}{millis:x}{sequence:03x}")
}

pub fn now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

fn cannot(action: &str, path: &Path, error: std::io::Error) -> String {
    format!("Cannot {action} {}: {error}", path.display())
}
