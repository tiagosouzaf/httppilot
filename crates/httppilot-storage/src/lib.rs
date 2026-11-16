//! HttpPilot workspace models and local SQLite persistence.

use std::{fs, path::Path};

use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("cannot determine the operating system's local application data directory")]
    NoDataDirectory,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRequest {
    pub id: u64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    pub requests: Vec<SavedRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub collections: Vec<Collection>,
    pub tabs: Vec<u64>,
    pub active_request_id: Option<u64>,
    pub next_id: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            collections: vec![Collection {
                id: 1,
                name: "My Collection".to_owned(),
                requests: vec![SavedRequest {
                    id: 2,
                    name: "Example GET".to_owned(),
                    method: "GET".to_owned(),
                    url: "https://httpbin.org/get".to_owned(),
                    headers: Vec::new(),
                    body: String::new(),
                }],
            }],
            tabs: vec![2],
            active_request_id: Some(2),
            next_id: 3,
        }
    }
}

impl Workspace {
    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn new_collection(&mut self) -> u64 {
        let id = self.allocate_id();
        self.collections.push(Collection {
            id,
            name: format!("Collection {}", self.collections.len() + 1),
            requests: Vec::new(),
        });
        id
    }

    pub fn new_request(&mut self, collection_id: u64) -> Option<u64> {
        if !self.collections.iter().any(|c| c.id == collection_id) {
            return None;
        }

        let id = self.allocate_id();
        let collection = self
            .collections
            .iter_mut()
            .find(|c| c.id == collection_id)?;
        collection.requests.push(SavedRequest {
            id,
            name: format!("New Request {}", collection.requests.len() + 1),
            method: "GET".to_owned(),
            url: String::new(),
            headers: Vec::new(),
            body: String::new(),
        });
        Some(id)
    }

    pub fn request(&self, id: u64) -> Option<&SavedRequest> {
        self.collections
            .iter()
            .flat_map(|c| &c.requests)
            .find(|request| request.id == id)
    }

    pub fn request_mut(&mut self, id: u64) -> Option<&mut SavedRequest> {
        self.collections
            .iter_mut()
            .flat_map(|c| &mut c.requests)
            .find(|request| request.id == id)
    }

    pub fn open_request(&mut self, id: u64) {
        if self.request(id).is_none() {
            return;
        }
        if !self.tabs.contains(&id) {
            self.tabs.push(id);
        }
        self.active_request_id = Some(id);
    }

    pub fn close_tab(&mut self, id: u64) {
        self.tabs.retain(|item| *item != id);
        if self.active_request_id == Some(id) {
            self.active_request_id = self.tabs.last().copied();
        }
    }
}

/// SQLite-backed workspace. The entire small workspace is stored atomically
/// in one SQLite row. Future versions may normalize collections and requests.
pub struct Storage {
    db: Connection,
}

impl Storage {
    pub fn open_default() -> Result<Self, StorageError> {
        let project = ProjectDirs::from("com", "httppilot", "HttpPilot")
            .ok_or(StorageError::NoDataDirectory)?;
        let dir = project.data_local_dir();
        fs::create_dir_all(dir)?;
        Self::open(dir.join("workspace.sqlite3"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let db = Connection::open(path)?;
        let storage = Self { db };
        storage.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspace (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state TEXT NOT NULL
            );",
        )?;
        Ok(storage)
    }

    pub fn load(&self) -> Result<Workspace, StorageError> {
        let serialized: Option<String> = self
            .db
            .query_row("SELECT state FROM workspace WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;

        match serialized {
            Some(data) => Ok(serde_json::from_str(&data)?),
            None => Ok(Workspace::default()),
        }
    }

    pub fn save(&self, workspace: &Workspace) -> Result<(), StorageError> {
        let mut sanitized = workspace.clone();

        // Credentials should be stored in the OS keychain, not SQLite.
        // For now, explicitly omit common secret-bearing headers.
        for collection in &mut sanitized.collections {
            for request in &mut collection.requests {
                request
                    .headers
                    .retain(|header| !is_sensitive_header(&header.name));
            }
        }

        let state = serde_json::to_string(&sanitized)?;
        self.db.execute(
            "INSERT INTO workspace (id, state) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state",
            params![state],
        )?;
        Ok(())
    }
}

fn is_sensitive_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key"
    ) || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("api-key")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Storage {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE workspace (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state TEXT NOT NULL
            );",
        )
        .unwrap();
        Storage { db }
    }

    #[test]
    fn round_trip_collections_tabs_and_request_body() {
        let store = in_memory();
        let mut workspace = Workspace::default();
        let collection = workspace.new_collection();
        let request_id = workspace.new_request(collection).unwrap();
        workspace.open_request(request_id);
        workspace.request_mut(request_id).unwrap().body = r#"{"ok":true}"#.into();

        store.save(&workspace).unwrap();
        assert_eq!(store.load().unwrap(), workspace);
    }

    #[test]
    fn credentials_are_not_persisted_to_sqlite() {
        let store = in_memory();
        let mut workspace = Workspace::default();
        workspace.request_mut(2).unwrap().headers = vec![
            Header {
                name: "Authorization".into(),
                value: "Bearer top-secret".into(),
            },
            Header {
                name: "Accept".into(),
                value: "application/json".into(),
            },
            Header {
                name: "X-API-Key".into(),
                value: "also-secret".into(),
            },
        ];

        store.save(&workspace).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.request(2).unwrap().headers,
            vec![Header {
                name: "Accept".into(),
                value: "application/json".into(),
            }]
        );
    }

    #[test]
    fn tabs_do_not_duplicate_and_close_active_tab_selects_previous() {
        let mut workspace = Workspace::default();
        workspace.open_request(2);
        assert_eq!(workspace.tabs, vec![2]);
        workspace.close_tab(2);
        assert_eq!(workspace.active_request_id, None);
    }
}
