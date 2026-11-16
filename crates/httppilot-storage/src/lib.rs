//! HttpPilot workspace models and local SQLite persistence.

use std::{fs, path::Path};

use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod history;
pub mod postman;
pub mod openapi;
pub use history::HistoryEntry;
pub use openapi::{OpenApiError, import_openapi_json};
pub use postman::{
    ImportedCollection, ImportedRequest, PostmanError, export_postman_json, import_postman_json,
};

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
    #[error("workspace changed in another instance; reload before saving")]
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryParameter {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: u64,
    pub name: String,
    pub variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRequest {
    pub id: u64,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    #[serde(default)]
    pub query: Vec<QueryParameter>,
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
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub active_environment_id: Option<u64>,
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
                    query: Vec::new(),
                    body: String::new(),
                }],
            }],
            environments: vec![Environment {
                id: 3,
                name: "Local".to_owned(),
                variables: Vec::new(),
            }],
            active_environment_id: Some(3),
            tabs: vec![2],
            active_request_id: Some(2),
            next_id: 4,
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

    /// Rename a collection without allowing blank names.
    pub fn rename_collection(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        if let Some(collection) = self.collections.iter_mut().find(|item| item.id == id) {
            collection.name = name.to_owned();
            true
        } else {
            false
        }
    }

    /// Append a parsed Postman collection, allocating fresh local IDs.
    ///
    /// Existing collections, requests and open tabs are left unchanged.
    pub fn append_imported_collection(&mut self, imported: ImportedCollection) -> u64 {
        let id = self.allocate_id();
        let requests = imported
            .requests
            .into_iter()
            .map(|request| SavedRequest {
                id: self.allocate_id(),
                name: request.name,
                method: request.method,
                url: request.url,
                headers: request.headers,
                query: request.query,
                body: request.body,
            })
            .collect();

        self.collections.push(Collection {
            id,
            name: imported.name,
            requests,
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
            query: Vec::new(),
            body: String::new(),
        });
        Some(id)
    }

    pub fn new_environment(&mut self) -> u64 {
        let id = self.allocate_id();
        self.environments.push(Environment {
            id,
            name: format!("Environment {}", self.environments.len() + 1),
            variables: Vec::new(),
        });
        self.active_environment_id = Some(id);
        id
    }

    pub fn environment_mut(&mut self, id: u64) -> Option<&mut Environment> {
        self.environments.iter_mut().find(|env| env.id == id)
    }

    pub fn selected_variables(&self) -> Vec<(String, String)> {
        self.environments
            .iter()
            .find(|env| Some(env.id) == self.active_environment_id)
            .map(|env| {
                env.variables
                    .iter()
                    .filter(|var| !var.name.trim().is_empty())
                    .map(|var| (var.name.clone(), var.value.clone()))
                    .collect()
            })
            .unwrap_or_default()
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
            );
            CREATE TABLE IF NOT EXISTS workspace_revision (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL);
            INSERT OR IGNORE INTO workspace_revision VALUES (1, 0);
            CREATE TABLE IF NOT EXISTS request_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER NOT NULL,
                method TEXT NOT NULL,
                status_code INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                occurred_at_ms INTEGER NOT NULL
            );",
        )?;
        Ok(storage)
    }

    pub fn load_versioned(&self) -> Result<(Workspace, i64), StorageError> {
        let tx = self.db.unchecked_transaction()?;
        let revision = tx.query_row(
            "SELECT revision FROM workspace_revision WHERE id = 1",
            [],
            |r| r.get(0),
        )?;
        let workspace = Self::load_from(&tx)?;
        tx.commit()?;
        Ok((workspace, revision))
    }

    pub fn load(&self) -> Result<Workspace, StorageError> {
        Self::load_from(&self.db)
    }

    fn load_from(db: &Connection) -> Result<Workspace, StorageError> {
        let serialized: Option<String> = db
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
        let (_, revision) = self.load_versioned()?;
        self.save_versioned(workspace, revision).map(|_| ())
    }

    pub fn save_versioned(
        &self,
        workspace: &Workspace,
        revision: i64,
    ) -> Result<i64, StorageError> {
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

        for environment in &mut sanitized.environments {
            for variable in &mut environment.variables {
                if variable.secret {
                    variable.value.clear();
                }
            }
        }

        let state = serde_json::to_string(&sanitized)?;
        let tx = self.db.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE workspace_revision SET revision = revision + 1 WHERE id = 1 AND revision = ?1",
            params![revision],
        )?;
        if changed != 1 {
            return Err(StorageError::Conflict);
        }
        tx.execute(
            "INSERT INTO workspace (id, state) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state",
            params![state],
        )?;
        tx.commit()?;
        Ok(revision + 1)
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
            );
            CREATE TABLE workspace_revision (id INTEGER PRIMARY KEY, revision INTEGER NOT NULL);
            INSERT INTO workspace_revision VALUES (1, 0);",
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
    fn secret_environment_values_are_never_saved() {
        let store = in_memory();
        let mut workspace = Workspace::default();
        workspace.environments[0].variables = vec![
            EnvironmentVariable {
                name: "baseUrl".into(),
                value: "https://example.com".into(),
                secret: false,
            },
            EnvironmentVariable {
                name: "token".into(),
                value: "dont-save-this".into(),
                secret: true,
            },
        ];
        store.save(&workspace).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.environments[0].variables[0].value,
            "https://example.com"
        );
        assert!(loaded.environments[0].variables[1].value.is_empty());
    }

    #[test]
    fn stale_writer_cannot_overwrite_newer_workspace() {
        let store = in_memory();
        let (mut first, revision) = store.load_versioned().unwrap();
        let (mut second, stale_revision) = store.load_versioned().unwrap();
        first.request_mut(2).unwrap().name = "first".into();
        second.request_mut(2).unwrap().name = "second".into();
        store.save_versioned(&first, revision).unwrap();
        assert!(matches!(
            store.save_versioned(&second, stale_revision),
            Err(StorageError::Conflict)
        ));
        assert_eq!(store.load().unwrap().request(2).unwrap().name, "first");
    }

    #[test]
    fn old_workspace_json_is_backward_compatible() {
        let old = r#"{ "collections": [], "tabs": [], "active_request_id": null, "next_id": 3 }"#;
        let workspace: Workspace = serde_json::from_str(old).unwrap();
        assert!(workspace.environments.is_empty());
        assert_eq!(workspace.active_environment_id, None);
    }

    #[test]
    fn imported_collections_get_unique_ids_and_preserve_existing_requests() {
        let mut workspace = Workspace::default();
        let collection_id = workspace.append_imported_collection(ImportedCollection {
            name: "External".into(),
            requests: vec![ImportedRequest {
                name: "Get status".into(),
                method: "GET".into(),
                url: "https://example.com/status".into(),
                headers: vec![],
                query: vec![],
                body: String::new(),
            }],
        });
        assert_ne!(collection_id, 1);
        assert_eq!(workspace.request(2).unwrap().name, "Example GET");
        assert_eq!(workspace.collections.len(), 2);
        assert!(workspace.collections[1].requests[0].id > collection_id);
        assert_eq!(workspace.tabs, vec![2]);
    }

    #[test]
    fn renaming_collections_rejects_empty_and_unknown_ids() {
        let mut workspace = Workspace::default();
        assert!(!workspace.rename_collection(1, "  "));
        assert!(!workspace.rename_collection(999, "No such collection"));
        assert!(workspace.rename_collection(1, "  Production  "));
        assert_eq!(workspace.collections[0].name, "Production");
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
