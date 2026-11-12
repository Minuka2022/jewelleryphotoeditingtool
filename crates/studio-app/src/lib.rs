//! Everything Jewelry Studio does, independent of how it is shown: the
//! workspace of photos and results, the settings, generation, cropping and
//! export. The desktop app (Tauri) and the web version are thin wrappers
//! around [`service`].

pub mod generate;
pub mod secrets;
pub mod service;
pub mod store;
pub mod studio;
