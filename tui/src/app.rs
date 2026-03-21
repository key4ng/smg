use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use openai_protocol::worker::{ProviderType, RuntimeType, WorkerSpec};
use tokio::sync::mpsc;

use crate::{
    client::SmgClient,
    event::{AppEvent, EventHandler},
    playground::{ChatEndpoint, ChatMessage},
    state::SharedState,
    types::{AddMenuState, ActionMenuItem, InputMode, ProviderPreset, View},
    ui,
};

/// Top-level application state.
pub struct App {
    pub view: View,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub active_filter: Option<String>,
    pub should_quit: bool,
    pub selected_index: usize,
    pub state: SharedState,
    pub client: SmgClient,
    pub status_message: Option<String>,
    pub show_help: bool,
    /// (worker_id, worker_url) pending confirmation
    pub confirm_delete: Option<(String, String)>,
    pub show_detail: bool,
    pub show_action_menu: bool,
    pub action_menu_index: usize,
    pub add_menu_state: Option<AddMenuState>,
    pub confirm_flush: Option<(String, String)>,

    // Playground state
    pub chat_messages: Vec<ChatMessage>,
    pub chat_input: String,
    pub chat_model: String,
    pub chat_streaming: bool,
    pub chat_scroll: u16,
    pub chat_endpoint: ChatEndpoint,
    chat_stream_rx: Option<mpsc::UnboundedReceiver<String>>,

    status_clear_at: Option<std::time::Instant>,
}

impl App {
    pub fn new(state: SharedState, client: SmgClient) -> Self {
        Self {
            view: View::Pulse,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            active_filter: None,
            should_quit: false,
            selected_index: 0,
            state,
            client,
            status_message: None,
            show_help: false,
            confirm_delete: None,
            show_detail: false,
            show_action_menu: false,
            action_menu_index: 0,
            add_menu_state: None,
            confirm_flush: None,
            chat_messages: Vec::new(),
            chat_input: String::new(),
            chat_model: "gpt-4o-mini".to_string(),
            chat_streaming: false,
            chat_scroll: 0,
            chat_endpoint: ChatEndpoint::default(),
            chat_stream_rx: None,
            status_clear_at: None,
        }
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let mut events = EventHandler::new(250);

        loop {
            terminal.draw(|f| ui::render(f, self))?;

            match events.next().await {
                Some(AppEvent::Key(key)) => self.handle_key(key).await,
                Some(AppEvent::Tick) => self.on_tick(),
                Some(AppEvent::Resize(_, _)) => {} // ratatui handles resize on next draw
                None => break,
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn on_tick(&mut self) {
        // Auto-clear status message after 5 seconds
        if let Some(deadline) = self.status_clear_at {
            if std::time::Instant::now() >= deadline {
                self.status_message = None;
                self.status_clear_at = None;
            }
        }

        // Drain streaming tokens from playground
        if let Some(ref mut rx) = self.chat_stream_rx {
            let mut got_data = false;
            loop {
                match rx.try_recv() {
                    Ok(token) => {
                        got_data = true;
                        if token == "\n[DONE]" {
                            self.chat_streaming = false;
                            self.chat_stream_rx = None;
                            break;
                        } else if token.starts_with("\n[ERROR]") {
                            let err = token.trim_start_matches("\n[ERROR]").to_string();
                            if let Some(msg) = self.chat_messages.last_mut() {
                                msg.content.push_str(&format!("\n[Error: {}]", err));
                            }
                            self.chat_streaming = false;
                            self.chat_stream_rx = None;
                            break;
                        } else if let Some(msg) = self.chat_messages.last_mut() {
                            msg.content.push_str(&token);
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.chat_streaming = false;
                        self.chat_stream_rx = None;
                        break;
                    }
                }
            }
            if got_data {
                // Auto-scroll to bottom
                self.chat_scroll = u16::MAX;
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Delete confirmation dialog takes priority
        if self.confirm_delete.is_some() {
            self.handle_delete_confirm(key).await;
            return;
        }

        if self.show_action_menu {
            self.handle_action_menu_key(key).await;
            return;
        }
        if self.add_menu_state.is_some() {
            self.handle_add_menu_key(key).await;
            return;
        }
        if let Some((ref id, ref _url)) = self.confirm_flush.clone() {
            match key.code {
                KeyCode::Char('y') => {
                    let id = id.clone();
                    self.confirm_flush = None;
                    match self.client.flush_worker_cache(&id).await {
                        Ok(_) => self.set_status("Cache flushed".to_string()),
                        Err(e) => self.set_status(format!("Error: {e}")),
                    }
                }
                _ => { self.confirm_flush = None; }
            }
            return;
        }

        // Playground has its own input handling
        if self.view == View::Chat && self.input_mode == InputMode::Normal {
            self.handle_chat_key(key).await;
            return;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal(key).await,
            InputMode::Filter => self.handle_filter_input(key),
            InputMode::Command => self.handle_command_input(key).await,
        }
    }

    async fn handle_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // View switching
            code @ (KeyCode::Char('1')
            | KeyCode::Char('2')
            | KeyCode::Char('3')
            | KeyCode::Char('4')
            | KeyCode::Char('5')
            | KeyCode::Char('6')) => {
                if let Some(v) = View::from_key(code) {
                    self.view = v;
                    self.selected_index = 0;
                }
            }

            // Navigation
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected_index = self.selected_index.saturating_add(1);
                self.clamp_selection();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }

            // Input modes
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
                self.input_buffer.clear();
            }
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.input_buffer.clear();
            }

            // Help
            KeyCode::Char('?') => self.show_help = !self.show_help,

            KeyCode::Enter => {
                if self.view == View::Workers {
                    self.show_detail = !self.show_detail;
                }
            }

            // Esc clears overlays/filters
            KeyCode::Esc => {
                if self.show_detail {
                    self.show_detail = false;
                } else if self.show_help {
                    self.show_help = false;
                } else {
                    self.active_filter = None;
                }
            }

            // Workers-only keys
            KeyCode::Char('d') if self.view == View::Workers => {
                self.start_delete();
            }
            KeyCode::Char('e') => {
                if self.view == View::Workers {
                    self.show_action_menu = true;
                    self.action_menu_index = 0;
                }
            }
            KeyCode::Char('a') => {
                if self.view == View::Workers {
                    self.add_menu_state = Some(AddMenuState::SelectProvider);
                }
            }

            _ => {}
        }
    }

    fn handle_filter_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.active_filter = if self.input_buffer.is_empty() {
                    None
                } else {
                    Some(self.input_buffer.clone())
                };
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    async fn handle_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let cmd = self.input_buffer.clone();
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.execute_command(&cmd).await;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    async fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
        match parts.first().copied() {
            Some("quit" | "q") => self.should_quit = true,
            Some("add") => self.cmd_add(parts.get(1).copied()).await,
            Some("delete") => {
                if let Some(id) = parts.get(1) {
                    match self.client.delete_worker(id.trim()).await {
                        Ok(_) => self.set_status(format!("Worker {} deleted", id.trim())),
                        Err(e) => self.set_status(format!("Error: {e}")),
                    }
                } else {
                    self.set_status("Usage: delete <id>".into());
                }
            }
            Some("priority") => {
                let args = parts.get(1).copied();
                if let Some(val) = args.and_then(|a| a.parse::<u32>().ok()) {
                    if let Some(id) = self.selected_worker_id() {
                        let update = openai_protocol::worker::WorkerUpdateRequest {
                            priority: Some(val),
                            cost: None, labels: None, api_key: None, health: None,
                        };
                        match self.client.update_worker(&id, &update).await {
                            Ok(_) => self.set_status(format!("Priority set to {val}")),
                            Err(e) => self.set_status(format!("Error: {e}")),
                        }
                    } else {
                        self.set_status("No worker selected".to_string());
                    }
                } else {
                    self.set_status("Usage: :priority <number>".to_string());
                }
            }
            Some("cost") => {
                let args = parts.get(1).copied();
                if let Some(val) = args.and_then(|a| a.parse::<f32>().ok()) {
                    if let Some(id) = self.selected_worker_id() {
                        let update = openai_protocol::worker::WorkerUpdateRequest {
                            cost: Some(val),
                            priority: None, labels: None, api_key: None, health: None,
                        };
                        match self.client.update_worker(&id, &update).await {
                            Ok(_) => self.set_status(format!("Cost set to {val}")),
                            Err(e) => self.set_status(format!("Error: {e}")),
                        }
                    } else {
                        self.set_status("No worker selected".to_string());
                    }
                } else {
                    self.set_status("Usage: :cost <number>".to_string());
                }
            }
            Some("flush-cache") => {
                if let Some(id) = self.selected_worker_id() {
                    let url = self.selected_worker_url().unwrap_or_default();
                    self.confirm_flush = Some((id, url));
                } else {
                    self.set_status("No worker selected".to_string());
                }
            }
            Some("toggle-health") => {
                if let Some(id) = self.selected_worker_id() {
                    let update = openai_protocol::worker::WorkerUpdateRequest {
                        health: Some(openai_protocol::worker::HealthCheckUpdate {
                            disable_health_check: Some(true),
                            timeout_secs: None, check_interval_secs: None,
                            success_threshold: None, failure_threshold: None,
                        }),
                        priority: None, cost: None, labels: None, api_key: None,
                    };
                    match self.client.update_worker(&id, &update).await {
                        Ok(_) => self.set_status("Health check toggled".to_string()),
                        Err(e) => self.set_status(format!("Error: {e}")),
                    }
                } else {
                    self.set_status("No worker selected".to_string());
                }
            }
            Some("add-openai") => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::OpenAI, input: String::new(),
                });
            }
            Some("add-anthropic") => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Anthropic, input: String::new(),
                });
            }
            Some("add-xai") => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Xai, input: String::new(),
                });
            }
            Some("add-gemini") => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Gemini, input: String::new(),
                });
            }
            _ => self.set_status(format!("Unknown command: {cmd}")),
        }
    }

    /// Parse `:add <url> [--provider <p>] [--runtime <r>]`
    async fn cmd_add(&mut self, args: Option<&str>) {
        let Some(args) = args else {
            self.set_status(
                "Usage: add <url> [--provider openai|anthropic|gemini|xai] [--runtime external|sglang|vllm|trtllm]".into(),
            );
            return;
        };

        let tokens: Vec<&str> = args.split_whitespace().collect();
        if tokens.is_empty() {
            self.set_status("Usage: add <url> [--provider <p>] [--runtime <r>]".into());
            return;
        }

        let url = tokens[0].to_string();
        let mut spec = WorkerSpec::new(url);

        let mut i = 1;
        while i < tokens.len() {
            match tokens[i] {
                "--provider" | "-p" => {
                    i += 1;
                    if i < tokens.len() {
                        spec.provider = parse_provider(tokens[i]);
                        // Auto-set runtime to external when provider is specified
                        if spec.provider.is_some() {
                            spec.runtime_type = RuntimeType::External;
                        }
                    }
                }
                "--runtime" | "-r" => {
                    i += 1;
                    if i < tokens.len() {
                        spec.runtime_type = tokens[i].parse().unwrap_or_default();
                    }
                }
                _ => {} // skip unknown flags
            }
            i += 1;
        }

        match self.client.add_worker(&spec).await {
            Ok(_) => self.set_status("Worker added".into()),
            Err(e) => self.set_status(format!("Error: {e}")),
        }
    }

    async fn handle_chat_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if !self.chat_streaming => {
                self.should_quit = true;
            }
            // Tab to cycle model, Backtab (Shift+Tab) to cycle endpoint
            KeyCode::Tab if !self.chat_streaming => {
                self.cycle_chat_model();
            }
            KeyCode::BackTab if !self.chat_streaming => {
                self.chat_endpoint = self.chat_endpoint.cycle();
                self.set_status(format!("Endpoint: /v1/{}", self.chat_endpoint.label()));
            }
            // Number keys for view switching (only when not typing)
            code @ (KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('4') | KeyCode::Char('5'))
                if self.chat_input.is_empty() && !self.chat_streaming =>
            {
                if let Some(v) = View::from_key(code) {
                    self.view = v;
                    self.selected_index = 0;
                }
            }
            KeyCode::Char('?') if self.chat_input.is_empty() && !self.chat_streaming => {
                self.show_help = !self.show_help;
            }
            // Enter sends the message
            KeyCode::Enter => {
                if self.chat_streaming {
                    return; // Don't send while streaming
                }
                let text = self.chat_input.trim().to_string();
                if text.is_empty() {
                    return;
                }
                self.chat_input.clear();
                self.chat_messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: text,
                });
                // Add empty assistant message that will be filled by streaming
                self.chat_messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: String::new(),
                });
                self.chat_streaming = true;
                self.chat_scroll = u16::MAX;

                // Build messages for API
                let api_messages: Vec<serde_json::Value> = self
                    .chat_messages
                    .iter()
                    .filter(|m| !m.content.is_empty())
                    .map(|m| {
                        serde_json::json!({
                            "role": m.role,
                            "content": m.content,
                        })
                    })
                    .collect();

                let (tx, rx) = mpsc::unbounded_channel();
                self.chat_stream_rx = Some(rx);

                let client = self.client.clone();
                let model = self.chat_model.clone();
                let endpoint = self.chat_endpoint;
                tokio::spawn(async move {
                    crate::playground::stream_chat(&client, &model, &api_messages, endpoint, tx).await;
                });
            }
            // Esc cancels input or stops streaming
            KeyCode::Esc => {
                if self.chat_streaming {
                    self.chat_streaming = false;
                    self.chat_stream_rx = None;
                    if let Some(msg) = self.chat_messages.last_mut() {
                        if msg.content.is_empty() {
                            self.chat_messages.pop();
                        }
                    }
                } else if !self.chat_input.is_empty() {
                    self.chat_input.clear();
                }
            }
            KeyCode::Backspace => {
                self.chat_input.pop();
            }
            KeyCode::Char(c) => {
                self.chat_input.push(c);
            }
            KeyCode::Up => {
                self.chat_scroll = self.chat_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                self.chat_scroll = self.chat_scroll.saturating_add(1);
            }
            _ => {}
        }
    }

    fn cycle_chat_model(&mut self) {
        let state = self.state.read().unwrap();

        // Try /v1/models first, fall back to worker model lists
        let mut models: Vec<String> = state
            .models
            .as_ref()
            .map(|m| m.data.iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default();

        if models.is_empty() {
            // Collect models from workers
            if let Some(ref workers) = state.workers {
                for w in &workers.workers {
                    for m in &w.models {
                        if !models.contains(&m.id) {
                            models.push(m.id.clone());
                        }
                    }
                }
            }
        }
        drop(state);

        if models.is_empty() {
            self.set_status("No models available".to_string());
            return;
        }

        let current_idx = models.iter().position(|m| m == &self.chat_model);
        let next_idx = match current_idx {
            Some(i) => (i + 1) % models.len(),
            None => 0,
        };
        self.chat_model = models[next_idx].clone();
        self.set_status(format!("Model: {}", self.chat_model));
    }

    fn start_delete(&mut self) {
        let state = self.state.read().unwrap();
        if let Some(ref wl) = state.workers {
            let filtered: Vec<_> = wl
                .workers
                .iter()
                .filter(|w| {
                    self.active_filter.as_ref().is_none_or(|f| {
                        w.id.to_lowercase().contains(&f.to_lowercase())
                            || w.url.to_lowercase().contains(&f.to_lowercase())
                    })
                })
                .collect();

            if let Some(worker) = filtered.get(self.selected_index) {
                self.confirm_delete = Some((worker.id.clone(), worker.url.clone()));
            }
        }
    }

    async fn handle_delete_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some((ref id, _)) = self.confirm_delete {
                    let id = id.clone();
                    match self.client.delete_worker(&id).await {
                        Ok(_) => self.set_status(format!("Worker {id} deleted")),
                        Err(e) => self.set_status(format!("Error: {e}")),
                    }
                }
                self.confirm_delete = None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.confirm_delete = None;
            }
            _ => {}
        }
    }

    async fn handle_action_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => { self.show_action_menu = false; }
            KeyCode::Char('j') | KeyCode::Down => {
                let max = ActionMenuItem::all().len().saturating_sub(1);
                self.action_menu_index = (self.action_menu_index + 1).min(max);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.action_menu_index = self.action_menu_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                let item = ActionMenuItem::all()[self.action_menu_index];
                self.show_action_menu = false;
                match item {
                    ActionMenuItem::UpdatePriority => {
                        self.input_mode = InputMode::Command;
                        self.input_buffer = "priority ".to_string();
                    }
                    ActionMenuItem::UpdateCost => {
                        self.input_mode = InputMode::Command;
                        self.input_buffer = "cost ".to_string();
                    }
                    ActionMenuItem::UpdateApiKey => {
                        self.input_mode = InputMode::Command;
                        self.input_buffer = "api-key ".to_string();
                    }
                    ActionMenuItem::FlushCache => {
                        if let Some(id) = self.selected_worker_id() {
                            let url = self.selected_worker_url().unwrap_or_default();
                            self.confirm_flush = Some((id, url));
                        }
                    }
                    ActionMenuItem::ToggleHealthCheck => {
                        if let Some(id) = self.selected_worker_id() {
                            match self.client.update_worker(&id, &openai_protocol::worker::WorkerUpdateRequest {
                                priority: None, cost: None, labels: None, api_key: None,
                                health: Some(openai_protocol::worker::HealthCheckUpdate {
                                    disable_health_check: Some(true),
                                    timeout_secs: None, check_interval_secs: None,
                                    success_threshold: None, failure_threshold: None,
                                }),
                            }).await {
                                Ok(_) => self.set_status("Health check toggled".to_string()),
                                Err(e) => self.set_status(format!("Error: {e}")),
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    async fn handle_add_menu_key(&mut self, key: KeyEvent) {
        match &self.add_menu_state.clone() {
            Some(AddMenuState::SelectProvider) => match key.code {
                KeyCode::Esc => { self.add_menu_state = None; }
                KeyCode::Char('1') => {
                    self.add_menu_state = Some(AddMenuState::EnterApiKey {
                        provider: ProviderPreset::OpenAI, input: String::new(),
                    });
                }
                KeyCode::Char('2') => {
                    self.add_menu_state = Some(AddMenuState::EnterApiKey {
                        provider: ProviderPreset::Anthropic, input: String::new(),
                    });
                }
                KeyCode::Char('3') => {
                    self.add_menu_state = Some(AddMenuState::EnterApiKey {
                        provider: ProviderPreset::Xai, input: String::new(),
                    });
                }
                KeyCode::Char('4') => {
                    self.add_menu_state = Some(AddMenuState::EnterApiKey {
                        provider: ProviderPreset::Gemini, input: String::new(),
                    });
                }
                KeyCode::Char('5') | KeyCode::Char('6') => {
                    self.add_menu_state = None;
                    self.set_status("Local backend launching coming in Phase 2".to_string());
                }
                KeyCode::Char('7') => {
                    self.add_menu_state = None;
                    self.input_mode = InputMode::Command;
                    self.input_buffer = "add ".to_string();
                }
                _ => {}
            },
            Some(AddMenuState::EnterApiKey { provider, input }) => match key.code {
                KeyCode::Esc => { self.add_menu_state = None; }
                KeyCode::Enter => {
                    let provider = *provider;
                    let api_key = input.clone();
                    self.add_menu_state = None;
                    let mut spec = WorkerSpec::new(provider.url());
                    spec.provider = Some(provider.provider_type());
                    spec.runtime_type = provider.runtime_type();
                    spec.api_key = Some(api_key);
                    match self.client.add_worker(&spec).await {
                        Ok(_) => self.set_status(format!("Added {} worker", provider.label())),
                        Err(e) => self.set_status(format!("Error: {e}")),
                    }
                }
                KeyCode::Backspace => {
                    if let Some(AddMenuState::EnterApiKey { ref mut input, .. }) = self.add_menu_state {
                        input.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(AddMenuState::EnterApiKey { ref mut input, .. }) = self.add_menu_state {
                        input.push(c);
                    }
                }
                _ => {}
            },
            None => {}
        }
    }

    fn selected_worker_id(&self) -> Option<String> {
        let state = self.state.read().unwrap();
        state.workers.as_ref()
            .and_then(|w| w.workers.get(self.selected_index))
            .map(|w| w.id.clone())
    }

    fn selected_worker_url(&self) -> Option<String> {
        let state = self.state.read().unwrap();
        state.workers.as_ref()
            .and_then(|w| w.workers.get(self.selected_index))
            .map(|w| w.url.clone())
    }

    fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_clear_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
    }

    fn clamp_selection(&mut self) {
        let state = self.state.read().unwrap();
        if let Some(ref wl) = state.workers {
            let count = wl.workers.len();
            if count > 0 {
                self.selected_index = self.selected_index.min(count - 1);
            }
        }
    }
}

fn parse_provider(s: &str) -> Option<ProviderType> {
    match s.to_lowercase().as_str() {
        "openai" => Some(ProviderType::OpenAI),
        "anthropic" | "claude" => Some(ProviderType::Anthropic),
        "gemini" | "google" => Some(ProviderType::Gemini),
        "xai" | "grok" => Some(ProviderType::XAI),
        other => Some(ProviderType::Custom(other.to_string())),
    }
}
