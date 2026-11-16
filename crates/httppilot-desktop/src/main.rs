use std::{fmt, io::Write};

mod assistant;
mod dialogs;

use httppilot_core::{HttpEngine, HttpRequest, HttpResponse, RequestAuth, render_template};
use httppilot_storage::{
    EnvironmentVariable, Header, HistoryEntry, ImportedCollection, QueryParameter, Storage,
    Workspace, export_postman_json, import_postman_json,
};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Font, Length, Task, Theme};

fn main() -> iced::Result {
    iced::application(State::default, update, view)
        .title("HttpPilot")
        .theme(Theme::Dark)
        .exit_on_close_request(false)
        .subscription(|_| iced::window::close_requests().map(Message::CloseRequested))
        .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Verb {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Verb {
    const ALL: [Verb; 7] = [
        Verb::Get,
        Verb::Post,
        Verb::Put,
        Verb::Patch,
        Verb::Delete,
        Verb::Head,
        Verb::Options,
    ];

    fn parse(method: &str) -> Self {
        match method.to_ascii_uppercase().as_str() {
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Get,
        }
    }
}

impl fmt::Display for Verb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
enum AuthMode {
    #[default]
    None,
    Bearer,
    Basic,
}

impl AuthMode {
    const ALL: [AuthMode; 3] = [AuthMode::None, AuthMode::Bearer, AuthMode::Basic];
}

impl fmt::Display for AuthMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "No Auth",
            Self::Bearer => "Bearer Token",
            Self::Basic => "Basic Auth",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvironmentChoice {
    id: u64,
    name: String,
}

impl fmt::Display for EnvironmentChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Credentials {
    mode: AuthMode,
    bearer: String,
    username: String,
    password: String,
}

struct State {
    assistant: assistant::State,
    workspace: Workspace,
    name: String,
    verb: Verb,
    url: String,
    headers: Vec<Header>,
    query: Vec<QueryParameter>,
    new_query_name: String,
    new_query_value: String,
    auth_mode: AuthMode,
    bearer_token: String,
    basic_username: String,
    basic_password: String,
    new_variable_name: String,
    new_variable_value: String,
    new_variable_secret: bool,
    new_header_name: String,
    new_header_value: String,
    body: text_editor::Content,
    response: Option<HttpResponse>,
    error: Option<String>,
    save_status: String,
    postman_path: String,
    postman_status: String,
    postman_busy: bool,
    history: Vec<HistoryEntry>,
    history_status: String,
    history_generation: u64,
    dirty: bool,
    saving: bool,
    revision: i64,
    edit_generation: u64,
    send_generation: u64,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    credential_generation: u64,
    credential_busy: bool,
    save_blocked: bool,
    load_failed: bool,
    closing: Option<iced::window::Id>,
    sending: bool,
    engine: HttpEngine,
}

impl Default for State {
    fn default() -> Self {
        let (workspace, revision, save_status, dirty, save_blocked) =
            match Storage::open_default().and_then(|storage| storage.load_versioned()) {
                Ok((workspace, revision)) => (
                    workspace,
                    revision,
                    "Local workspace ready".to_owned(),
                    false,
                    false,
                ),
                Err(error) => (
                    Workspace::default(),
                    0,
                    format!("Could not load local workspace: {error}"),
                    true,
                    true,
                ),
            };

        let history = Storage::open_default()
            .and_then(|storage| storage.recent_history(30))
            .unwrap_or_default();

        let mut state = Self {
            assistant: assistant::State::default(),
            workspace,
            name: String::new(),
            verb: Verb::Get,
            url: String::new(),
            headers: Vec::new(),
            query: Vec::new(),
            new_query_name: String::new(),
            new_query_value: String::new(),
            auth_mode: AuthMode::None,
            bearer_token: String::new(),
            basic_username: String::new(),
            basic_password: String::new(),
            new_variable_name: String::new(),
            new_variable_value: String::new(),
            new_variable_secret: false,
            new_header_name: String::new(),
            new_header_value: String::new(),
            body: text_editor::Content::new(),
            response: None,
            error: None,
            save_status,
            postman_path: String::new(),
            postman_status: "Choose a Postman Collection v2.1 JSON file".to_owned(),
            postman_busy: false,
            history,
            history_status: String::new(),
            history_generation: 0,
            dirty,
            saving: false,
            revision,
            edit_generation: 0,
            send_generation: 0,
            cancel: None,
            credential_generation: 0,
            credential_busy: false,
            save_blocked,
            load_failed: save_blocked,
            closing: None,
            sending: false,
            engine: HttpEngine::default(),
        };
        state.load_active();
        state
    }
}

impl State {
    fn capture_active(&mut self) {
        let Some(id) = self.workspace.active_request_id else {
            return;
        };
        if let Some(request) = self.workspace.request_mut(id) {
            request.name.clone_from(&self.name);
            request.method = self.verb.to_string();
            request.url.clone_from(&self.url);
            request.headers.clone_from(&self.headers);
            request.query.clone_from(&self.query);
            request.body = self.body.text();
        }
    }

    fn load_active(&mut self) {
        let request = self
            .workspace
            .active_request_id
            .and_then(|id| self.workspace.request(id))
            .cloned();

        if let Some(request) = request {
            self.name = request.name;
            self.verb = Verb::parse(&request.method);
            self.url = request.url;
            self.headers = request.headers;
            self.query = request.query;
            self.body = text_editor::Content::with_text(&request.body);
        } else {
            self.name.clear();
            self.verb = Verb::Get;
            self.url.clear();
            self.headers.clear();
            self.query.clear();
            self.body = text_editor::Content::new();
        }

        self.new_header_name.clear();
        self.new_header_value.clear();
        self.new_query_name.clear();
        self.new_query_value.clear();
        self.credential_generation = self.credential_generation.wrapping_add(1);
        self.auth_mode = AuthMode::None;
        self.bearer_token.clear();
        self.basic_username.clear();
        self.basic_password.clear();
        self.response = None;
        self.error = None;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.edit_generation = self.edit_generation.wrapping_add(1);
        self.save_status = "Unsaved changes".to_owned();
    }
}

#[derive(Debug, Clone)]
enum Message {
    Assistant(assistant::Message),
    NewCollection,
    PostmanPathChanged(String),
    PickPostmanFile(bool),
    PostmanFilePicked(Result<Option<String>, String>),
    ImportPostman,
    PostmanImported(Result<ImportedCollection, String>),
    ExportPostman(u64),
    PostmanExported(Result<String, String>),
    HistoryRecorded(u64, Result<Vec<HistoryEntry>, String>),
    ClearHistory,
    HistoryCleared(Result<(), String>),
    RenameCollection(u64, String),
    NewRequest(u64),
    OpenRequest(u64),
    CloseTab(u64),
    NameChanged(String),
    VerbChanged(Verb),
    UrlChanged(String),
    BodyEdited(text_editor::Action),
    HeaderNameChanged(usize, String),
    HeaderValueChanged(usize, String),
    NewHeaderName(String),
    NewHeaderValue(String),
    AddHeader,
    RemoveHeader(usize),
    QueryNameChanged(usize, String),
    QueryValueChanged(usize, String),
    NewQueryName(String),
    NewQueryValue(String),
    AddQuery,
    RemoveQuery(usize),
    NewEnvironment,
    SelectEnvironment(u64),
    EnvironmentRenamed(String),
    EnvVariableNameChanged(usize, String),
    EnvVariableValueChanged(usize, String),
    EnvVariableSecret(usize, bool),
    RemoveEnvVariable(usize),
    NewVariableName(String),
    NewVariableValue(String),
    ToggleNewVariableSecret,
    AddEnvVariable,
    AuthChanged(AuthMode),
    BearerChanged(String),
    BasicUsernameChanged(String),
    BasicPasswordChanged(String),
    Send,
    CloseRequested(iced::window::Id),
    DiscardAndClose,
    RetrySave,
    StoreCredentials,
    LoadCredentials,
    DeleteCredentials,
    CredentialResult(u64, u64, Result<Option<String>, String>),
    Cancel,
    Completed(u64, u64, Result<HttpResponse, String>),
    Autosave(u64),
    Save,
    Saved(u64, Result<i64, String>),
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    let generation_before = state.edit_generation;
    match message {
        Message::Assistant(action) => {
            return assistant::update(&mut state.assistant, action).map(Message::Assistant);
        }
        Message::NewCollection => {
            state.capture_active();
            let collection_id = state.workspace.new_collection();
            if let Some(id) = state.workspace.new_request(collection_id) {
                state.workspace.open_request(id);
            }
            state.load_active();
            state.mark_dirty();
        }
        Message::PostmanPathChanged(path) => {
            state.postman_path = path;
        }
        Message::PickPostmanFile(save) => {
            if state.postman_busy {
                return Task::none();
            }
            state.postman_busy = true;
            state.postman_status = "Opening native file chooser...".to_owned();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || dialogs::select_json_file(save))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result)
                },
                Message::PostmanFilePicked,
            );
        }
        Message::PostmanFilePicked(result) => {
            state.postman_busy = false;
            match result {
                Ok(Some(path)) => {
                    state.postman_path = path;
                    state.postman_status = "File selected. Choose Import or Export.".to_owned();
                }
                Ok(None) => state.postman_status = "File selection cancelled".to_owned(),
                Err(error) => {
                    state.postman_status =
                        format!("File chooser unavailable ({error}); enter a path manually");
                }
            }
        }
        Message::ImportPostman => {
            if state.postman_busy {
                return Task::none();
            }
            let path = state.postman_path.trim().to_owned();
            if path.is_empty() {
                state.postman_status = "Choose a JSON file or enter its path".to_owned();
                return Task::none();
            }
            state.postman_busy = true;
            state.postman_status = "Importing Postman collection...".to_owned();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || -> Result<ImportedCollection, String> {
                        let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                        if metadata.len() > 10 * 1024 * 1024 {
                            return Err("Postman import exceeds the 10 MiB limit".to_owned());
                        }
                        let json = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                        import_postman_json(&json).map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                Message::PostmanImported,
            );
        }
        Message::PostmanImported(result) => {
            state.postman_busy = false;
            match result {
                Ok(collection) => {
                    let count = collection.requests.len();
                    state.capture_active();
                    let id = state.workspace.append_imported_collection(collection);
                    if let Some(request_id) = state
                        .workspace
                        .collections
                        .iter()
                        .find(|collection| collection.id == id)
                        .and_then(|collection| collection.requests.first())
                        .map(|request| request.id)
                    {
                        state.workspace.open_request(request_id);
                        state.load_active();
                    }
                    state.mark_dirty();
                    state.postman_status =
                        format!("Imported {count} requests. Review URLs and bodies for secrets.");
                }
                Err(error) => state.postman_status = format!("Import failed: {error}"),
            }
        }
        Message::ExportPostman(collection_id) => {
            if state.postman_busy {
                return Task::none();
            }
            let Some(collection) = state
                .workspace
                .collections
                .iter()
                .find(|collection| collection.id == collection_id)
                .cloned()
            else {
                state.postman_status = "Collection no longer exists".to_owned();
                return Task::none();
            };
            let path = state.postman_path.trim().to_owned();
            if path.is_empty() {
                state.postman_status = "Choose a destination or enter a new file path".to_owned();
                return Task::none();
            }

            // Export the live workspace snapshot, including unsaved editor changes.
            state.capture_active();
            let collection = state
                .workspace
                .collections
                .iter()
                .find(|item| item.id == collection_id)
                .cloned()
                .unwrap_or(collection);

            state.postman_busy = true;
            state.postman_status = "Exporting collection...".to_owned();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || -> Result<String, String> {
                        let json = export_postman_json(&collection).map_err(|e| e.to_string())?;
                        let mut options = std::fs::OpenOptions::new();
                        options.write(true).create_new(true);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::OpenOptionsExt;
                            options.mode(0o600);
                        }
                        let mut output = options.open(&path).map_err(|e| e.to_string())?;
                        output
                            .write_all(json.as_bytes())
                            .map_err(|e| e.to_string())?;
                        Ok(path)
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                Message::PostmanExported,
            );
        }
        Message::PostmanExported(result) => {
            state.postman_busy = false;
            state.postman_status = match result {
                Ok(path) => {
                    format!("Exported to {path}. Review the JSON for sensitive URLs and bodies.")
                }
                Err(error) => format!("Export failed: {error}"),
            };
        }
        Message::HistoryRecorded(generation, result) => {
            if generation == state.history_generation {
                match result {
                    Ok(entries) => state.history = entries,
                    Err(error) => state.history_status = format!("History error: {error}"),
                }
            }
        }
        Message::ClearHistory => {
            state.history_generation = state.history_generation.wrapping_add(1);
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        Storage::open_default()
                            .and_then(|storage| storage.clear_history())
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                Message::HistoryCleared,
            );
        }
        Message::HistoryCleared(result) => match result {
            Ok(()) => {
                state.history.clear();
                state.history_status = "History cleared".to_owned();
            }
            Err(error) => state.history_status = format!("Could not clear history: {error}"),
        },
        Message::RenameCollection(id, name) => {
            if state.workspace.rename_collection(id, &name) {
                state.mark_dirty();
            }
        }
        Message::NewRequest(collection_id) => {
            state.capture_active();
            if let Some(id) = state.workspace.new_request(collection_id) {
                state.workspace.open_request(id);
                state.load_active();
                state.mark_dirty();
            }
        }
        Message::OpenRequest(id) => {
            state.capture_active();
            state.workspace.open_request(id);
            state.load_active();
            state.mark_dirty();
        }
        Message::CloseTab(id) => {
            state.capture_active();
            state.workspace.close_tab(id);
            state.load_active();
            state.mark_dirty();
        }
        Message::NameChanged(name) => {
            state.name = name;
            state.mark_dirty();
        }
        Message::VerbChanged(verb) => {
            state.verb = verb;
            state.mark_dirty();
        }
        Message::UrlChanged(url) => {
            state.url = url;
            state.mark_dirty();
        }
        Message::BodyEdited(action) => {
            state.body.perform(action);
            state.mark_dirty();
        }
        Message::HeaderNameChanged(index, name) => {
            if let Some(header) = state.headers.get_mut(index) {
                header.name = name;
                state.mark_dirty();
            }
        }
        Message::HeaderValueChanged(index, value) => {
            if let Some(header) = state.headers.get_mut(index) {
                header.value = value;
                state.mark_dirty();
            }
        }
        Message::NewHeaderName(name) => state.new_header_name = name,
        Message::NewHeaderValue(value) => state.new_header_value = value,
        Message::AddHeader => {
            let name = state.new_header_name.trim().to_owned();
            if !name.is_empty() {
                state.headers.push(Header {
                    name,
                    value: std::mem::take(&mut state.new_header_value),
                });
                state.new_header_name.clear();
                state.mark_dirty();
            }
        }
        Message::RemoveHeader(index) => {
            if index < state.headers.len() {
                state.headers.remove(index);
                state.mark_dirty();
            }
        }
        Message::QueryNameChanged(index, name) => {
            if let Some(param) = state.query.get_mut(index) {
                param.name = name;
                state.mark_dirty();
            }
        }
        Message::QueryValueChanged(index, value) => {
            if let Some(param) = state.query.get_mut(index) {
                param.value = value;
                state.mark_dirty();
            }
        }
        Message::NewQueryName(value) => state.new_query_name = value,
        Message::NewQueryValue(value) => state.new_query_value = value,
        Message::AddQuery => {
            let name = state.new_query_name.trim().to_owned();
            if !name.is_empty() {
                state.query.push(QueryParameter {
                    name,
                    value: std::mem::take(&mut state.new_query_value),
                });
                state.new_query_name.clear();
                state.mark_dirty();
            }
        }
        Message::RemoveQuery(index) => {
            if index < state.query.len() {
                state.query.remove(index);
                state.mark_dirty();
            }
        }
        Message::NewEnvironment => {
            state.workspace.new_environment();
            state.mark_dirty();
        }
        Message::SelectEnvironment(id) => {
            if state.workspace.environments.iter().any(|env| env.id == id) {
                state.workspace.active_environment_id = Some(id);
                state.mark_dirty();
            }
        }
        Message::EnvironmentRenamed(name) => {
            if let Some(id) = state.workspace.active_environment_id
                && let Some(environment) = state.workspace.environment_mut(id)
            {
                environment.name = name;
                state.mark_dirty();
            }
        }
        Message::EnvVariableNameChanged(index, name) => {
            if let Some(id) = state.workspace.active_environment_id
                && let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
            {
                variable.name = name;
                state.mark_dirty();
            }
        }
        Message::EnvVariableValueChanged(index, value) => {
            if let Some(id) = state.workspace.active_environment_id
                && let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
            {
                variable.value = value;
                state.mark_dirty();
            }
        }
        Message::EnvVariableSecret(index, secret) => {
            if let Some(id) = state.workspace.active_environment_id
                && let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
            {
                variable.secret = secret;
                state.mark_dirty();
            }
        }
        Message::RemoveEnvVariable(index) => {
            if let Some(id) = state.workspace.active_environment_id
                && let Some(environment) = state.workspace.environment_mut(id)
                && index < environment.variables.len()
            {
                environment.variables.remove(index);
                state.mark_dirty();
            }
        }
        Message::NewVariableName(value) => state.new_variable_name = value,
        Message::NewVariableValue(value) => state.new_variable_value = value,
        Message::ToggleNewVariableSecret => state.new_variable_secret = !state.new_variable_secret,
        Message::AddEnvVariable => {
            let name = state.new_variable_name.trim().to_owned();
            if !name.is_empty()
                && let Some(id) = state.workspace.active_environment_id
                && let Some(environment) = state.workspace.environment_mut(id)
            {
                environment.variables.push(EnvironmentVariable {
                    name,
                    value: std::mem::take(&mut state.new_variable_value),
                    secret: state.new_variable_secret,
                });
                state.new_variable_name.clear();
                state.new_variable_secret = false;
                state.mark_dirty();
            }
        }
        Message::AuthChanged(mode) => {
            state.auth_mode = mode;
            state.credential_generation = state.credential_generation.wrapping_add(1);
        }
        Message::BearerChanged(value) => {
            state.bearer_token = value;
            state.credential_generation = state.credential_generation.wrapping_add(1);
        }
        Message::BasicUsernameChanged(value) => {
            state.basic_username = value;
            state.credential_generation = state.credential_generation.wrapping_add(1);
        }
        Message::BasicPasswordChanged(value) => {
            state.basic_password = value;
            state.credential_generation = state.credential_generation.wrapping_add(1);
        }
        Message::CloseRequested(id) => {
            if !state.dirty && !state.saving {
                return iced::window::close(id);
            }
            state.closing = Some(id);
            if !state.saving && !state.save_blocked {
                return update(state, Message::Save);
            }
        }
        Message::DiscardAndClose => {
            if let Some(id) = state.closing.take() {
                return iced::window::close(id);
            }
        }
        Message::RetrySave => {
            if state.load_failed {
                return Task::none();
            }
            state.save_blocked = false;
            return update(state, Message::Save);
        }
        Message::StoreCredentials | Message::LoadCredentials | Message::DeleteCredentials => {
            if state.credential_busy {
                return Task::none();
            }
            let Some(id) = state.workspace.active_request_id else {
                return Task::none();
            };
            state.credential_busy = true;
            let generation = state.credential_generation;
            let action = match message {
                Message::StoreCredentials => 0,
                Message::LoadCredentials => 1,
                _ => 2,
            };
            let data = Credentials {
                mode: state.auth_mode,
                bearer: state.bearer_token.clone(),
                username: state.basic_username.clone(),
                password: state.basic_password.clone(),
            };
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
                        let entry = keyring::Entry::new("HttpPilot", &format!("request-{id}"))
                            .map_err(|e| e.to_string())?;
                        match action {
                            0 => {
                                let value =
                                    serde_json::to_string(&data).map_err(|e| e.to_string())?;
                                entry.set_password(&value).map_err(|e| e.to_string())?;
                                Ok(None)
                            }
                            1 => entry.get_password().map(Some).map_err(|e| e.to_string()),
                            _ => {
                                entry.delete_credential().map_err(|e| e.to_string())?;
                                Ok(None)
                            }
                        }
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r)
                },
                move |result| Message::CredentialResult(id, generation, result),
            );
        }
        Message::CredentialResult(id, generation, result) => {
            state.credential_busy = false;
            match result {
                Ok(Some(data))
                    if state.workspace.active_request_id == Some(id)
                        && state.credential_generation == generation =>
                {
                    match serde_json::from_str::<Credentials>(&data) {
                        Ok(credentials) => {
                            state.auth_mode = credentials.mode;
                            state.bearer_token = credentials.bearer;
                            state.basic_username = credentials.username;
                            state.basic_password = credentials.password;
                            state.save_status = "Credentials loaded from system keychain".into();
                        }
                        Err(error) => {
                            state.save_status = format!("Could not read credentials: {error}")
                        }
                    }
                }
                Ok(_) => state.save_status = "Keychain operation completed".into(),
                Err(error) => state.save_status = format!("Keychain operation failed: {error}"),
            }
        }
        Message::Send => {
            if state.sending || state.workspace.active_request_id.is_none() {
                return Task::none();
            }

            state.capture_active();
            let vars = state.workspace.selected_variables();
            let request = (|| -> Result<HttpRequest, String> {
                let url = render_template(state.url.trim(), &vars).map_err(|e| e.to_string())?;
                let mut request = HttpRequest::new(state.verb.to_string(), url);
                request.headers = state
                    .headers
                    .iter()
                    .filter(|h| !h.name.trim().is_empty())
                    .map(|h| {
                        Ok((
                            render_template(h.name.trim(), &vars).map_err(|e| e.to_string())?,
                            render_template(&h.value, &vars).map_err(|e| e.to_string())?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                request.query = state
                    .query
                    .iter()
                    .filter(|param| !param.name.trim().is_empty())
                    .map(|param| {
                        Ok((
                            render_template(param.name.trim(), &vars).map_err(|e| e.to_string())?,
                            render_template(&param.value, &vars).map_err(|e| e.to_string())?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;

                let body = state.body.text();
                if !body.is_empty() && state.verb != Verb::Get && state.verb != Verb::Head {
                    request.body = Some(render_template(&body, &vars).map_err(|e| e.to_string())?);
                }

                request.auth = match state.auth_mode {
                    AuthMode::None => RequestAuth::None,
                    AuthMode::Bearer => RequestAuth::Bearer(
                        render_template(&state.bearer_token, &vars).map_err(|e| e.to_string())?,
                    ),
                    AuthMode::Basic => RequestAuth::Basic {
                        username: render_template(&state.basic_username, &vars)
                            .map_err(|e| e.to_string())?,
                        password: render_template(&state.basic_password, &vars)
                            .map_err(|e| e.to_string())?,
                    },
                };
                Ok(request)
            })();

            let request = match request {
                Ok(request) => request,
                Err(error) => {
                    state.error = Some(error);
                    state.response = None;
                    return Task::none();
                }
            };

            let id = state.workspace.active_request_id.unwrap_or_default();
            state.sending = true;
            state.response = None;
            state.error = None;

            let engine = state.engine.clone();
            state.send_generation = state.send_generation.wrapping_add(1);
            let generation = state.send_generation;
            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
            state.cancel = Some(cancel_tx);
            return Task::perform(
                async move { execute_cancellable(engine, request, cancel_rx).await },
                move |result| Message::Completed(id, generation, result),
            );
        }
        Message::Cancel => {
            if let Some(cancel) = state.cancel.take() {
                let _ = cancel.send(());
            }
            state.sending = false;
            state.send_generation = state.send_generation.wrapping_add(1);
            state.error = Some("Request cancelled".to_owned());
        }
        Message::Completed(id, generation, result) => {
            if generation != state.send_generation {
                return Task::none();
            }
            state.cancel = None;
            state.sending = false;
            // A response must never appear in a different request's tab.
            if state.workspace.active_request_id != Some(id) {
                return Task::none();
            }
            match result {
                Ok(response) => {
                    let status_code = response.status;
                    let duration_ms = response.elapsed.as_millis().min(u64::MAX as u128) as u64;
                    let method = state
                        .workspace
                        .request(id)
                        .map(|request| request.method.clone())
                        .unwrap_or_else(|| state.verb.to_string());
                    state.response = Some(response);
                    state.history_generation = state.history_generation.wrapping_add(1);
                    let history_generation = state.history_generation;
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(
                                move || -> Result<Vec<HistoryEntry>, String> {
                                    let storage =
                                        Storage::open_default().map_err(|e| e.to_string())?;
                                    storage
                                        .record_history(id, &method, status_code, duration_ms)
                                        .map_err(|e| e.to_string())?;
                                    storage.recent_history(30).map_err(|e| e.to_string())
                                },
                            )
                            .await
                            .map_err(|error| error.to_string())
                            .and_then(|result| result)
                        },
                        move |result| Message::HistoryRecorded(history_generation, result),
                    );
                }
                Err(error) => state.error = Some(error),
            }
        }
        Message::Autosave(generation) => {
            if generation == state.edit_generation
                && state.dirty
                && !state.saving
                && !state.save_blocked
            {
                return update(state, Message::Save);
            }
        }
        Message::Save => {
            if state.saving || state.save_blocked {
                return Task::none();
            }

            state.capture_active();
            let workspace = state.workspace.clone();
            state.saving = true;
            let generation = state.edit_generation;
            let revision = state.revision;
            state.save_status = "Saving locally...".to_owned();

            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        Storage::open_default()
                            .and_then(|storage| storage.save_versioned(&workspace, revision))
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                move |result| Message::Saved(generation, result),
            );
        }
        Message::Saved(generation, result) => {
            state.saving = false;
            match result {
                Ok(revision) => {
                    state.revision = revision;
                    state.dirty = generation != state.edit_generation;
                    state.save_status = if state.dirty {
                        "Saved snapshot; newer changes are unsaved".to_owned()
                    } else {
                        "Saved to local SQLite".to_owned()
                    };
                    if state.dirty {
                        return update(state, Message::Save);
                    }
                    if let Some(id) = state.closing.take() {
                        return iced::window::close(id);
                    }
                }
                Err(error) => {
                    state.dirty = true;
                    state.save_blocked = true;
                    state.save_status = format!("Could not save: {error}");
                }
            }
        }
    }
    if state.edit_generation != generation_before && !state.save_blocked {
        let generation = state.edit_generation;
        return Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                generation
            },
            Message::Autosave,
        );
    }
    Task::none()
}

async fn execute_cancellable(
    engine: HttpEngine,
    request: HttpRequest,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<HttpResponse, String> {
    tokio::select! {
        result = engine.execute(request) => result.map_err(|error| error.to_string()),
        _ = cancel => Err("Request cancelled".to_owned()),
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let mut sidebar = column![
        text("HTTP PILOT").size(22),
        text("COLLECTIONS").size(14),
        button("+ New collection").on_press(Message::NewCollection),
    ]
    .spacing(12)
    .width(Length::Fixed(220.0));

    for collection in &state.workspace.collections {
        let collection_id = collection.id;
        sidebar = sidebar.push(
            row![
                text_input("Collection name", &collection.name)
                    .on_input(move |name| Message::RenameCollection(collection_id, name))
                    .width(Length::Fill),
                button("+").on_press(Message::NewRequest(collection.id)),
                button("Export").on_press(Message::ExportPostman(collection.id)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );

        for request in &collection.requests {
            sidebar = sidebar.push(
                button(text(format!("{}  {}", request.method, request.name)).size(13))
                    .on_press(Message::OpenRequest(request.id))
                    .width(Length::Fill),
            );
        }
    }

    sidebar = sidebar.push(text("POSTMAN v2.1").size(14));
    sidebar = sidebar.push(
        text_input("Path to collection.json", &state.postman_path)
            .on_input(Message::PostmanPathChanged),
    );
    let mut file_buttons = row![
        button("Open file").on_press(Message::PickPostmanFile(false)),
        button("Save as").on_press(Message::PickPostmanFile(true)),
    ]
    .spacing(4);
    if state.postman_busy {
        file_buttons = row![text("Working...")].spacing(4);
    }
    sidebar = sidebar.push(file_buttons);
    if !state.postman_busy {
        sidebar = sidebar.push(button("Import collection").on_press(Message::ImportPostman));
    }
    sidebar = sidebar.push(text(&state.postman_status).size(11));
    sidebar = sidebar.push(text("History (metadata only)").size(14));
    sidebar = sidebar.push(button("Clear history").on_press(Message::ClearHistory));
    for entry in &state.history {
        if let Some(request) = state.workspace.request(entry.request_id) {
            sidebar = sidebar.push(
                button(
                    text(format!(
                        "{} {} · {}ms · {}",
                        entry.method, entry.status_code, entry.duration_ms, request.name,
                    ))
                    .size(12),
                )
                .on_press(Message::OpenRequest(entry.request_id))
                .width(Length::Fill),
            );
        }
    }
    if !state.history_status.is_empty() {
        sidebar = sidebar.push(text(&state.history_status).size(11));
    }

    sidebar = sidebar.push(text("ENVIRONMENTS").size(14));
    sidebar = sidebar.push(button("+ New environment").on_press(Message::NewEnvironment));

    let choices: Vec<EnvironmentChoice> = state
        .workspace
        .environments
        .iter()
        .map(|env| EnvironmentChoice {
            id: env.id,
            name: env.name.clone(),
        })
        .collect();
    let selected = choices
        .iter()
        .find(|choice| Some(choice.id) == state.workspace.active_environment_id)
        .cloned();
    sidebar = sidebar.push(pick_list(choices, selected, |choice| {
        Message::SelectEnvironment(choice.id)
    }));

    if let Some(environment) = state
        .workspace
        .environments
        .iter()
        .find(|env| Some(env.id) == state.workspace.active_environment_id)
    {
        sidebar = sidebar.push(
            text_input("Environment name", &environment.name).on_input(Message::EnvironmentRenamed),
        );

        for (index, var) in environment.variables.iter().enumerate() {
            sidebar = sidebar.push(
                column![
                    row![
                        text_input("Variable", &var.name)
                            .on_input(move |value| Message::EnvVariableNameChanged(index, value))
                            .width(Length::Fill),
                        button("x").on_press(Message::RemoveEnvVariable(index)),
                    ]
                    .spacing(4),
                    row![
                        text_input("Value", &var.value)
                            .secure(var.secret)
                            .on_input(move |value| Message::EnvVariableValueChanged(index, value))
                            .width(Length::Fill),
                        button(if var.secret { "Secret" } else { "Plain" })
                            .on_press(Message::EnvVariableSecret(index, !var.secret)),
                    ]
                    .spacing(4),
                ]
                .spacing(4),
            );
        }

        sidebar = sidebar.push(
            column![
                text_input("New variable name", &state.new_variable_name)
                    .on_input(Message::NewVariableName),
                text_input("New variable value", &state.new_variable_value)
                    .secure(state.new_variable_secret)
                    .on_input(Message::NewVariableValue),
                row![
                    button(if state.new_variable_secret {
                        "Secret"
                    } else {
                        "Plain"
                    })
                    .on_press(Message::ToggleNewVariableSecret),
                    button("+ Add variable").on_press(Message::AddEnvVariable),
                ]
                .spacing(6),
            ]
            .spacing(5),
        );
    }

    sidebar =
        sidebar.push(text("Secret values are session-only; plain values are saved.").size(11));

    let sidebar: Element<'_, Message> = container(scrollable(sidebar.padding(14)))
        .width(Length::Fixed(240.0))
        .height(Length::Fill)
        .into();

    let mut tabs = row![text("TABS").size(14)]
        .spacing(8)
        .align_y(Alignment::Center);
    for id in &state.workspace.tabs {
        if let Some(request) = state.workspace.request(*id) {
            tabs = tabs.push(
                row![
                    button(request.name.as_str()).on_press(Message::OpenRequest(*id)),
                    button("x").on_press(Message::CloseTab(*id)),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            );
        }
    }

    let main_content: Element<'_, Message> = if state.workspace.active_request_id.is_some() {
        let mut send_button = button(if state.sending { "Sending..." } else { "Send" });
        send_button = if state.sending {
            button("Cancel").on_press(Message::Cancel)
        } else {
            send_button.on_press(Message::Send)
        };

        let mut save_button = button(if state.saving { "Saving..." } else { "Save" });
        if !state.saving {
            save_button = save_button.on_press(Message::Save);
        }

        let request_bar = row![
            pick_list(Verb::ALL, Some(state.verb), Message::VerbChanged),
            text_input("https://api.example.com/resource", &state.url)
                .on_input(Message::UrlChanged)
                .on_submit(Message::Send)
                .padding(10)
                .width(Length::Fill),
            send_button,
            save_button,
        ]
        .align_y(Alignment::Center)
        .spacing(8);

        let mut params = column![text("QUERY PARAMETERS").size(16)].spacing(8);
        for (index, param) in state.query.iter().enumerate() {
            params = params.push(
                row![
                    text_input("Key", &param.name)
                        .on_input(move |value| Message::QueryNameChanged(index, value))
                        .width(Length::FillPortion(2)),
                    text_input("Value", &param.value)
                        .on_input(move |value| Message::QueryValueChanged(index, value))
                        .width(Length::FillPortion(3)),
                    button("Remove").on_press(Message::RemoveQuery(index)),
                ]
                .spacing(8),
            );
        }
        params = params.push(
            row![
                text_input("New parameter", &state.new_query_name)
                    .on_input(Message::NewQueryName)
                    .width(Length::FillPortion(2)),
                text_input("Value", &state.new_query_value)
                    .on_input(Message::NewQueryValue)
                    .width(Length::FillPortion(3)),
                button("Add").on_press(Message::AddQuery),
            ]
            .spacing(8),
        );

        let mut authentication = column![
            text("AUTHENTICATION").size(16),
            pick_list(AuthMode::ALL, Some(state.auth_mode), Message::AuthChanged),
        ]
        .spacing(8);
        authentication = match state.auth_mode {
            AuthMode::None => authentication,
            AuthMode::Bearer => authentication.push(
                text_input("Bearer token", &state.bearer_token)
                    .secure(true)
                    .on_input(Message::BearerChanged),
            ),
            AuthMode::Basic => authentication
                .push(
                    text_input("Username", &state.basic_username)
                        .on_input(Message::BasicUsernameChanged),
                )
                .push(
                    text_input("Password", &state.basic_password)
                        .secure(true)
                        .on_input(Message::BasicPasswordChanged),
                ),
        };

        authentication = authentication.push(
            row![
                button("Save credentials to keychain").on_press(Message::StoreCredentials),
                button("Load credentials").on_press(Message::LoadCredentials),
                button("Delete credentials").on_press(Message::DeleteCredentials)
            ]
            .spacing(6),
        );

        let mut headers =
            column![text("HEADERS").size(16), text("Name / Value").size(12),].spacing(8);

        for (index, header) in state.headers.iter().enumerate() {
            headers = headers.push(
                row![
                    text_input("Header name", &header.name)
                        .on_input(move |value| Message::HeaderNameChanged(index, value))
                        .width(Length::FillPortion(2)),
                    text_input("Value", &header.value)
                        .on_input(move |value| Message::HeaderValueChanged(index, value))
                        .width(Length::FillPortion(3)),
                    button("Remove").on_press(Message::RemoveHeader(index)),
                ]
                .spacing(8),
            );
        }

        headers = headers.push(
            row![
                text_input("New header", &state.new_header_name)
                    .on_input(Message::NewHeaderName)
                    .width(Length::FillPortion(2)),
                text_input("Value", &state.new_header_value)
                    .on_input(Message::NewHeaderValue)
                    .width(Length::FillPortion(3)),
                button("Add").on_press(Message::AddHeader),
            ]
            .spacing(8),
        );

        let response_summary = match &state.response {
            Some(response) => format!(
                "HTTP {} | {} ms | {} preview bytes{}",
                response.status,
                response.elapsed.as_millis(),
                response.preview_bytes,
                if response.truncated {
                    " (truncated)"
                } else {
                    ""
                }
            ),
            None if state.sending => "Sending request...".to_owned(),
            None if state.error.is_some() => "Request failed".to_owned(),
            None => "Ready to send a request".to_owned(),
        };

        let response_text = state
            .response
            .as_ref()
            .map(|response| response.body_preview.as_str())
            .or(state.error.as_deref())
            .unwrap_or("Response body will appear here.");

        let body_editor = text_editor(&state.body)
            .placeholder("Request body (JSON or plain text)")
            .on_action(Message::BodyEdited)
            .height(Length::Fixed(150.0));

        let main = column![
            tabs,
            text_input("Request name", &state.name)
                .on_input(Message::NameChanged)
                .padding(10),
            request_bar,
            params,
            headers,
            authentication,
            text("BODY").size(16),
            body_editor,
            text("RESPONSE").size(16),
            text(response_summary).size(14),
            container(text(response_text).font(Font::MONOSPACE).size(13))
                .padding(12)
                .width(Length::Fill),
            text(&state.save_status).size(12),
            assistant::view(&state.assistant).map(Message::Assistant),
        ]
        .spacing(12)
        .padding(16)
        .width(Length::Fill);

        container(scrollable(main))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        container(
            column![
                tabs,
                text("No request is open").size(20),
                text("Choose a request from the collections on the left."),
                button("Save workspace").on_press(Message::Save),
                text(&state.save_status).size(12),
                assistant::view(&state.assistant).map(Message::Assistant),
            ]
            .spacing(14)
            .padding(16),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    let mut layout = column![row![sidebar, main_content].height(Length::Fill)];
    if state.save_blocked {
        let mut controls = row![text(&state.save_status)].spacing(8);
        if !state.load_failed {
            controls = controls.push(button("Retry saving").on_press(Message::RetrySave));
        }
        if state.closing.is_some() {
            controls = controls
                .push(button("Discard changes and close").on_press(Message::DiscardAndClose));
        }
        layout = layout.push(controls);
    }
    layout.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_method_round_trips_without_becoming_get() {
        assert!(matches!(Verb::parse("options"), Verb::Options));
        assert_eq!(Verb::Options.to_string(), "OPTIONS");
    }

    #[tokio::test]
    async fn cancellation_stops_an_in_flight_request() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            let _ = accepted_tx.send(());
            std::thread::sleep(std::time::Duration::from_millis(500));
        });
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(execute_cancellable(
            HttpEngine::default(),
            HttpRequest::new("GET", format!("http://{address}")),
            cancel_rx,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), accepted_rx)
            .await
            .unwrap()
            .unwrap();
        cancel_tx.send(()).unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err(), "Request cancelled");
        server.join().unwrap();
    }

    #[test]
    fn cancellation_ignores_late_completion() {
        let mut state = State {
            sending: true,
            send_generation: 10,
            ..State::default()
        };
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        state.cancel = Some(sender);
        let _ = update(&mut state, Message::Cancel);
        assert!(!state.sending);
        assert!(receiver.try_recv().is_ok());
        state.sending = true;
        let _ = update(
            &mut state,
            Message::Completed(2, 10, Err("old result".into())),
        );
        assert!(state.sending);
        assert_ne!(state.error.as_deref(), Some("old result"));
    }

    #[test]
    fn edits_during_save_remain_dirty() {
        let mut state = State {
            saving: true,
            save_blocked: false,
            edit_generation: 2,
            ..State::default()
        };
        let _ = update(&mut state, Message::Saved(1, Ok(3)));
        assert!(state.dirty);
        assert!(state.saving);
        assert_eq!(state.revision, 3);
    }
}
