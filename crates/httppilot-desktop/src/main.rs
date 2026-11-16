use std::fmt;

use httppilot_core::{HttpEngine, HttpRequest, HttpResponse};
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_editor, text_input};
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

struct State {
    verb: Verb,
    url: String,
    body: text_editor::Content,
    response: Option<HttpResponse>,
    error: Option<String>,
    sending: bool,
    engine: HttpEngine,
}

impl Default for State {
    fn default() -> Self {
        Self {
            verb: Verb::Get,
            url: "https://httpbin.org/get".to_owned(),
            body: text_editor::Content::new(),
            response: None,
            error: None,
            sending: false,
            engine: HttpEngine::default(),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    VerbChanged(Verb),
    UrlChanged(String),
    BodyEdited(text_editor::Action),
    Send,
    Completed(Result<HttpResponse, String>),
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::VerbChanged(verb) => state.verb = verb,
        Message::UrlChanged(url) => state.url = url,
        Message::BodyEdited(action) => state.body.perform(action),
        Message::Send => {
            if state.sending {
                return Task::none();
            }

            let mut request = HttpRequest::new(state.verb.to_string(), state.url.trim());
            let body = state.body.text();
            if !body.trim().is_empty() && state.verb != Verb::Get && state.verb != Verb::Head {
                request.body = Some(body);
                request.headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
            }

            state.sending = true;
            state.response = None;
            state.error = None;

            let engine = state.engine.clone();
            return Task::perform(
                async move { engine.execute(request).await.map_err(|error| error.to_string()) },
                Message::Completed,
            );
        }
        Message::Completed(result) => {
            state.sending = false;
            match result {
                Ok(response) => state.response = Some(response),
                Err(error) => state.error = Some(error),
            }
        }
    }

    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let sidebar = column![
        text("HTTP PILOT").size(22),
        text("COLLECTIONS").size(15),
        text("Local collections coming soon").size(13),
        text("ENVIRONMENTS").size(15),
        text("Development (planned)").size(13),
    ]
    .padding(20)
    .spacing(16)
    .width(Length::Fixed(235.0));

    let mut send_button = button(if state.sending { "Sending..." } else { "Send" });
    if !state.sending {
        send_button = send_button.on_press(Message::Send);
    }

    let request_bar = row![
        pick_list(Verb::ALL, Some(state.verb), Message::VerbChanged),
        text_input("https://api.example.com/resource", &state.url)
            .on_input(Message::UrlChanged)
            .on_submit(Message::Send)
            .padding(10)
            .width(Length::Fill),
        send_button,
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    let response_summary = match &state.response {
        Some(response) => {
            let truncated = if response.truncated { " - preview truncated" } else { "" };
            format!(
                "HTTP {} | {} ms | {} preview bytes{}",
                response.status,
                response.elapsed.as_millis(),
                response.preview_bytes,
                truncated
            )
        }
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

    let editor = text_editor(&state.body)
        .placeholder("JSON or plain-text request body")
        .on_action(Message::BodyEdited)
        .height(Length::Fixed(180.0));

    let content = column![
        text("Request").size(24),
        request_bar,
        text("Body").size(16),
        editor,
        text("Response").size(24),
        text(response_summary).size(14),
        scrollable(
            container(text(response_text).font(Font::MONOSPACE).size(14))
                .padding(12)
                .width(Length::Fill)
        )
        .height(Length::Fill),
    ]
    .spacing(12)
    .padding(20)
    .width(Length::Fill)
    .height(Length::Fill);

    row![
        container(sidebar).height(Length::Fill),
        container(content).width(Length::Fill).height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
