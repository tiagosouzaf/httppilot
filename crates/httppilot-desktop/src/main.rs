use std::fmt;

use httppilot_core::{HttpEngine, HttpRequest, HttpResponse, RequestAuth, render_template};
use httppilot_storage::{EnvironmentVariable, Header, QueryParameter, Storage, Workspace};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Font, Length, Task, Theme};

fn main() -> iced::Result {
    iced::application(State::default, update, view)
        .title("HttpPilot")
        .theme(Theme::Dark)
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
}

impl Verb {
    const ALL: [Verb; 6] = [
        Verb::Get,
        Verb::Post,
        Verb::Put,
        Verb::Patch,
        Verb::Delete,
        Verb::Head,
    ];

    fn parse(method: &str) -> Self {
        match method.to_ascii_uppercase().as_str() {
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
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
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

struct State {
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
    new_environment_name: String,
    new_variable_name: String,
    new_variable_value: String,
    new_variable_secret: bool,
    new_header_name: String,
    new_header_value: String,
    body: text_editor::Content,
    response: Option<HttpResponse>,
    error: Option<String>,
    save_status: String,
    dirty: bool,
    saving: bool,
    sending: bool,
    engine: HttpEngine,
}

impl Default for State {
    fn default() -> Self {
        let (workspace, save_status, dirty) =
            match Storage::open_default().and_then(|storage| storage.load()) {
                Ok(workspace) => (workspace, "Local workspace ready".to_owned(), false),
                Err(error) => (
                    Workspace::default(),
                    format!("Could not load local workspace: {error}"),
                    true,
                ),
            };

        let mut state = Self {
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
            new_environment_name: String::new(),
            new_variable_name: String::new(),
            new_variable_value: String::new(),
            new_variable_secret: false,
            new_header_name: String::new(),
            new_header_value: String::new(),
            body: text_editor::Content::new(),
            response: None,
            error: None,
            save_status,
            dirty,
            saving: false,
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
        self.auth_mode = AuthMode::None;
        self.bearer_token.clear();
        self.basic_username.clear();
        self.basic_password.clear();
        self.response = None;
        self.error = None;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.save_status = "Unsaved changes".to_owned();
    }
}

#[derive(Debug, Clone)]
enum Message {
    NewCollection,
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
    Completed(u64, Result<HttpResponse, String>),
    Save,
    Saved(Result<(), String>),
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::NewCollection => {
            state.capture_active();
            let collection_id = state.workspace.new_collection();
            if let Some(id) = state.workspace.new_request(collection_id) {
                state.workspace.open_request(id);
            }
            state.load_active();
            state.mark_dirty();
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
            if let Some(id) = state.workspace.active_environment_id {
                if let Some(environment) = state.workspace.environment_mut(id) {
                    environment.name = name;
                    state.mark_dirty();
                }
            }
        }
        Message::EnvVariableNameChanged(index, name) => {
            if let Some(id) = state.workspace.active_environment_id {
                if let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
                {
                    variable.name = name;
                    state.mark_dirty();
                }
            }
        }
        Message::EnvVariableValueChanged(index, value) => {
            if let Some(id) = state.workspace.active_environment_id {
                if let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
                {
                    variable.value = value;
                    state.mark_dirty();
                }
            }
        }
        Message::EnvVariableSecret(index, secret) => {
            if let Some(id) = state.workspace.active_environment_id {
                if let Some(variable) = state
                    .workspace
                    .environment_mut(id)
                    .and_then(|env| env.variables.get_mut(index))
                {
                    variable.secret = secret;
                    state.mark_dirty();
                }
            }
        }
        Message::RemoveEnvVariable(index) => {
            if let Some(id) = state.workspace.active_environment_id {
                if let Some(environment) = state.workspace.environment_mut(id) {
                    if index < environment.variables.len() {
                        environment.variables.remove(index);
                        state.mark_dirty();
                    }
                }
            }
        }
        Message::NewVariableName(value) => state.new_variable_name = value,
        Message::NewVariableValue(value) => state.new_variable_value = value,
        Message::ToggleNewVariableSecret => state.new_variable_secret = !state.new_variable_secret,
        Message::AddEnvVariable => {
            let name = state.new_variable_name.trim().to_owned();
            if !name.is_empty() {
                if let Some(id) = state.workspace.active_environment_id {
                    if let Some(environment) = state.workspace.environment_mut(id) {
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
            }
        }
        Message::AuthChanged(mode) => state.auth_mode = mode,
        Message::BearerChanged(value) => state.bearer_token = value,
        Message::BasicUsernameChanged(value) => state.basic_username = value,
        Message::BasicPasswordChanged(value) => state.basic_password = value,
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
            return Task::perform(
                async move {
                    engine
                        .execute(request)
                        .await
                        .map_err(|error| error.to_string())
                },
                move |result| Message::Completed(id, result),
            );
        }
        Message::Completed(id, result) => {
            state.sending = false;
            // A response must never appear in a different request's tab.
            if state.workspace.active_request_id != Some(id) {
                return Task::none();
            }
            match result {
                Ok(response) => state.response = Some(response),
                Err(error) => state.error = Some(error),
            }
        }
        Message::Save => {
            if state.saving {
                return Task::none();
            }

            state.capture_active();
            let workspace = state.workspace.clone();
            state.saving = true;
            state.dirty = false;
            state.save_status = "Saving locally...".to_owned();

            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        Storage::open_default()
                            .and_then(|storage| storage.save(&workspace))
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                Message::Saved,
            );
        }
        Message::Saved(result) => {
            state.saving = false;
            match result {
                Ok(()) => {
                    state.save_status = if state.dirty {
                        "Saved snapshot; newer changes are unsaved".to_owned()
                    } else {
                        "Saved to local SQLite".to_owned()
                    };
                }
                Err(error) => {
                    state.dirty = true;
                    state.save_status = format!("Could not save: {error}");
                }
            }
        }
    }
    Task::none()
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
        sidebar = sidebar.push(
            row![
                text(&collection.name).size(15).width(Length::Fill),
                button("+").on_press(Message::NewRequest(collection.id)),
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
        if !state.sending {
            send_button = send_button.on_press(Message::Send);
        }

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
            text("AUTHENTICATION - SESSION ONLY").size(16),
            pick_list(AuthMode::ALL, Some(state.auth_mode), Message::AuthChanged),
        ]
        .spacing(8);
        authentication = match state.auth_mode {
            AuthMode::None => authentication,
            AuthMode::Bearer => authentication.push(
                text_input("Bearer token (not saved)", &state.bearer_token)
                    .secure(true)
                    .on_input(Message::BearerChanged),
            ),
            AuthMode::Basic => authentication
                .push(
                    text_input("Username", &state.basic_username)
                        .on_input(Message::BasicUsernameChanged),
                )
                .push(
                    text_input("Password (not saved)", &state.basic_password)
                        .secure(true)
                        .on_input(Message::BasicPasswordChanged),
                ),
        };

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
            ]
            .spacing(14)
            .padding(16),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    row![sidebar, main_content].height(Length::Fill).into()
}
