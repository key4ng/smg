use crossterm::event::KeyCode;

/// Active view/tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Pulse,
    Workers,
    Models,
    Traffic,
    Mesh,
}

impl View {
    /// Map a number key to a view.
    pub fn from_key(code: KeyCode) -> Option<Self> {
        match code {
            KeyCode::Char('1') => Some(Self::Pulse),
            KeyCode::Char('2') => Some(Self::Workers),
            KeyCode::Char('3') => Some(Self::Models),
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
            Self::Models => "Models",
            Self::Traffic => "Traffic",
            Self::Mesh => "Mesh",
        }
    }

    /// All views in order.
    pub fn all() -> &'static [View] {
        &[
            Self::Pulse,
            Self::Workers,
            Self::Models,
            Self::Traffic,
            Self::Mesh,
        ]
    }

    /// 1-based index for display.
    pub fn index(&self) -> usize {
        match self {
            Self::Pulse => 1,
            Self::Workers => 2,
            Self::Models => 3,
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
