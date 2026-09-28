// currently 3 copies of the same item is stored in ram
// - in localstore
// - in watch history
// - in the actual state
//
// this needs to change
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    sync::{Mutex, MutexGuard, OnceLock},
};

use crate::global::{functions::paths, structs::Item};

static LOCALSTORE: OnceLock<Mutex<LocalStore>> = OnceLock::new();

pub struct LocalRecord {
    item: Item,
    is_new: bool,
}

/// cached access and write files to ~/.local/share
#[derive(Default)]
pub struct LocalStore {
    info: HashMap<String, LocalRecord>,
    downloaded_images: HashSet<String>,
}

impl LocalStore {
    fn get_store() -> MutexGuard<'static, Self> {
        LOCALSTORE
            .get()
            .expect("LocalStore not initialised; call LocalStore::init() first")
            .lock()
            .unwrap()
    }

    pub fn add_image(id: String) {
        Self::get_store().downloaded_images.insert(id);
    }

    pub fn init() {
        let _ = LOCALSTORE.set(Mutex::new(Self::default()));
    }

    pub fn rm_cache(id: &str) -> bool {
        let mut store = Self::get_store();
        let res = store.info.remove(id);
        store.downloaded_images.remove(id);
        drop(store);

        let data = paths::data_dir();
        let info_path = data.join("info");
        let thumbnail_path = data.join("thumbnails");
        let _ = fs::remove_file(info_path.join(id).with_extension("json"));
        let _ = fs::remove_file(thumbnail_path.join(id));

        res.is_some()
    }

    pub fn get_info(id: &str) -> Option<Item> {
        let store = Self::get_store();

        match store.info.get(id) {
            Some(LocalRecord { item, .. }) => Some(item.clone()),
            None => {
                let path = paths::data_dir().join(format!("info/{id}.json"));

                if path.exists() {
                    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?
                } else {
                    None
                }
            }
        }
    }

    pub fn set_info(id: String, item: Item, is_new: bool) {
        let path = paths::data_dir().join(format!("info/{id}.json"));

        {
            let mut store = Self::get_store();
            store.info.insert(
                id.clone(),
                LocalRecord {
                    item: item.clone(),
                    is_new,
                },
            );
        }

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&item) {
            let _ = crate::global::traits::atomic_write(path, json);
        }
    }

    pub fn save_only(ids: &HashSet<String>) {
        let store = Self::get_store();
        let info_path = paths::data_dir().join("info");

        for (id, LocalRecord { item, is_new }) in store.info.iter() {
            let info = info_path.join(id).with_extension("json");
            if *is_new && ids.contains(id) {
                let mut file = match OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(info)
                {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let item_string = serde_json::to_string(&item).unwrap();
                let _ = file.write_all(item_string.as_bytes());
            }
        }
    }

    pub fn list_downloaded_images() -> Vec<String> {
        Self::get_store()
            .downloaded_images
            .iter()
            .cloned()
            .collect()
    }
}
