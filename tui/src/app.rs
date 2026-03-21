use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use openai_protocol::worker::{ProviderType, RuntimeType, WorkerSpec};

use crate::{
    client::SmgClient,
    event::{AppEvent, EventHandler},
    state::SharedState,
    types::{InputMode, View},
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
            | KeyCode::Char('5')) => {
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
            KeyCode::Char('a') if self.view == View::Workers => {
                self.input_mode = InputMode::Command;
                self.input_buffer = "add ".to_string();
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
