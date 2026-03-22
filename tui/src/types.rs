use crossterm::event::KeyCode;

/// Active view/tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Pulse,
    Workers,
    Chat,
    Logs,
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
            KeyCode::Char('4') => Some(Self::Logs),
            KeyCode::Char('5') => Some(Self::Traffic),
            KeyCode::Char('6') => Some(Self::Mesh),
            _ => None,
        }
    }

    /// Human-readable label for the tab bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pulse => "Pulse",
            Self::Workers => "Workers",
            Self::Chat => "Chat",
            Self::Logs => "Logs",
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
            Self::Logs,
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
            Self::Logs => 4,
            Self::Traffic => 5,
            Self::Mesh => 6,
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
    /// Top-level: 1. External  2. Local  3. Custom URL
    SelectCategory,
    /// External: pick provider
    SelectProvider,
    /// External: enter API key
    EnterApiKey {
        provider: ProviderPreset,
        input: String,
    },
    /// Local: pick runtime (sglang/vllm)
    SelectRuntime,
    /// Local: pick connection mode (http/grpc)
    SelectConnection {
        runtime: LocalRuntime,
    },
    /// Local: pick model preset
    SelectModel {
        runtime: LocalRuntime,
        connection: LocalConnection,
    },
    /// Local: enter worker URL
    EnterLocalUrl {
        runtime: LocalRuntime,
        connection: LocalConnection,
        model: LocalModelPreset,
        input: String,
    },
    /// Custom: enter URL
    EnterCustomUrl {
        input: String,
    },
}

impl AddMenuState {
    pub fn get_input(&self) -> Option<String> {
        match self {
            Self::EnterApiKey { input, .. }
            | Self::EnterLocalUrl { input, .. }
            | Self::EnterCustomUrl { input } => Some(input.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalRuntime {
    Sglang,
    Vllm,
}

impl LocalRuntime {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Sglang => "sglang",
            Self::Vllm => "vllm",
        }
    }

    pub fn runtime_type(&self) -> openai_protocol::worker::RuntimeType {
        match self {
            Self::Sglang => openai_protocol::worker::RuntimeType::Sglang,
            Self::Vllm => openai_protocol::worker::RuntimeType::Vllm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalConnection {
    Http,
    Grpc,
}

impl LocalConnection {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Grpc => "grpc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalModelPreset {
    Preset { name: &'static str, model_id: &'static str, tp: u32 },
    Custom { model_id: String, tp: u32 },
}

impl LocalModelPreset {
    pub fn all() -> Vec<LocalModelPreset> {
        vec![
            Self::Preset { name: "Llama-3.2-1B", model_id: "meta-llama/Llama-3.2-1B-Instruct", tp: 1 },
            Self::Preset { name: "Llama-3.1-8B", model_id: "meta-llama/Llama-3.1-8B-Instruct", tp: 1 },
            Self::Preset { name: "Qwen2.5-7B", model_id: "Qwen/Qwen2.5-7B-Instruct", tp: 1 },
            Self::Preset { name: "Qwen2.5-14B", model_id: "Qwen/Qwen2.5-14B-Instruct", tp: 2 },
            Self::Preset { name: "DeepSeek-R1-7B", model_id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-7B", tp: 1 },
            Self::Preset { name: "Mistral-7B", model_id: "mistralai/Mistral-7B-Instruct-v0.3", tp: 1 },
        ]
    }

    pub fn label(&self) -> String {
        match self {
            Self::Preset { name, tp, .. } => format!("{name} (TP={tp})"),
            Self::Custom { model_id, tp } => format!("{model_id} (TP={tp})"),
        }
    }

    pub fn model_id(&self) -> &str {
        match self {
            Self::Preset { model_id, .. } => model_id,
            Self::Custom { model_id, .. } => model_id,
        }
    }

    pub fn tp(&self) -> u32 {
        match self {
            Self::Preset { tp, .. } | Self::Custom { tp, .. } => *tp,
        }
    }
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
