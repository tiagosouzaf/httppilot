//! Optional native AI panel. Only explicitly entered prompt text is sent.
//! The panel never reads current HTTP requests or responses automatically.

use std::fmt;

use httppilot_ai::{
    AiEngine, AiProvider, AiRequest,
    local::{self, CodexAuth},
};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, pick_list, row, text, text_editor, text_input},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    CodexSubscription,
    OpenAiApi,
    ClaudeApi,
}

impl Backend {
    const ALL: [Backend; 3] = [
        Backend::CodexSubscription,
        Backend::OpenAiApi,
        Backend::ClaudeApi,
    ];

    fn provider(self) -> Option<AiProvider> {
        match self {
            Self::CodexSubscription => None,
            Self::OpenAiApi => Some(AiProvider::OpenAi),
            Self::ClaudeApi => Some(AiProvider::Anthropic),
        }
    }

    fn default_model(self) -> &'static str {
        self.provider().map(AiProvider::default_model).unwrap_or("")
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CodexSubscription => "ChatGPT (assinatura via Codex CLI)",
            Self::OpenAiApi => "OpenAI (chave API)",
            Self::ClaudeApi => "Claude (chave API)",
        })
    }
}

pub enum KeyringReply {
    Stored,
    Loaded(String),
    Deleted,
}

/// Secret-bearing messages have a redacted Debug implementation to avoid
/// accidentally printing API keys or private prompts through UI diagnostics.
#[derive(Clone)]
pub enum Message {
    BackendChanged(Backend),
    ModelChanged(String),
    ApiKeyChanged(String),
    PromptEdited(text_editor::Action),
    Send,
    Completed(u64, Result<String, String>),
    StoreKey,
    LoadKey,
    DeleteKey,
    KeyringResult(Backend, u64, Result<KeyringReply, String>),
    CheckCodex,
    CodexChecked(Result<CodexAuth, String>),
    ScanMcp,
    McpScanned(Result<Vec<String>, String>),
    ToggleMcp,
    McpToggleServer(String),
    OpenClaudeCode,
    ClaudeCodeOpened(Result<(), String>),
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AssistantMessage(redacted)")
    }
}

impl Clone for KeyringReply {
    fn clone(&self) -> Self {
        match self {
            Self::Stored => Self::Stored,
            Self::Loaded(key) => Self::Loaded(key.clone()),
            Self::Deleted => Self::Deleted,
        }
    }
}

pub struct State {
    backend: Backend,
    model: String,
    api_key: String,
    prompt: text_editor::Content,
    answer: String,
    status: String,
    busy: bool,
    keyring_busy: bool,
    key_generation: u64,
    send_generation: u64,
    mcp_enabled: bool,
    mcp_servers: Vec<String>,
    mcp_selected: Vec<String>,
    mcp_status: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            model: String::new(),
            api_key: String::new(),
            prompt: text_editor::Content::new(),
            answer: String::new(),
            status: "Assinatura: execute codex login no terminal.".to_owned(),
            busy: false,
            keyring_busy: false,
            key_generation: 0,
            send_generation: 0,
            mcp_enabled: false,
            mcp_servers: Vec::new(),
            mcp_selected: Vec::new(),
            mcp_status: "MCP desativado por padrão.".to_owned(),
        }
    }
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::BackendChanged(next) => {
            state.backend = next;
            state.model = next.default_model().to_owned();
            state.api_key.clear();
            state.answer.clear();
            state.status = "Modo alterado. Informe somente o texto que deseja enviar.".to_owned();
            state.key_generation = state.key_generation.wrapping_add(1);
            state.send_generation = state.send_generation.wrapping_add(1);
        }
        Message::ModelChanged(model) => state.model = model,
        Message::ApiKeyChanged(key) => {
            state.api_key = key;
            state.key_generation = state.key_generation.wrapping_add(1);
        }
        Message::PromptEdited(action) => state.prompt.perform(action),
        Message::Send => {
            if state.busy {
                return Task::none();
            }
            let prompt = state.prompt.text();
            if prompt.trim().is_empty() {
                state.status = "Digite a pergunta antes de enviar.".into();
                return Task::none();
            }
            if prompt.len() > 32 * 1024 {
                state.status = "Pergunta muito longa (máximo 32 KiB).".into();
                return Task::none();
            }
            state.busy = true;
            state.answer.clear();
            state.status = "Consultando o provedor selecionado...".to_owned();
            state.send_generation = state.send_generation.wrapping_add(1);
            let generation = state.send_generation;
            let backend = state.backend;
            let model = state.model.clone();
            let key = state.api_key.clone();
            let selected_mcp = if state.mcp_enabled {
                state.mcp_selected.clone()
            } else {
                Vec::new()
            };
            return Task::perform(
                async move {
                    match backend {
                        Backend::CodexSubscription => {
                            local::ask_codex_subscription(&prompt, &selected_mcp)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Backend::OpenAiApi | Backend::ClaudeApi => {
                            let request = AiRequest {
                                provider: backend.provider().unwrap_or(AiProvider::OpenAi),
                                model,
                                prompt,
                                api_key: key,
                            };
                            match AiEngine::new() {
                                Ok(engine) => engine
                                    .ask(request)
                                    .await
                                    .map(|answer| answer.content)
                                    .map_err(|e| e.to_string()),
                                Err(error) => Err(error.to_string()),
                            }
                        }
                    }
                },
                move |result| Message::Completed(generation, result),
            );
        }
        Message::Completed(generation, result) => {
            if generation != state.send_generation {
                return Task::none();
            }
            state.busy = false;
            match result {
                Ok(answer) => {
                    state.answer = answer;
                    state.status = "Resposta recebida.".to_owned();
                }
                Err(error) => {
                    state.status = format!("Falha: {error}");
                }
            }
        }
        Message::StoreKey | Message::LoadKey | Message::DeleteKey => {
            let Some(provider) = state.backend.provider() else {
                return Task::none();
            };
            if state.keyring_busy {
                return Task::none();
            }
            let action = match message {
                Message::StoreKey => 0u8,
                Message::LoadKey => 1,
                _ => 2,
            };
            if action == 0 && state.api_key.trim().is_empty() {
                state.status = "Informe uma chave API antes de salvar.".to_owned();
                return Task::none();
            }

            state.keyring_busy = true;
            let backend = state.backend;
            let generation = state.key_generation;
            let key = state.api_key.clone();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || -> Result<KeyringReply, String> {
                        let entry =
                            keyring::Entry::new("HttpPilot-AI", provider.keychain_account())
                                .map_err(|e| e.to_string())?;
                        match action {
                            0 => {
                                entry.set_password(&key).map_err(|e| e.to_string())?;
                                Ok(KeyringReply::Stored)
                            }
                            1 => entry
                                .get_password()
                                .map(KeyringReply::Loaded)
                                .map_err(|e| e.to_string()),
                            _ => {
                                entry.delete_credential().map_err(|e| e.to_string())?;
                                Ok(KeyringReply::Deleted)
                            }
                        }
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                },
                move |result| Message::KeyringResult(backend, generation, result),
            );
        }
        Message::KeyringResult(backend, generation, result) => {
            state.keyring_busy = false;
            match result {
                Ok(KeyringReply::Loaded(key))
                    if state.backend == backend && state.key_generation == generation =>
                {
                    state.api_key = key;
                    state.status = "Chave carregada do cofre do sistema.".to_owned();
                }
                Ok(KeyringReply::Loaded(_)) => {
                    state.status = "Resultado ignorado: o provedor ou a chave mudou.".to_owned();
                }
                Ok(KeyringReply::Stored) => {
                    state.status = "Chave salva no cofre do sistema.".to_owned();
                }
                Ok(KeyringReply::Deleted) => {
                    if state.backend == backend && state.key_generation == generation {
                        state.api_key.clear();
                    }
                    state.status = "Chave removida do cofre do sistema.".to_owned();
                }
                Err(error) => state.status = format!("Erro no cofre do sistema: {error}"),
            }
        }
        Message::CheckCodex => {
            state.status = "Verificando autenticação do Codex instalado...".to_owned();
            return Task::perform(
                async {
                    local::codex_auth_status()
                        .await
                        .map_err(|error| error.to_string())
                },
                Message::CodexChecked,
            );
        }
        Message::CodexChecked(result) => {
            state.status = match result {
                Ok(CodexAuth::ChatGptSubscription) => {
                    "Codex autenticado com ChatGPT (assinatura detectada).".to_owned()
                }
                Ok(CodexAuth::ApiKey) => {
                    "Codex está usando chave de API (não a assinatura). Rode codex logout e codex login.".to_owned()
                }
                Ok(CodexAuth::NotSignedIn) => {
                    "Codex não autenticado. Rode codex login no terminal.".to_owned()
                }
                Ok(CodexAuth::Unknown) => {
                    "Não foi possível verificar a modalidade. Rode codex login status no terminal.".to_owned()
                }
                Err(error) => format!("Codex indisponível: {error}"),
            };
        }
        Message::ScanMcp => {
            state.mcp_status = "Consultando MCPs cadastrados no Codex local...".to_owned();
            return Task::perform(
                async {
                    local::list_codex_mcp_names()
                        .await
                        .map_err(|error| error.to_string())
                },
                Message::McpScanned,
            );
        }
        Message::McpScanned(result) => match result {
            Ok(names) => {
                state.mcp_status = if names.is_empty() {
                    "Nenhum servidor MCP configurado no Codex local.".to_owned()
                } else {
                    format!("{} MCP(s) configurado(s).", names.len())
                };
                state.mcp_selected.retain(|name| names.contains(name));
                state.mcp_servers = names;
            }
            Err(error) => state.mcp_status = format!("MCP: {error}"),
        },
        Message::ToggleMcp => {
            state.mcp_enabled = !state.mcp_enabled;
            state.mcp_status = if state.mcp_enabled {
                "MCP ativado: selecione explicitamente os servidores autorizados abaixo. Ferramentas remotas podem executar ações.".to_owned()
            } else {
                "MCP desativado; configuração de servidores não é carregada nas execuções."
                    .to_owned()
            };
        }
        Message::McpToggleServer(name) => {
            if state.mcp_servers.contains(&name) {
                if state.mcp_selected.contains(&name) {
                    state.mcp_selected.retain(|selected| selected != &name);
                } else {
                    state.mcp_selected.push(name);
                }
                state.mcp_status = format!(
                    "{} de {} MCP(s) selecionado(s) para uso quando MCP estiver ligado.",
                    state.mcp_selected.len(),
                    state.mcp_servers.len()
                );
            }
        }
        Message::OpenClaudeCode => {
            state.status = "Abrindo aplicativo Claude Code oficial...".to_owned();
            return Task::perform(
                async {
                    tokio::task::spawn_blocking(local::launch_official_claude_code)
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result.map_err(|error| error.to_string()))
                },
                Message::ClaudeCodeOpened,
            );
        }
        Message::ClaudeCodeOpened(result) => {
            state.status = match result {
                Ok(()) => "Claude Code oficial aberto em terminal separado.".to_owned(),
                Err(error) => format!("Não foi possível abrir Claude Code: {error}"),
            };
        }
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let mut layout =
        column![
        text("ASSISTENTE IA").size(20),
        text("API ou assinatura do ChatGPT via Codex CLI. Somente o texto digitado aqui e enviado.")
            .size(12),
        pick_list(Backend::ALL, Some(state.backend), Message::BackendChanged),
    ]
        .spacing(10)
        .width(Length::Fill);

    match state.backend {
        Backend::CodexSubscription => {
            layout = layout.push(
                text("Instale o Codex CLI e execute 'codex login' com ChatGPT no terminal. Sua assinatura segue os limites do plano.")
                    .size(12),
            );
            layout = layout.push(
                row![
                    button("Verificar login").on_press(Message::CheckCodex),
                    button("Listar MCPs locais").on_press(Message::ScanMcp),
                ]
                .spacing(8),
            );
            layout = layout.push(
                button(if state.mcp_enabled {
                    "MCPs locais: LIGADOS"
                } else {
                    "MCPs locais: DESLIGADOS"
                })
                .on_press(Message::ToggleMcp),
            );
            layout = layout.push(
                text("Selecione servidores individualmente. Os demais serão desativados para esta consulta. MCPs autorizados ainda podem alterar sistemas externos, mesmo em sandbox read-only.")
                    .size(12),
            );
            layout = layout.push(text(&state.mcp_status).size(12));
            for name in &state.mcp_servers {
                let selected = state.mcp_selected.contains(name);
                let label = if selected {
                    format!("[Selecionado] {name}")
                } else {
                    format!("[Não selecionado] {name}")
                };
                layout = layout
                    .push(button(text(label)).on_press(Message::McpToggleServer(name.clone())));
            }
        }
        Backend::OpenAiApi | Backend::ClaudeApi => {
            layout = layout
                .push(text_input("ID do modelo", &state.model).on_input(Message::ModelChanged));
            layout = layout.push(
                text_input("Chave de API (não salva no SQLite)", &state.api_key)
                    .secure(true)
                    .on_input(Message::ApiKeyChanged),
            );
            let mut buttons = row![].spacing(8);
            if !state.keyring_busy {
                buttons = buttons
                    .push(button("Salvar no cofre").on_press(Message::StoreKey))
                    .push(button("Carregar do cofre").on_press(Message::LoadKey))
                    .push(button("Excluir do cofre").on_press(Message::DeleteKey));
            } else {
                buttons = buttons.push(text("Acessando cofre do sistema..."));
            }
            layout = layout.push(buttons);
            layout = layout.push(
                text("A chave de API pode gerar cobranças por consumo, independentemente da assinatura mensal.")
                    .size(12),
            );
        }
    }

    layout = layout.push(
        text_editor(&state.prompt)
            .placeholder("Digite uma pergunta, por exemplo: explique este código HTTP...")
            .on_action(Message::PromptEdited)
            .height(Length::Fixed(155.0)),
    );

    if state.busy {
        layout = layout.push(text("Consultando...").size(12));
    } else {
        layout = layout.push(button("Enviar para IA").on_press(Message::Send));
    }
    layout = layout.push(text(&state.status).size(12));

    if !state.answer.is_empty() {
        layout = layout.push(text("Resposta").size(16));
        layout = layout.push(
            container(text(&state.answer).size(13))
                .padding(10)
                .width(Length::Fill),
        );
    }

    layout = layout.push(text("Claude por assinatura: abra o Claude Code oficial (sem integrar credenciais ao HttpPilot).").size(12));
    layout = layout.push(button("Abrir Claude Code oficial").on_press(Message::OpenClaudeCode));

    container(layout).padding(14).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_defaults_to_codex_subscription() {
        let state = State::default();
        assert_eq!(state.backend, Backend::CodexSubscription);
        assert!(!state.mcp_enabled);
        assert!(state.api_key.is_empty());
    }

    #[test]
    fn provider_switch_drops_key_and_resets_model() {
        let mut state = State::default();
        let _ = update(&mut state, Message::BackendChanged(Backend::OpenAiApi));
        assert_eq!(state.model, "gpt-6-luna");
        let _ = update(&mut state, Message::ApiKeyChanged("private-secret".into()));
        let _ = update(&mut state, Message::BackendChanged(Backend::ClaudeApi));
        assert!(state.api_key.is_empty());
        assert_eq!(state.model, "claude-haiku-5-5");
    }

    #[test]
    fn toggling_mcp_is_explicit_opt_in() {
        let mut state = State::default();
        assert!(!state.mcp_enabled);
        let _ = update(&mut state, Message::ToggleMcp);
        assert!(state.mcp_enabled);
    }

    #[test]
    fn only_explicit_mcp_server_selection_is_retained() {
        let mut state = State::default();
        let _ = update(
            &mut state,
            Message::McpScanned(Ok(vec!["github".into(), "database".into()])),
        );
        assert!(state.mcp_selected.is_empty());
        let _ = update(&mut state, Message::McpToggleServer("github".into()));
        assert_eq!(state.mcp_selected, vec!["github"]);
        let _ = update(&mut state, Message::McpScanned(Ok(vec!["database".into()])));
        assert!(state.mcp_selected.is_empty());
    }

    #[test]
    fn debug_messages_redact_prompts_and_api_keys() {
        let s = format!("{:?}", Message::ApiKeyChanged("secret-value".into()));
        assert!(!s.contains("secret-value"));
    }
}
