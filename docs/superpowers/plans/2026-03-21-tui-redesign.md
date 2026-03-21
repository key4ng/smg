# SMG TUI Dashboard Redesign — Implementation Plan (Phase 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the SMG TUI with Tokyo Night theme, persistent stats bar, sparkline charts, worker detail panel, action menus, models view, and responsive layout.

**Architecture:** Replace the existing header with a stats bar + tab bar. Upgrade the client to deserialize full `SchedulerLoadSnapshot` data. Add sparkline history buffers to shared state. Restructure UI modules into smaller focused components. Add worker action menu and quick-add presets for external providers.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, tokio, reqwest, openai-protocol (workspace crate)

**Spec:** `docs/superpowers/specs/2026-03-21-tui-redesign-design.md`

---

## File Map

### New files
| File | Responsibility |
|------|----------------|
| `tui/src/ui/theme.rs` | Tokyo Night color constants |
| `tui/src/ui/stats_bar.rs` | Persistent top metrics strip |
| `tui/src/ui/tabs.rs` | Tab bar rendering |
| `tui/src/ui/sparkline.rs` | Sparkline rendering utility |
| `tui/src/ui/detail.rs` | Worker detail bottom panel |
| `tui/src/ui/models.rs` | Models table + detail view |
| `tui/src/ui/action_menu.rs` | Worker action menu + add menu |

### Modified files
| File | Changes |
|------|---------|
| `tui/src/client.rs` | Add `update_worker()`, `flush_worker_cache()`; update `WorkerLoad` to include `details` |
| `tui/src/state.rs` | Add sparkline history buffers; update `poll_once` to populate them |
| `tui/src/types.rs` | Add `AddMenuState` enum, `ActionMenuItem` enum |
| `tui/src/app.rs` | Add new state fields (`show_detail`, `show_action_menu`, `add_menu_state`, etc.); add new key handlers; add new commands |
| `tui/src/ui/mod.rs` | Update layout (stats bar + tabs + content + footer); add new module declarations |
| `tui/src/ui/pulse.rs` | Rewrite to 2-column layout with sparklines; apply theme |
| `tui/src/ui/workers.rs` | Apply theme; integrate detail panel split |
| `tui/src/ui/footer.rs` | Apply theme; add new keybinding hints |
| `tui/src/ui/dialog.rs` | Apply theme; add flush-cache confirmation |
| `tui/src/ui/filter.rs` | Apply theme |
| `tui/src/ui/help.rs` | Apply theme; update help text with new commands |
| `tui/src/lib.rs` | No changes (new modules auto-discovered via ui/mod.rs) |
| `tui/src/main.rs` | No changes |
| `tui/src/event.rs` | No changes |

### Deleted files
| File | Reason |
|------|--------|
| `tui/src/ui/header.rs` | Replaced by `stats_bar.rs` + `tabs.rs` |

---

## Task 1: Theme Constants

**Files:**
- Create: `tui/src/ui/theme.rs`
- Modify: `tui/src/ui/mod.rs` (add `pub mod theme;`)

- [ ] **Step 1: Create theme module**

```rust
// tui/src/ui/theme.rs
use ratatui::style::{Color, Modifier, Style};

// Tokyo Night palette
pub const BG: Color = Color::Rgb(26, 27, 38);         // #1a1b26
pub const PANEL_BG: Color = Color::Rgb(36, 40, 59);   // #24283b
pub const STATS_BG: Color = Color::Rgb(30, 32, 48);   // #1e2030
pub const BORDER: Color = Color::Rgb(59, 66, 97);     // #3b4261
pub const TEXT: Color = Color::Rgb(192, 202, 245);     // #c0caf5
pub const TEXT_MUTED: Color = Color::Rgb(86, 95, 137); // #565f89
pub const ACCENT: Color = Color::Rgb(122, 162, 247);   // #7aa2f7
pub const GREEN: Color = Color::Rgb(158, 206, 106);    // #9ece6a
pub const YELLOW: Color = Color::Rgb(224, 175, 104);   // #e0af68
pub const RED: Color = Color::Rgb(247, 118, 142);      // #f7768e
pub const PURPLE: Color = Color::Rgb(187, 154, 247);   // #bb9af7

/// Style for panel titles (accent + bold).
pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Style for section labels (muted + uppercase convention).
pub fn label() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Style for primary text.
pub fn text() -> Style {
    Style::default().fg(TEXT)
}

/// Severity color for a 0.0–1.0 ratio.
pub fn severity(ratio: f64) -> Color {
    if ratio < 0.5 {
        GREEN
    } else if ratio < 0.8 {
        YELLOW
    } else {
        RED
    }
}

/// Standard panel block with border.
pub fn panel(title: &str) -> ratatui::widgets::Block<'_> {
    ratatui::widgets::Block::default()
        .title(title)
        .title_style(title())
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG))
}
```

- [ ] **Step 2: Register module in ui/mod.rs**

Add `pub mod theme;` to `tui/src/ui/mod.rs` at the top of the module declarations.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add tui/src/ui/theme.rs tui/src/ui/mod.rs
git commit -m "feat(tui): add Tokyo Night theme constants"
```

---

## Task 2: Sparkline Utility

**Files:**
- Create: `tui/src/ui/sparkline.rs`
- Modify: `tui/src/ui/mod.rs` (add `pub mod sparkline;`)

- [ ] **Step 1: Create sparkline renderer**

```rust
// tui/src/ui/sparkline.rs
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::collections::VecDeque;

/// Unicode block characters for 8-level sparkline.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render a sparkline from a VecDeque of f64 values.
/// Scales values to min/max of the buffer and maps to 8 Unicode block levels.
pub fn render_sparkline(
    f: &mut Frame,
    data: &VecDeque<f64>,
    color: Color,
    area: Rect,
) {
    if data.is_empty() || area.width == 0 {
        return;
    }

    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;

    let width = area.width as usize;
    // Take the last `width` data points (or all if fewer).
    let start = data.len().saturating_sub(width);
    let chars: String = data
        .iter()
        .skip(start)
        .map(|&v| {
            let idx = if range <= f64::EPSILON {
                3 // middle level if all values are the same
            } else {
                ((v - min) / range * 7.0).round() as usize
            };
            BLOCKS[idx.min(7)]
        })
        .collect();

    let line = Line::from(Span::styled(chars, Style::default().fg(color)));
    f.render_widget(Paragraph::new(line), area);
}

/// Build a gauge bar string: filled portion + empty portion.
/// Returns (filled_str, empty_str, percentage).
pub fn gauge_bar(ratio: f64, width: usize) -> (String, String, u16) {
    let pct = (ratio * 100.0).round() as u16;
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    ("▓".repeat(filled), "░".repeat(empty), pct)
}
```

- [ ] **Step 2: Register module**

Add `pub mod sparkline;` to `tui/src/ui/mod.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add tui/src/ui/sparkline.rs tui/src/ui/mod.rs
git commit -m "feat(tui): add sparkline rendering utility"
```

---

## Task 3: Update Client — Load Details + Worker Actions

**Files:**
- Modify: `tui/src/client.rs`

- [ ] **Step 1: Update WorkerLoad struct to include details**

In `tui/src/client.rs`, update the `WorkerLoad` struct and add imports:

```rust
// Add to imports at top of client.rs:
use openai_protocol::worker::{WorkerUpdateRequest, WorkerApiResponse, WorkerLoadResponse};

// Replace existing WorkerLoad struct:
#[derive(Clone, Debug, Deserialize)]
pub struct WorkerLoad {
    pub worker: String,
    #[serde(default)]
    pub worker_type: Option<String>,
    pub load: isize,
    #[serde(default)]
    pub details: Option<WorkerLoadResponse>,
}
```

- [ ] **Step 2: Add update_worker method**

Add to `impl SmgClient`:

```rust
pub async fn update_worker(
    &self,
    id: &str,
    update: &WorkerUpdateRequest,
) -> Result<WorkerApiResponse> {
    let resp = self
        .request(reqwest::Method::PATCH, &format!("/workers/{id}"))
        .json(update)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp)
}
```

- [ ] **Step 3: Add flush_worker_cache method**

Add to `impl SmgClient`:

```rust
pub async fn flush_worker_cache(&self, id: &str) -> Result<serde_json::Value> {
    let resp = self
        .request(reqwest::Method::POST, &format!("/workers/{id}/flush_cache"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp)
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles (there may be unused import warnings for now — that's fine)

- [ ] **Step 5: Commit**

```bash
git add tui/src/client.rs
git commit -m "feat(tui): add worker update/flush client methods and load details"
```

---

## Task 4: Update Shared State — Sparkline Buffers

**Files:**
- Modify: `tui/src/state.rs`

- [ ] **Step 1: Add sparkline history fields to GatewayState**

Add to imports in `state.rs`:
```rust
use std::collections::{HashMap, VecDeque};
```

Add new fields to `GatewayState` struct (after existing fields):
```rust
    /// Rolling aggregate throughput history (capacity: 20).
    #[allow(clippy::zero_sized_map_values)]
    pub throughput_history: VecDeque<f64>,
    /// Rolling aggregate cache hit rate history (capacity: 20).
    pub cache_hit_history: VecDeque<f64>,
    /// Per-worker throughput history.
    pub per_worker_throughput: HashMap<String, VecDeque<f64>>,
    /// Per-worker cache hit rate history.
    pub per_worker_cache_hit: HashMap<String, VecDeque<f64>>,
```

- [ ] **Step 2: Update poll_once to populate sparkline buffers**

At the end of the `poll_once` function, after the existing state updates (inside the write lock block), add logic to extract sparkline data from loads:

```rust
// Inside poll_once, after updating state.loads:
const SPARKLINE_CAP: usize = 20;

if let Some(ref loads_resp) = state.loads {
    let mut total_throughput = 0.0_f64;
    let mut total_cache_hits = 0.0_f64;
    let mut worker_count = 0_u32;

    for wl in &loads_resp.workers {
        if let Some(ref details) = wl.details {
            let throughput: f64 = details
                .loads
                .iter()
                .map(|s| s.gen_throughput)
                .sum();
            let cache_hit: f64 = if details.loads.is_empty() {
                0.0
            } else {
                details.loads.iter().map(|s| s.cache_hit_rate).sum::<f64>()
                    / details.loads.len() as f64
            };

            total_throughput += throughput;
            total_cache_hits += cache_hit;
            worker_count += 1;

            // Per-worker sparklines
            let th = state
                .per_worker_throughput
                .entry(wl.worker.clone())
                .or_insert_with(|| VecDeque::with_capacity(SPARKLINE_CAP));
            if th.len() >= SPARKLINE_CAP {
                th.pop_front();
            }
            th.push_back(throughput);

            let ch = state
                .per_worker_cache_hit
                .entry(wl.worker.clone())
                .or_insert_with(|| VecDeque::with_capacity(SPARKLINE_CAP));
            if ch.len() >= SPARKLINE_CAP {
                ch.pop_front();
            }
            ch.push_back(cache_hit);
        }
    }

    // Aggregate sparklines
    if state.throughput_history.len() >= SPARKLINE_CAP {
        state.throughput_history.pop_front();
    }
    state.throughput_history.push_back(total_throughput);

    let avg_cache = if worker_count > 0 {
        total_cache_hits / worker_count as f64
    } else {
        0.0
    };
    if state.cache_hit_history.len() >= SPARKLINE_CAP {
        state.cache_hit_history.pop_front();
    }
    state.cache_hit_history.push_back(avg_cache);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add tui/src/state.rs
git commit -m "feat(tui): add sparkline history buffers to shared state"
```

---

## Task 5: Update Types — AddMenuState, ActionMenuItem

**Files:**
- Modify: `tui/src/types.rs`

- [ ] **Step 1: Add AddMenuState and ActionMenuItem enums**

Add after the existing `InputMode` enum:

```rust
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

    /// Returns the ProviderType for use in WorkerSpec.
    pub fn provider_type(&self) -> openai_protocol::worker::ProviderType {
        match self {
            Self::OpenAI => openai_protocol::worker::ProviderType::OpenAI,
            Self::Anthropic => openai_protocol::worker::ProviderType::Anthropic,
            Self::Xai => openai_protocol::worker::ProviderType::XAI,
            Self::Gemini => openai_protocol::worker::ProviderType::Gemini,
        }
    }

    /// Returns the RuntimeType (always External for presets).
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles (unused warnings OK for now)

- [ ] **Step 3: Commit**

```bash
git add tui/src/types.rs
git commit -m "feat(tui): add AddMenuState and ActionMenuItem types"
```

---

## Task 6: Stats Bar + Tab Bar (replace header.rs)

**Files:**
- Create: `tui/src/ui/stats_bar.rs`
- Create: `tui/src/ui/tabs.rs`
- Delete: `tui/src/ui/header.rs`
- Modify: `tui/src/ui/mod.rs`

- [ ] **Step 1: Create stats_bar.rs**

```rust
// tui/src/ui/stats_bar.rs
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::theme;
use crate::state::GatewayState;

/// Render the persistent stats bar (1 row: workers | models | throughput | load).
pub fn render_stats_bar(f: &mut Frame, state: &GatewayState, area: Rect) {
    let bg = Style::default().bg(theme::STATS_BG);
    f.render_widget(ratatui::widgets::Block::default().style(bg), area);

    let chunks = Layout::horizontal([
        Constraint::Ratio(1, 5), // logo + connection
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
    ])
    .split(area);

    // Logo + connection
    let conn = if state.connected {
        Span::styled("● connected", Style::default().fg(theme::GREEN))
    } else {
        Span::styled("● disconnected", Style::default().fg(theme::RED))
    };
    let logo = Line::from(vec![
        Span::styled("⎔ SMG ", theme::title()),
        conn,
    ]);
    f.render_widget(Paragraph::new(logo).style(bg), chunks[0]);

    // Workers
    let (total, healthy) = state
        .workers
        .as_ref()
        .map(|w| {
            let h = w.workers.iter().filter(|w| w.is_healthy).count();
            (w.total, h)
        })
        .unwrap_or((0, 0));
    let workers_line = if state.connected {
        let unhealthy = total.saturating_sub(healthy);
        let health_span = if unhealthy == 0 {
            Span::styled("all healthy", Style::default().fg(theme::GREEN))
        } else {
            Span::styled(
                format!("{unhealthy} unhealthy"),
                Style::default().fg(theme::RED),
            )
        };
        Line::from(vec![
            Span::styled("WORKERS ", theme::label()),
            Span::styled(total.to_string(), theme::text()),
            Span::raw(" "),
            health_span,
        ])
    } else {
        Line::from(vec![
            Span::styled("WORKERS ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(workers_line).style(bg), chunks[1]);

    // Models
    let model_count = state
        .models
        .as_ref()
        .map(|m| m.data.len())
        .unwrap_or(0);
    let models_line = if state.connected {
        Line::from(vec![
            Span::styled("MODELS ", theme::label()),
            Span::styled(model_count.to_string(), theme::text()),
        ])
    } else {
        Line::from(vec![
            Span::styled("MODELS ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(models_line).style(bg), chunks[2]);

    // Throughput
    let throughput = state.throughput_history.back().copied().unwrap_or(0.0);
    let tp_line = if state.connected {
        Line::from(vec![
            Span::styled("THROUGHPUT ", theme::label()),
            Span::styled(format!("{throughput:.0}"), theme::text()),
            Span::styled(" tok/s", theme::label()),
        ])
    } else {
        Line::from(vec![
            Span::styled("THROUGHPUT ", theme::label()),
            Span::styled("--", theme::text()),
        ])
    };
    f.render_widget(Paragraph::new(tp_line).style(bg), chunks[3]);

    // Avg Load
    let (avg_load, load_color) = if state.connected {
        let loads = state.loads.as_ref();
        let avg = loads
            .map(|l| {
                if l.workers.is_empty() {
                    0.0
                } else {
                    l.workers.iter().map(|w| w.load as f64).sum::<f64>()
                        / l.workers.len() as f64
                }
            })
            .unwrap_or(0.0);
        let ratio = avg / 100.0;
        (format!("{avg:.0}%"), theme::severity(ratio))
    } else {
        ("--".to_string(), theme::TEXT_MUTED)
    };
    let load_line = Line::from(vec![
        Span::styled("LOAD ", theme::label()),
        Span::styled(avg_load, Style::default().fg(load_color)),
    ]);
    f.render_widget(Paragraph::new(load_line).style(bg), chunks[4]);
}
```

- [ ] **Step 2: Create tabs.rs**

```rust
// tui/src/ui/tabs.rs
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::theme;
use crate::types::View;

/// Render the tab bar.
pub fn render_tabs(f: &mut Frame, active: View, area: Rect) {
    let bg = Style::default().bg(theme::PANEL_BG);

    let tabs: Vec<Span> = View::all()
        .iter()
        .enumerate()
        .flat_map(|(i, view)| {
            let num = format!("{}", i + 1);
            let label = view.label();
            let style = if *view == active {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            vec![
                Span::styled(format!("{num}:{label}"), style),
                Span::raw("  "),
            ]
        })
        .collect();

    f.render_widget(Paragraph::new(Line::from(tabs)).style(bg), area);
}
```

- [ ] **Step 3: Delete header.rs and update mod.rs**

Delete `tui/src/ui/header.rs`.

Update `tui/src/ui/mod.rs`:
- Remove `mod header;`
- Add `pub mod stats_bar;`
- Add `pub mod tabs;`
- (theme and sparkline modules should already be declared from Tasks 1-2)

- [ ] **Step 4: Update render() layout in mod.rs**

Replace the existing `render()` function body in `tui/src/ui/mod.rs`:

```rust
pub fn render(f: &mut Frame, app: &App) {
    let state = app.state.read().unwrap();

    // Layout: stats_bar (1) + tabs (1) + content (fill) + footer (2)
    let chunks = Layout::vertical([
        Constraint::Length(1), // stats bar
        Constraint::Length(1), // tabs
        Constraint::Min(10),   // content
        Constraint::Length(2), // footer
    ])
    .split(f.area());

    // Background
    f.render_widget(
        ratatui::widgets::Block::default().style(
            ratatui::style::Style::default().bg(theme::BG),
        ),
        f.area(),
    );

    stats_bar::render_stats_bar(f, &state, chunks[0]);
    tabs::render_tabs(f, app.view, chunks[1]);

    match app.view {
        View::Pulse => pulse::render_pulse(f, app, chunks[2]),
        View::Workers => workers::render_workers(f, app, chunks[2]),
        View::Models => render_placeholder(f, View::Models, chunks[2]),
        View::Traffic => render_placeholder(f, View::Traffic, chunks[2]),
        View::Mesh => render_placeholder(f, View::Mesh, chunks[2]),
    }

    footer::render_footer(f, app, chunks[3]);

    // Overlays (rendered last)
    if app.show_help {
        help::render_help(f);
    }
    if app.confirm_delete.is_some() {
        dialog::render_delete_dialog(f, app);
    }
    if app.input_mode != crate::types::InputMode::Normal {
        filter::render_filter(f, app, chunks[3]);
    }

    drop(state);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles (some warnings about unused functions in pulse.rs/workers.rs are OK — we'll update those next)

- [ ] **Step 6: Commit**

```bash
git rm tui/src/ui/header.rs
git add tui/src/ui/stats_bar.rs tui/src/ui/tabs.rs tui/src/ui/mod.rs
git commit -m "feat(tui): replace header with stats bar + tab bar"
```

---

## Task 7: Redesign Pulse Dashboard (2-Column with Sparklines)

**Files:**
- Modify: `tui/src/ui/pulse.rs`

- [ ] **Step 1: Rewrite pulse.rs**

Rewrite the entire `pulse.rs`. Delete the existing 5 functions and replace with the following structure:

```rust
// tui/src/ui/pulse.rs
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{sparkline, theme};
use crate::app::App;
use crate::state::GatewayState;

pub fn render_pulse(f: &mut Frame, app: &App, area: Rect) {
    let state = app.state.read().unwrap();

    // 2-column split
    let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: 3 stacked panels (Worker Health, Cluster, Rate Limits)
    let left = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(columns[0]);

    // Right: 3 stacked panels (Throughput sparkline, Token Usage bars, Cache Hit sparkline)
    let right = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(columns[1]);

    render_worker_health(f, &state, left[0]);
    render_cluster(f, &state, left[1]);
    render_rate_limits(f, &state, left[2]);
    render_throughput(f, &state, right[0]);
    render_token_usage(f, &state, right[1]);
    render_cache_hit(f, &state, right[2]);

    drop(state);
}

fn render_worker_health(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Worker Health ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (total, healthy, stats) = state
        .workers
        .as_ref()
        .map(|w| {
            let h = w.workers.iter().filter(|w| w.is_healthy).count();
            (w.total, h, &w.stats)
        })
        .unwrap_or((0, 0, /* need a default — use separate extraction */));

    // Extract from workers response
    let mut lines = Vec::new();
    if let Some(ref wr) = state.workers {
        let healthy = wr.workers.iter().filter(|w| w.is_healthy).count();
        let unhealthy = wr.total.saturating_sub(healthy);
        lines.push(Line::from(vec![
            Span::styled(format!("{healthy}"), Style::default().fg(theme::GREEN)),
            Span::styled(" healthy  ", theme::label()),
            Span::styled(format!("{unhealthy}"), Style::default().fg(if unhealthy > 0 { theme::RED } else { theme::TEXT_MUTED })),
            Span::styled(" unhealthy", theme::label()),
        ]));
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(vec![
            Span::styled("BY TYPE  ", theme::label()),
            Span::styled(format!("regular: {}  ", wr.stats.regular_count), theme::text()),
            Span::styled(format!("prefill: {}  ", wr.stats.prefill_count), theme::text()),
            Span::styled(format!("decode: {}", wr.stats.decode_count), theme::text()),
        ]));
    } else {
        lines.push(Line::from(Span::styled("No data", theme::label())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_cluster(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Cluster ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(ref cluster) = state.cluster {
        if let Some(ref name) = cluster.node_name {
            lines.push(Line::from(vec![
                Span::styled("node: ", theme::label()),
                Span::styled(name, theme::text()),
            ]));
        }
        if let Some(size) = cluster.cluster_size {
            lines.push(Line::from(vec![
                Span::styled("size: ", theme::label()),
                Span::styled(size.to_string(), theme::text()),
            ]));
        }
        if let Some(ref stores) = cluster.stores {
            let store_spans: Vec<Span> = stores
                .iter()
                .flat_map(|s| {
                    let color = if s.healthy { theme::GREEN } else { theme::RED };
                    vec![
                        Span::styled("● ", Style::default().fg(color)),
                        Span::styled(format!("{}  ", s.name), theme::text()),
                    ]
                })
                .collect();
            lines.push(Line::from(vec![Span::styled("stores: ", theme::label())]));
            lines.push(Line::from(store_spans));
        }
    } else {
        lines.push(Line::from(Span::styled("No cluster data", theme::label())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_rate_limits(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Rate Limits ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(ref rl) = state.rate_limits {
        let limit = rl.limit.unwrap_or(0);
        let current = rl.current.unwrap_or(0);
        let remaining = rl.remaining.unwrap_or(0);
        lines.push(Line::from(vec![
            Span::styled("limit: ", theme::label()),
            Span::styled(limit.to_string(), theme::text()),
            Span::raw("  "),
            Span::styled("current: ", theme::label()),
            Span::styled(current.to_string(), theme::text()),
            Span::raw("  "),
            Span::styled("remaining: ", theme::label()),
            Span::styled(remaining.to_string(), theme::text()),
        ]));
        let ratio = if limit > 0 { current as f64 / limit as f64 } else { 0.0 };
        let color = theme::severity(ratio);
        let (filled, empty, pct) = sparkline::gauge_bar(ratio, 20);
        lines.push(Line::from(vec![
            Span::styled(filled, Style::default().fg(color)),
            Span::styled(empty, Style::default().fg(theme::BORDER)),
            Span::styled(format!(" {pct}%"), theme::text()),
        ]));
    } else {
        lines.push(Line::from(Span::styled("No rate limit data", theme::label())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_throughput(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Throughput (last 60s) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner into sparkline area + time labels
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    sparkline::render_sparkline(f, &state.throughput_history, theme::GREEN, rows[0]);

    let latest = state.throughput_history.back().copied().unwrap_or(0.0);
    let time_line = Line::from(vec![
        Span::styled("-60s", theme::label()),
        Span::raw(format!("{:>width$}", format!("{latest:.0} tok/s now"), width = (rows[1].width as usize).saturating_sub(4))),
    ]);
    f.render_widget(Paragraph::new(time_line).style(theme::label()), rows[1]);
}

fn render_token_usage(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Token Usage by Worker ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(ref loads) = state.loads {
        for wl in loads.workers.iter().take(inner.height as usize) {
            let ratio = if let Some(ref details) = wl.details {
                details.effective_token_usage()
            } else {
                0.0
            };
            let color = theme::severity(ratio);
            let (filled, empty, pct) = sparkline::gauge_bar(ratio, 16);
            // Truncate worker URL to fit
            let name: String = wl.worker.chars().take(12).collect();
            lines.push(Line::from(vec![
                Span::styled(format!("{name:<12} "), theme::text()),
                Span::styled(filled, Style::default().fg(color)),
                Span::styled(empty, Style::default().fg(theme::BORDER)),
                Span::styled(format!(" {pct}%"), theme::text()),
            ]));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("No load data", theme::label())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_cache_hit(f: &mut Frame, state: &GatewayState, area: Rect) {
    let block = theme::panel(" Cache Hit Rate (last 60s) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    sparkline::render_sparkline(f, &state.cache_hit_history, theme::PURPLE, rows[0]);

    let latest = state.cache_hit_history.back().copied().unwrap_or(0.0);
    let time_line = Line::from(vec![
        Span::styled("-60s", theme::label()),
        Span::raw(format!("{:>width$}", format!("{:.1}% now", latest * 100.0), width = (rows[1].width as usize).saturating_sub(4))),
    ]);
    f.render_widget(Paragraph::new(time_line).style(theme::label()), rows[1]);
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add tui/src/ui/pulse.rs
git commit -m "feat(tui): redesign pulse dashboard with 2-column layout and sparklines"
```

---

## Task 8: Redesign Workers Table (Theme + Prepare for Detail Panel)

**Files:**
- Modify: `tui/src/ui/workers.rs`

- [ ] **Step 1: Apply Tokyo Night theme to workers table**

Update all hardcoded colors in `workers.rs`:
- Header row: `theme::ACCENT` fg, `theme::PANEL_BG` bg
- Normal rows: `theme::TEXT` fg, `theme::BG` bg
- Selected row: `theme::TEXT` fg, `theme::BORDER` bg (highlight)
- Health "healthy": `theme::GREEN`, "unhealthy": `theme::RED`
- Muted text (runtime, mode): `theme::TEXT_MUTED`
- Block border: `theme::BORDER`
- Block title: `theme::title()`

Replace: `Color::Cyan` → `theme::ACCENT`, `Color::Green` → `theme::GREEN`, `Color::Red` → `theme::RED`, `Color::DarkGray` → `theme::TEXT_MUTED`, `Color::White` → `theme::TEXT`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add tui/src/ui/workers.rs
git commit -m "feat(tui): apply Tokyo Night theme to workers table"
```

---

## Task 9: Worker Detail Panel

**Files:**
- Create: `tui/src/ui/detail.rs`
- Modify: `tui/src/ui/mod.rs` (add `pub mod detail;`)
- Modify: `tui/src/ui/workers.rs` (split layout when detail is visible)
- Modify: `tui/src/app.rs` (add `show_detail` field, `Enter`/`Esc` handling)

- [ ] **Step 1: Add show_detail field to App**

In `tui/src/app.rs`, add to `App` struct:
```rust
pub show_detail: bool,
```

Initialize as `false` in `App::new()`.

In `handle_normal()`, add `Enter` key handler:
```rust
KeyCode::Enter => {
    if self.view == View::Workers {
        self.show_detail = !self.show_detail;
    }
}
```

Update `Esc` in normal mode to close detail if open:
```rust
KeyCode::Esc => {
    if self.show_detail {
        self.show_detail = false;
    }
}
```

- [ ] **Step 2: Create detail.rs**

```rust
// tui/src/ui/detail.rs
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{sparkline, theme};
use crate::app::App;
use crate::client::WorkerInfo;
use crate::state::GatewayState;

/// Render the worker detail bottom panel.
pub fn render_detail(f: &mut Frame, app: &App, worker: &WorkerInfo, area: Rect) {
    let state = app.state.read().unwrap();

    // Border
    let block = theme::panel(&format!(" {} ", worker.id));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // 3 columns: config | load | sparklines
    let cols = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(inner);

    // Left: configuration
    render_config(f, worker, cols[0]);

    // Middle: load by DP rank
    render_load(f, worker, &state, cols[1]);

    // Right: sparklines
    render_sparklines(f, worker, &state, cols[2]);

    drop(state);
}

fn render_config(f: &mut Frame, worker: &WorkerInfo, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled("CONFIGURATION", theme::label()),
        ]),
        Line::from(vec![
            Span::styled("url: ", theme::label()),
            Span::styled(&worker.url, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("runtime: ", theme::label()),
            Span::styled(&worker.runtime_type, theme::text()),
            Span::raw("  "),
            Span::styled("mode: ", theme::label()),
            Span::styled(&worker.connection_mode, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("health: ", theme::label()),
            if worker.is_healthy {
                Span::styled("● healthy", Style::default().fg(theme::GREEN))
            } else {
                Span::styled("● unhealthy", Style::default().fg(theme::RED))
            },
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn render_load(f: &mut Frame, worker: &WorkerInfo, state: &GatewayState, area: Rect) {
    let mut lines = vec![Line::from(Span::styled("LOAD", theme::label()))];

    // Find this worker's load details
    if let Some(ref loads) = state.loads {
        if let Some(wl) = loads.workers.iter().find(|w| w.worker == worker.url) {
            if let Some(ref details) = wl.details {
                for snap in &details.loads {
                    let ratio = snap.token_usage;
                    let color = theme::severity(ratio);
                    let (filled, empty, pct) = sparkline::gauge_bar(ratio, 16);
                    lines.push(Line::from(vec![
                        Span::styled(format!("rank-{} ", snap.dp_rank), theme::label()),
                        Span::styled(filled, Style::default().fg(color)),
                        Span::styled(empty, Style::default().fg(theme::BORDER)),
                        Span::styled(format!(" {pct}%"), theme::text()),
                    ]));
                }
                // Summary
                let total_running: i32 = details.loads.iter().map(|s| s.num_running_reqs).sum();
                let total_waiting: i32 = details.loads.iter().map(|s| s.num_waiting_reqs).sum();
                lines.push(Line::from(vec![
                    Span::styled("running: ", theme::label()),
                    Span::styled(total_running.to_string(), theme::text()),
                    Span::raw("  "),
                    Span::styled("waiting: ", theme::label()),
                    Span::styled(total_waiting.to_string(), theme::text()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("load: ", theme::label()),
                    Span::styled(wl.load.to_string(), theme::text()),
                ]));
            }
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn render_sparklines(f: &mut Frame, worker: &WorkerInfo, state: &GatewayState, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1), // label
        Constraint::Length(1), // sparkline
        Constraint::Length(1), // spacer
        Constraint::Length(1), // label
        Constraint::Length(1), // sparkline
        Constraint::Min(0),    // rest
    ])
    .split(area);

    f.render_widget(
        Paragraph::new(Span::styled("THROUGHPUT", theme::label())),
        rows[0],
    );
    if let Some(data) = state.per_worker_throughput.get(&worker.url) {
        sparkline::render_sparkline(f, data, theme::GREEN, rows[1]);
    }

    f.render_widget(
        Paragraph::new(Span::styled("CACHE HIT", theme::label())),
        rows[3],
    );
    if let Some(data) = state.per_worker_cache_hit.get(&worker.url) {
        sparkline::render_sparkline(f, data, theme::PURPLE, rows[4]);
    }
}
```

- [ ] **Step 3: Update workers.rs to split layout when detail is open**

In `render_workers()` in `workers.rs`, wrap the existing table rendering with a layout split:

```rust
pub fn render_workers(f: &mut Frame, app: &App, area: Rect) {
    let state = app.state.read().unwrap();

    // Get filtered workers list
    let filtered: Vec<&WorkerInfo> = /* existing filter logic */;

    let (table_area, detail_area) = if app.show_detail {
        let split = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        (split[0], Some(split[1]))
    } else {
        (area, None)
    };

    // Render table in table_area (existing table code)
    // ...

    // Render detail panel if visible
    if let Some(detail_area) = detail_area {
        if let Some(worker) = filtered.get(app.selected_index) {
            detail::render_detail(f, app, worker, detail_area);
        }
    }

    drop(state);
}
```

- [ ] **Step 4: Register detail module**

Add `pub mod detail;` to `tui/src/ui/mod.rs`.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p smg-tui`
Expected: compiles

- [ ] **Step 6: Commit**

```bash
git add tui/src/ui/detail.rs tui/src/ui/workers.rs tui/src/ui/mod.rs tui/src/app.rs
git commit -m "feat(tui): add worker detail bottom panel"
```

---

## Task 10: Action Menu

**Files:**
- Create: `tui/src/ui/action_menu.rs`
- Modify: `tui/src/ui/mod.rs` (add `pub mod action_menu;`)
- Modify: `tui/src/app.rs` (add action menu state + key handlers)

- [ ] **Step 1: Add action menu state to App**

In `app.rs`, add fields to `App`:
```rust
pub show_action_menu: bool,
pub action_menu_index: usize,
pub add_menu_state: Option<AddMenuState>,
```

Initialize all as `false` / `0` / `None` in `App::new()`.

- [ ] **Step 2: Add key handlers for action menu**

In `handle_normal()`, add `e` key handler:
```rust
KeyCode::Char('e') => {
    if self.view == View::Workers && !self.show_action_menu {
        self.show_action_menu = true;
        self.action_menu_index = 0;
    }
}
```

Update `a` key handler to open add menu instead of direct command:
```rust
KeyCode::Char('a') => {
    if self.view == View::Workers {
        self.add_menu_state = Some(AddMenuState::SelectProvider);
    }
}
```

Add a new method `handle_action_menu_key()`:

```rust
async fn handle_action_menu_key(&mut self, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            self.show_action_menu = false;
        }
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
                    // Requires confirmation — set up confirm dialog
                    if let Some(id) = self.selected_worker_id() {
                        let url = self.selected_worker_url().unwrap_or_default();
                        self.confirm_flush = Some((id, url));
                    }
                }
                ActionMenuItem::ToggleHealthCheck => {
                    if let Some(id) = self.selected_worker_id() {
                        self.cmd_toggle_health(&id).await;
                    }
                }
            }
        }
        KeyCode::Char(c) if ('1'..='5').contains(&c) => {
            let idx = (c as usize) - ('1' as usize);
            if idx < ActionMenuItem::all().len() {
                self.action_menu_index = idx;
                // Simulate Enter
                self.handle_action_menu_key(KeyEvent::from(KeyCode::Enter)).await;
            }
        }
        _ => {}
    }
}
```

Add a new method `handle_add_menu_key()`:

```rust
async fn handle_add_menu_key(&mut self, key: KeyEvent) {
    match &self.add_menu_state {
        Some(AddMenuState::SelectProvider) => match key.code {
            KeyCode::Esc => {
                self.add_menu_state = None;
            }
            KeyCode::Char('1') => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::OpenAI,
                    input: String::new(),
                });
            }
            KeyCode::Char('2') => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Anthropic,
                    input: String::new(),
                });
            }
            KeyCode::Char('3') => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Xai,
                    input: String::new(),
                });
            }
            KeyCode::Char('4') => {
                self.add_menu_state = Some(AddMenuState::EnterApiKey {
                    provider: ProviderPreset::Gemini,
                    input: String::new(),
                });
            }
            KeyCode::Char('5') | KeyCode::Char('6') => {
                // SGLang/vLLM — Phase 2, show message
                self.add_menu_state = None;
                self.set_status("Local backend launching coming in Phase 2".to_string());
            }
            KeyCode::Char('7') => {
                // Custom URL — switch to command mode with `:add ` prefilled
                self.add_menu_state = None;
                self.input_mode = InputMode::Command;
                self.input_buffer = "add ".to_string();
            }
            _ => {}
        },
        Some(AddMenuState::EnterApiKey { provider, input }) => match key.code {
            KeyCode::Esc => {
                self.add_menu_state = None;
            }
            KeyCode::Enter => {
                let provider = *provider;
                let api_key = input.clone();
                self.add_menu_state = None;
                // Build WorkerSpec with preset values
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
                if let Some(AddMenuState::EnterApiKey { input, .. }) = &mut self.add_menu_state {
                    input.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(AddMenuState::EnterApiKey { input, .. }) = &mut self.add_menu_state {
                    input.push(c);
                }
            }
            _ => {}
        },
        None => {}
    }
}
```

Also add `confirm_flush: Option<(String, String)>` field to `App` (initialized as `None`), and wire flush confirmation in `handle_key()` similar to `handle_delete_confirm()`.

- [ ] **Step 3: Create action_menu.rs**

```rust
// tui/src/ui/action_menu.rs
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::theme;
use crate::app::App;
use crate::types::{ActionMenuItem, AddMenuState, ProviderPreset};

/// Render the worker action menu overlay.
pub fn render_action_menu(f: &mut Frame, app: &App) {
    let area = centered_rect(40, 30, f.area());
    f.render_widget(Clear, area);

    let state = app.state.read().unwrap();
    let worker_id = state
        .workers
        .as_ref()
        .and_then(|w| w.workers.get(app.selected_index))
        .map(|w| w.id.as_str())
        .unwrap_or("?");
    let block = Block::default()
        .title(format!(" Worker Actions: {worker_id} "))
        .title_style(theme::title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::PANEL_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items = ActionMenuItem::all();
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == app.action_menu_index {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme::text()
            };
            let prefix = if i == app.action_menu_index { "▸ " } else { "  " };
            Line::from(Span::styled(
                format!("{prefix}{}. {}", i + 1, item.label()),
                style,
            ))
        })
        .collect();

    f.render_widget(Paragraph::new(lines), inner);
}

/// Render the add worker menu overlay.
pub fn render_add_menu(f: &mut Frame, app: &App) {
    let area = centered_rect(40, 40, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Add Worker ")
        .title_style(theme::title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(theme::PANEL_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    match &app.add_menu_state {
        Some(AddMenuState::SelectProvider) => {
            let lines = vec![
                Line::from(Span::styled("1. OpenAI      (external)", theme::text())),
                Line::from(Span::styled("2. Anthropic   (external)", theme::text())),
                Line::from(Span::styled("3. xAI         (external)", theme::text())),
                Line::from(Span::styled("4. Gemini      (external)", theme::text())),
                Line::from(Span::styled("5. SGLang      (local)    [coming soon]", theme::label())),
                Line::from(Span::styled("6. vLLM        (local)    [coming soon]", theme::label())),
                Line::from(Span::styled("7. Custom URL", theme::text())),
                Line::from(Span::raw("")),
                Line::from(Span::styled("Press 1-4, 7, or Esc to cancel", theme::label())),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Some(AddMenuState::EnterApiKey { provider, input }) => {
            let masked: String = "*".repeat(input.len());
            let lines = vec![
                Line::from(Span::styled(
                    format!("Provider: {}", provider.label()),
                    theme::text(),
                )),
                Line::from(Span::styled(
                    format!("URL: {}", provider.url()),
                    theme::label(),
                )),
                Line::from(Span::raw("")),
                Line::from(Span::styled("Enter API key:", theme::text())),
                Line::from(Span::styled(
                    format!("{masked}▌"),
                    Style::default().fg(theme::ACCENT),
                )),
                Line::from(Span::raw("")),
                Line::from(Span::styled("Enter to submit │ Esc to cancel", theme::label())),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        None => {}
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vert = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vert[0])[0]
}
```

- [ ] **Step 4: Wire overlays into render()**

In `tui/src/ui/mod.rs`, add overlay rendering after existing overlays:
```rust
if app.show_action_menu {
    action_menu::render_action_menu(f, app);
}
if app.add_menu_state.is_some() {
    action_menu::render_add_menu(f, app);
}
```

- [ ] **Step 5: Register module**

Add `pub mod action_menu;` to `tui/src/ui/mod.rs`.

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p smg-tui`

- [ ] **Step 7: Commit**

```bash
git add tui/src/ui/action_menu.rs tui/src/ui/mod.rs tui/src/app.rs
git commit -m "feat(tui): add worker action menu and quick-add presets"
```

---

## Task 11: Command Mode — New Commands

**Files:**
- Modify: `tui/src/app.rs`

- [ ] **Step 1: Add new commands to execute_command()**

Extend the `execute_command()` method in `app.rs` to handle new commands. Note: `WorkerUpdateRequest` has all `Option` fields but may not derive `Default` — construct it explicitly with all fields set to `None` except the one being updated.

Add a helper:
```rust
fn selected_worker_id(&self) -> Option<String> {
    let state = self.state.read().unwrap();
    state
        .workers
        .as_ref()
        .and_then(|w| w.workers.get(self.selected_index))
        .map(|w| w.id.clone())
}

fn selected_worker_url(&self) -> Option<String> {
    let state = self.state.read().unwrap();
    state
        .workers
        .as_ref()
        .and_then(|w| w.workers.get(self.selected_index))
        .map(|w| w.url.clone())
}
```

Add command handlers inside `execute_command()`:
```rust
"priority" => {
    if let Some(val) = args.and_then(|a| a.parse::<u32>().ok()) {
        if let Some(id) = self.selected_worker_id() {
            let update = WorkerUpdateRequest {
                priority: Some(val),
                cost: None,
                labels: None,
                api_key: None,
                health: None,
            };
            match self.client.update_worker(&id, &update).await {
                Ok(_) => self.set_status(format!("Priority set to {val}")),
                Err(e) => self.set_status(format!("Error: {e}")),
            }
        }
    } else {
        self.set_status("Usage: :priority <number>".to_string());
    }
}
"cost" => {
    if let Some(val) = args.and_then(|a| a.parse::<f32>().ok()) {
        if let Some(id) = self.selected_worker_id() {
            let update = WorkerUpdateRequest {
                cost: Some(val),
                priority: None,
                labels: None,
                api_key: None,
                health: None,
            };
            match self.client.update_worker(&id, &update).await {
                Ok(_) => self.set_status(format!("Cost set to {val}")),
                Err(e) => self.set_status(format!("Error: {e}")),
            }
        }
    } else {
        self.set_status("Usage: :cost <number>".to_string());
    }
}
"flush-cache" => {
    if let Some(id) = self.selected_worker_id() {
        let url = self.selected_worker_url().unwrap_or_default();
        self.confirm_flush = Some((id, url));
    }
}
"toggle-health" => {
    if let Some(id) = self.selected_worker_id() {
        self.cmd_toggle_health(&id).await;
    }
}
"add-openai" => {
    self.add_menu_state = Some(AddMenuState::EnterApiKey {
        provider: ProviderPreset::OpenAI,
        input: String::new(),
    });
}
"add-anthropic" => {
    self.add_menu_state = Some(AddMenuState::EnterApiKey {
        provider: ProviderPreset::Anthropic,
        input: String::new(),
    });
}
"add-xai" => {
    self.add_menu_state = Some(AddMenuState::EnterApiKey {
        provider: ProviderPreset::Xai,
        input: String::new(),
    });
}
"add-gemini" => {
    self.add_menu_state = Some(AddMenuState::EnterApiKey {
        provider: ProviderPreset::Gemini,
        input: String::new(),
    });
}
```

Add `cmd_toggle_health` helper:
```rust
async fn cmd_toggle_health(&mut self, id: &str) {
    let update = WorkerUpdateRequest {
        health: Some(HealthCheckUpdate {
            disable_health_check: Some(true), // toggles — implementor should read current state and flip
            timeout_secs: None,
            check_interval_secs: None,
            success_threshold: None,
            failure_threshold: None,
        }),
        priority: None,
        cost: None,
        labels: None,
        api_key: None,
    };
    match self.client.update_worker(id, &update).await {
        Ok(_) => self.set_status("Health check toggled".to_string()),
        Err(e) => self.set_status(format!("Error: {e}")),
    }
}
```

Wire flush confirmation: in `handle_key()`, add a check for `self.confirm_flush.is_some()` similar to `confirm_delete`. On `y`, call `self.client.flush_worker_cache(&id).await`. On `n`/`Esc`, clear `self.confirm_flush`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p smg-tui`

- [ ] **Step 3: Commit**

```bash
git add tui/src/app.rs
git commit -m "feat(tui): add priority/cost/flush/health commands"
```

---

## Task 12: Models View

**Files:**
- Create: `tui/src/ui/models.rs`
- Modify: `tui/src/ui/mod.rs` (add `pub mod models;`, update render dispatch)

- [ ] **Step 1: Create models.rs**

Build a table view that reads from `state.models` (`ListModelsResponse`) and cross-references with `state.workers` to compute worker count per model.

**Table columns** (per spec):
- **ID**: model identifier (`model.id`)
- **Owner/Provider**: `model.owned_by` field
- **Worker Count**: count of workers whose `models` list contains this model ID (cross-reference with `state.workers.workers`)
- **Created**: formatted timestamp from `model.created`

Use `theme::*` for all colors:
- Header row: `theme::ACCENT` fg, `theme::PANEL_BG` bg
- Normal rows: `theme::TEXT` fg, `theme::BG` bg
- Selected row: `theme::TEXT` fg, `theme::BORDER` bg

Use `theme::panel("Models")` for the block. Support j/k navigation with `app.selected_index`.

Note: `ListModelsResponse` contains `Vec<ModelObject>` where `ModelObject` has `{ id, object, created, owned_by }`. Full model card details (capabilities, context length, etc.) are not available from the `/v1/models` endpoint — they'd require parsing from worker model data. For Phase 1, show what the API provides. If richer model data becomes available from workers' `ModelCard` info, it can be added later.

- [ ] **Step 2: Wire into render dispatch**

In `tui/src/ui/mod.rs`, replace `View::Models => render_placeholder(...)` with:
```rust
View::Models => models::render_models(f, app, chunks[2]),
```

- [ ] **Step 3: Register module**

Add `pub mod models;` to `tui/src/ui/mod.rs`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p smg-tui`

- [ ] **Step 5: Commit**

```bash
git add tui/src/ui/models.rs tui/src/ui/mod.rs
git commit -m "feat(tui): add models table view"
```

---

## Task 13: Apply Theme to Remaining UI Components

**Files:**
- Modify: `tui/src/ui/footer.rs`
- Modify: `tui/src/ui/dialog.rs`
- Modify: `tui/src/ui/filter.rs`
- Modify: `tui/src/ui/help.rs`

- [ ] **Step 1: Theme footer.rs**

Replace hardcoded colors:
- Key hints: accent color for key, muted for description
- Status: green for success, red for "Error" prefix
- Background: `theme::PANEL_BG`

- [ ] **Step 2: Theme dialog.rs**

Replace hardcoded colors:
- Block border: `theme::BORDER`
- Title: `theme::title()`
- Text: `theme::TEXT`
- Confirmation keys: `theme::ACCENT`
- Background: `theme::PANEL_BG`

- [ ] **Step 3: Theme filter.rs**

Replace hardcoded colors:
- Filter input: `theme::YELLOW` fg on `theme::BG` bg
- Command input: `theme::ACCENT` fg on `theme::BG` bg

- [ ] **Step 4: Theme help.rs**

Replace hardcoded colors:
- Block border: `theme::BORDER`
- Title: `theme::title()`
- Text: `theme::TEXT`
- Section headers: `theme::ACCENT`
- Background: `theme::PANEL_BG`

- [ ] **Step 5: Make help overlay context-aware**

Replace the static `HELP_TEXT` constant with a function `fn help_text(view: View) -> String` that returns different content based on the active view:

- **All views**: `q` quit, `1-5` tabs, `?` help, `/` filter, `:` command
- **Workers view only**: `j/k` navigate, `Enter` detail, `e` action menu, `a` add worker, `d` delete
- **Commands section**: `:priority`, `:cost`, `:flush-cache`, `:toggle-health`, `:add-openai`, `:add-anthropic`, `:add-xai`, `:add-gemini`, `:add`, `:delete`, `:quit`

Update `render_help()` to accept the current `View` and call `help_text(view)`. Update the call site in `mod.rs` to pass `app.view`.

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p smg-tui`

- [ ] **Step 7: Commit**

```bash
git add tui/src/ui/footer.rs tui/src/ui/dialog.rs tui/src/ui/filter.rs tui/src/ui/help.rs
git commit -m "feat(tui): apply Tokyo Night theme to all UI components"
```

---

## Task 14: Responsive Layout

**Files:**
- Modify: `tui/src/ui/stats_bar.rs`
- Modify: `tui/src/ui/workers.rs`
- Modify: `tui/src/ui/pulse.rs`

- [ ] **Step 1: Add responsive behavior to stats_bar.rs**

At the start of `render_stats_bar()`, check `area.width`:
- `< 80`: show only "SMG ● connected" and "W: 12" (ultra-compact)
- `80-99`: show logo + connection, workers count, load only (drop models + throughput)
- `≥ 100`: full stats bar (existing)

- [ ] **Step 2: Add responsive columns to workers.rs**

At the start of `render_workers()`, check `area.width`:
- `< 80`: show only ID, Health, Load columns
- `80-99`: show ID, URL, Health, Load columns
- `100-119`: show ID, URL, Type, Runtime, Health, Load (drop Models, Mode)
- `≥ 120`: show all columns

- [ ] **Step 3: Add responsive sparklines to pulse.rs**

- `< 80`: hide sparklines, show only left column (status panels)
- `80-99`: show both columns but use plain numbers instead of sparklines
- `≥ 100`: full layout with sparklines

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p smg-tui`

- [ ] **Step 5: Commit**

```bash
git add tui/src/ui/stats_bar.rs tui/src/ui/workers.rs tui/src/ui/pulse.rs
git commit -m "feat(tui): add responsive layout for narrow terminals"
```

---

## Task 15: Integration — Final Wiring and Cleanup

**Files:**
- Modify: `tui/src/app.rs` (ensure all key handlers are wired)
- Modify: `tui/src/ui/mod.rs` (ensure all overlays rendered)

- [ ] **Step 1: Verify all key handler dispatch is complete**

In `handle_key()`, ensure the dispatch order is:
1. `Ctrl+C` → quit
2. Delete confirmation dialog → `handle_delete_confirm()`
3. Action menu open → `handle_action_menu_key()`
4. Add menu open → `handle_add_menu_key()`
5. Input mode Filter/Command → existing handlers
6. Normal mode → `handle_normal()`

- [ ] **Step 2: Verify all overlay render order is correct**

In `render()`, ensure overlay order is (last = on top):
1. filter input
2. help overlay
3. delete dialog
4. action menu
5. add menu

- [ ] **Step 3: Full build check**

Run: `cargo build -p smg-tui`
Expected: builds successfully with no errors

- [ ] **Step 4: Run the TUI manually**

Run: `cargo run -p smg-tui -- --gateway-url http://localhost:30000`

Verify:
- Stats bar shows at top with connection status
- Tab bar shows below stats bar
- Pulse dashboard renders with 2-column layout
- Workers table renders with theme colors
- `Enter` opens detail panel, `Esc` closes it
- `e` opens action menu
- `a` opens add worker menu
- `1-5` switches tabs
- `/` opens filter, `:` opens command mode
- `?` shows help overlay
- Resize terminal to < 80 cols and verify responsive behavior

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(tui): complete Phase 1 integration and wiring"
```

---

## Summary

| Task | Description | New Files | Modified Files |
|------|-------------|-----------|----------------|
| 1 | Theme constants | `theme.rs` | `mod.rs` |
| 2 | Sparkline utility | `sparkline.rs` | `mod.rs` |
| 3 | Client updates | — | `client.rs` |
| 4 | State sparkline buffers | — | `state.rs` |
| 5 | Type definitions | — | `types.rs` |
| 6 | Stats bar + tabs | `stats_bar.rs`, `tabs.rs` | `mod.rs`, delete `header.rs` |
| 7 | Pulse redesign | — | `pulse.rs` |
| 8 | Workers theme | — | `workers.rs` |
| 9 | Detail panel | `detail.rs` | `workers.rs`, `mod.rs`, `app.rs` |
| 10 | Action menu | `action_menu.rs` | `mod.rs`, `app.rs` |
| 11 | New commands | — | `app.rs` |
| 12 | Models view | `models.rs` | `mod.rs` |
| 13 | Theme remaining UI | — | `footer.rs`, `dialog.rs`, `filter.rs`, `help.rs` |
| 14 | Responsive layout | — | `stats_bar.rs`, `workers.rs`, `pulse.rs` |
| 15 | Integration | — | `app.rs`, `mod.rs` |
