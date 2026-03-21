use crossterm::event::KeyCode;

/// Active view/tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Pulse,
    Workers,
    Chat,
    Traffic,
    Mesh,
}

impl View {
    /// Map a number key to a view.
    pub fn from_key(code: KeyCode) -> Option<Self> {
        match code {
            KeyCode::Char('1') => Some(Self::Pulse),
            KeyCode::Char('2') => Some(Self::Workers),
            KeyCode::Char('3') => Some(Self::Chat),
            KeyCode::Char('4') => Some(Self::Traffic),
            KeyCode::Char('5') => Some(Self::Mesh),
            _ => None,
        }
    }

    /// Human-readable label for the tab bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pulse => "Pulse",
            Self::Workers => "Workers",
            Self::Chat => "Chat",
            Self::Traffic => "Traffic",
            Self::Mesh => "Mesh",
        }
    }

    /// All views in order.
    pub fn all() -> &'static [View] {
        &[
            Self::Pulse,
            Self::Workers,
            Self::Chat,
            Self::Traffic,
            Self::Mesh,
        ]
    }

    /// 1-based index for display.
    pub fn index(&self) -> usize {
        match self {
            Self::Pulse => 1,
            Self::Workers => 2,
            Self::Chat => 3,
            Self::Traffic => 4,
            Self::Mesh => 5,
        }
    }
}

/// Input mode determines how keystrokes are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal navigation mode — keys are shortcuts.
    #[default]
    Normal,
    /// Filter mode — typing populates the filter bar (prefix: `/`).
    Filter,
    /// Command mode — typing populates the command bar (prefix: `:`).
    Command,
}

/// State machine for the Add Worker menu flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddMenuState {
    /// Selecting provider type (1-7 menu).
    SelectProvider,
    /// Typing API key for external provider.
    EnterApiKey {
        provider: ProviderPreset,
        input: String,
    },
}

/// Preset provider for quick-add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPreset {
    OpenAI,
    Anthropic,
    Xai,
    Gemini,
}

impl ProviderPreset {
    pub fn url(&self) -> &'static str {
        match self {
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Xai => "https://api.x.ai/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Xai => "xAI",
            Self::Gemini => "Gemini",
        }
    }

    pub fn provider_type(&self) -> openai_protocol::worker::ProviderType {
        match self {
            Self::OpenAI => openai_protocol::worker::ProviderType::OpenAI,
            Self::Anthropic => openai_protocol::worker::ProviderType::Anthropic,
            Self::Xai => openai_protocol::worker::ProviderType::XAI,
            Self::Gemini => openai_protocol::worker::ProviderType::Gemini,
        }
    }

    pub fn runtime_type(&self) -> openai_protocol::worker::RuntimeType {
        openai_protocol::worker::RuntimeType::External
    }

    pub fn all() -> &'static [ProviderPreset] {
        &[Self::OpenAI, Self::Anthropic, Self::Xai, Self::Gemini]
    }
}

/// Items in the worker action menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionMenuItem {
    UpdatePriority,
    UpdateCost,
    UpdateApiKey,
    FlushCache,
    ToggleHealthCheck,
}

impl ActionMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            Self::UpdatePriority => "Update priority",
            Self::UpdateCost => "Update cost",
            Self::UpdateApiKey => "Update API key",
            Self::FlushCache => "Flush cache",
            Self::ToggleHealthCheck => "Toggle health check",
        }
    }

    pub fn all() -> &'static [ActionMenuItem] {
        &[
            Self::UpdatePriority,
            Self::UpdateCost,
            Self::UpdateApiKey,
            Self::FlushCache,
            Self::ToggleHealthCheck,
        ]
    }
}
