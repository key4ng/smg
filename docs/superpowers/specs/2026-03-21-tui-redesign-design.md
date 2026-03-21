# SMG TUI Dashboard Redesign

## Overview

Complete redesign of the SMG TUI dashboard with a modern Tokyo Night theme, persistent stats bar, live sparkline charts, worker detail panels, GPU-aware local worker launching, and responsive layout. Targets both ops/SRE (quick health glance, incident triage) and platform engineers (worker management, capacity planning).

## Phasing

- **Phase 1**: Core redesign — layout, theme, Pulse, Workers (table + detail + actions), Models, responsive layout, sparklines
- **Phase 2**: Traffic view, Mesh view, Log tail pane, local backend launching (GPU detection + process management)

This spec covers the full design. Phase 2 features are marked as such and have known prerequisites (log streaming endpoint, GPU access).

## Layout Structure

### Top Stats Bar (always visible)
Persistent horizontal strip showing key metrics regardless of active tab:
- **Workers**: total count + healthy/unhealthy indicator
- **Models**: count of served models
- **Throughput**: aggregate tokens/sec
- **Avg Load**: percentage + mini gauge bar

### Tab Bar
Below stats bar. Tabs: `Pulse | Workers | Models | Traffic | Mesh`. Switch with keys `1-5`. Active tab highlighted with underline accent.

### Content Area
Full remaining height for active tab content.

### Footer
Two lines:
1. Context-sensitive keybindings (vary by active tab and mode)
2. Status messages (auto-clear after 5s) + "updated Ns ago" timestamp

### Disconnected State
When the gateway is unreachable (`connected: false`):
- Stats bar shows `● disconnected` in red, all metric values show `--`
- Content area shows last known data (if any) with a dimmed overlay and message: "Connection lost — retrying every Ns"
- If no data has ever been received, show: "Connecting to <gateway-url>..."
- `last_error` message displayed in footer status line

## Color Theme: Tokyo Night

| Role | Color | Hex |
|------|-------|-----|
| Background | Deep navy | #1a1b26 |
| Panel bg | Dark blue-gray | #24283b |
| Stats bar bg | Muted navy | #1e2030 |
| Border | Subtle gray | #3b4261 |
| Text primary | Light lavender | #c0caf5 |
| Text muted | Gray | #565f89 |
| Accent/links | Blue | #7aa2f7 |
| Healthy/success | Green | #9ece6a |
| Warning/medium | Yellow | #e0af68 |
| Error/critical | Red | #f7768e |
| Purple accent | Purple | #bb9af7 |

Severity-based coloring for gauges/bars:
- Green: < 50% utilization
- Yellow: 50-80% utilization
- Red: > 80% utilization

## Views

### 1. Pulse (Home Screen)

2-column layout:

**Left column — Status:**
- **Worker Health**: healthy/unhealthy counts (large numbers), by-type breakdown (regular/prefill/decode)
- **Cluster**: node name, cluster size, store health indicators (● colored dots)
- **Rate Limits**: limit/current/remaining with colored gauge bar

**Right column — Metrics:**
- **Throughput sparkline**: rolling history as Unicode block characters (▁▂▃▄▅▆▇█), with time labels
- **Token Usage by Worker**: horizontal bar per worker, color-coded by severity
- **Cache Hit Rate sparkline**: rolling history, purple accent

### 2. Workers

Full-width scrollable table with bottom-split detail panel.

**Table columns**: ID, URL, Type, Runtime (`runtime_type` field), Models, Health (● ok/err), Load (%)

**Navigation**: `j/k` or arrow keys to scroll. `/` to filter (substring match on ID/URL/type/runtime).

**Detail panel**: Press `Enter` on a worker → bottom panel slides up showing:
- Configuration: URL, runtime, connection mode, priority, cost, labels
- Load by DP rank: horizontal bars per rank (from `SchedulerLoadSnapshot`)
- Request counts: running, waiting, total (from `SchedulerLoadSnapshot`)
- Token usage: used/max (from `SchedulerLoadSnapshot`)
- Throughput sparkline (rolling history)
- Cache hit rate sparkline (rolling history)

Press `Esc` to close detail panel.

**Actions**:
- `a` — open Add Worker menu (see Worker Quick-Add below)
- `d` — delete with confirmation dialog
- `e` — open action menu on selected worker (see Worker Actions below)
- Command mode (`:`) for power users (see Command Reference below)

### 3. Models

Table of served models.

**Columns**: ID, Capabilities (tags: CHAT, VISION, TOOLS, REASONING, etc.), Context Length, Worker Count (how many workers serve it), Provider

**Detail**: `Enter` to expand showing full model card — display name, aliases, architectures, HF model type, tokenizer path, chat template, parser types, metadata.

### 4. Traffic (Phase 2)

Rate limit and request metrics dashboard.

- **Rate limit gauge**: large colored bar with limit/current/remaining numbers
- **Request counts by model**: table or bars showing per-model request volume
- **Error rates**: error count and percentage, color-coded

### 5. Mesh (Phase 2)

Cluster topology view.

- **Node list**: each node with HA status, role
- **Store health**: per-node store status (raft, kv) with colored indicators
- **Mesh health**: overall status, node count, connectivity

## Worker Actions

### Action Menu
Press `e` on a selected worker → overlay menu:

```
 Worker Actions: wk-d4e5f6
 ─────────────────────────────
 1. Update priority
 2. Update cost
 3. Update API key
 4. Flush cache
 5. Toggle health check
```

**Behavior per action:**
- **Update priority**: prompts for new value (integer). Calls `PATCH /workers/{id}` with `{"priority": N}`.
- **Update cost**: prompts for new value (float). Calls `PATCH /workers/{id}` with `{"cost": N}`.
- **Update API key**: prompts for new key (input masked with `*`). Calls `PATCH /workers/{id}` with `{"api_key": "..."}`.
- **Flush cache**: confirmation dialog → calls `POST /workers/{id}/flush_cache`. Shows result (success/failure) in status.
- **Toggle health check**: calls `PATCH /workers/{id}` with `{"health": {"disable_health_check": <toggled>}}`. Shows current state and new state in status message.

### Command Reference

All commands operate on the currently selected worker unless an ID is specified:

| Command | Action | API Call |
|---------|--------|----------|
| `:priority <N>` | Set worker priority | `PATCH /workers/{id}` with `{"priority": N}` |
| `:cost <N>` | Set worker cost | `PATCH /workers/{id}` with `{"cost": N}` |
| `:flush-cache` | Flush worker cache | `POST /workers/{id}/flush_cache` |
| `:toggle-health` | Toggle `disable_health_check` | `PATCH /workers/{id}` with `{"health": {"disable_health_check": !current}}` |
| `:delete [id]` | Delete worker | `DELETE /workers/{id}` |
| `:add <url> [flags]` | Add custom worker | `POST /workers` |
| `:add-openai` | Add OpenAI worker | Prompts for API key, then `POST /workers` |
| `:add-anthropic` | Add Anthropic worker | Prompts for API key, then `POST /workers` |
| `:add-xai` | Add xAI worker | Prompts for API key, then `POST /workers` |
| `:add-gemini` | Add Gemini worker | Prompts for API key, then `POST /workers` |
| `:add-sglang <model> [flags]` | Launch SGLang backend (Phase 2) | Spawn process + auto-register |
| `:add-vllm <model> [flags]` | Launch vLLM backend (Phase 2) | Spawn process + auto-register |
| `:quit` / `:q` | Quit TUI | — |

**Error handling**: invalid commands show red status message "Unknown command: ...". Failed API calls show "Error: <message>" in red. Successful actions show green confirmation.

### Required Client Methods (new)

These methods must be added to `SmgClient` in `client.rs`:

```rust
// Worker update (PATCH)
async fn update_worker(&self, id: &str, update: WorkerUpdateRequest) -> Result<WorkerApiResponse>
// Endpoint: PATCH /workers/{id}
// Body: JSON WorkerUpdateRequest (from openai_protocol::worker)

// Flush worker cache
async fn flush_worker_cache(&self, id: &str) -> Result<FlushCacheResult>
// Endpoint: POST /workers/{id}/flush_cache
```

## Shared Features

### Log Tail Pane (Phase 2)

Toggle with `L` on any view. Bottom pane slides up showing streaming gateway logs. Coexists with any tab content (content area shrinks to accommodate). Press `L` again or `Esc` to dismiss.

**Prerequisite**: Requires a log streaming endpoint on the gateway (SSE, websocket, or log file path). This does not exist yet. Implementation deferred to Phase 2 pending gateway-side support.

Stores last 500 lines in a `VecDeque<String>` ring buffer.

### Worker Quick-Add Menu

Press `a` on Workers view → menu appears:

```
 Add Worker
 ─────────────────────────
 1. OpenAI      (external)
 2. Anthropic   (external)
 3. xAI         (external)
 4. Gemini      (external)
 5. SGLang      (local)    [Phase 2]
 6. vLLM        (local)    [Phase 2]
 7. Custom URL
```

**External providers (1-4):**
Pre-filled URL and provider/runtime config. Prompts for API key. Calls `POST /workers` immediately.

| Preset | URL | Provider | Runtime |
|--------|-----|----------|---------|
| OpenAI | https://api.openai.com/v1 | openai | external |
| Anthropic | https://api.anthropic.com/v1 | anthropic | external |
| xAI | https://api.x.ai/v1 | xai | external |
| Gemini | https://generativelanguage.googleapis.com/v1 | gemini | external |

**Custom URL (7):** prompts for URL and optional flags (same as current `:add` command).

**Local backends (5-6) — Phase 2:**

Flow: configuration → GPU selection → launch → auto-register.

**Step 1 — Configuration prompt**: model name, TP (tensor parallelism), DP (data parallelism, SGLang only), port.
- Validates: TP × DP ≤ available idle GPU count
- Defaults: TP=1, DP=1 (SGLang) or TP=1 (vLLM), port=8000
- Note: vLLM does not support `--dp` natively. For DP with vLLM, the TUI launches multiple independent vLLM instances on different ports, each with `--tensor-parallel-size <tp>`, and registers each as a separate worker.

**Step 2 — GPU detection**: runs `nvidia-smi --query-gpu=index,name,memory.total,memory.used,memory.free,utilization.gpu --format=csv,noheader`
- If `nvidia-smi` not found → immediate error: "nvidia-smi not found — cannot launch local workers"
- If no GPUs → immediate error: "No GPUs available"
- Parse GPU list with index, name, total memory, used memory, free memory, utilization

**Step 3 — GPU selection**: two modes:
- **Auto** (default): selects only **idle GPUs** (utilization = 0% and memory used < 5% of total). Picks the first N idle GPUs needed (N = TP × DP). If not enough idle GPUs available → error: "Need N idle GPUs but only M available"
- **Manual**: interactive picker, toggle with `Space`, navigate with `j/k`. Shows per-GPU: index, name, memory total/free, utilization. Tags each GPU as `idle`, `in use`, or `low memory`. User can select non-idle GPUs manually (their choice).

**Step 4 — Launch**:
- Sets `CUDA_VISIBLE_DEVICES=<selected_gpus>` on spawned process
- SGLang: `python -m sglang.launch_server --model <model> --tp <tp> --dp <dp> --port <port>`
- vLLM: `python -m vllm.entrypoints.openai.api_server --model <model> --tensor-parallel-size <tp> --port <port>`
- Shows "launching..." with spinner in log pane
- Polls `http://localhost:<port>/health` until healthy (timeout: 5 minutes)
- Auto-registers via `POST /workers` with `runtime_type: sglang/vllm`
- Status: "worker registered: wk-abc123"
- If process crashes or health poll times out, shows error in log pane

**Process lifecycle**: spawned processes are tracked in `App.spawned_processes`. On TUI exit:
- Prompt: "N local workers are running. Kill them or leave running? [k]ill / [l]eave"
- Kill: sends SIGTERM, waits 5s, then SIGKILL if still alive
- Leave: detaches processes (they continue running independently)

**Command mode equivalents:**
- `:add-sglang <model> [--tp N] [--dp N] [--port N] [--gpus 0,1,3]`
- `:add-vllm <model> [--tp N] [--port N] [--gpus 0,1,3]`
- No `--gpus` flag = auto-select (idle GPUs only)

### Sparkline History

Ring buffer per metric. Buffer capacity = 20 data points. At the default 3-second poll interval, this provides **60 seconds** of rolling history. If the user changes `--poll-interval`, the time window scales accordingly (e.g., 1s interval = 20s window, 5s interval = 100s window).

Rendered as Unicode block characters mapped to 8 levels (▁▂▃▄▅▆▇█) scaled to min/max of the buffer.

Metrics tracked:
- Aggregate throughput (tok/s) — sum of `gen_throughput` across all workers
- Per-worker token usage ratio — from `SchedulerLoadSnapshot.token_usage`
- Per-worker cache hit rate — from `SchedulerLoadSnapshot.cache_hit_rate`
- Per-worker generation throughput — from `SchedulerLoadSnapshot.gen_throughput`

Stored in shared `GatewayState` as `VecDeque<f64>` fields.

### Client Data Model Changes

The current `LoadsResponse` in `client.rs` only deserializes `worker` (string) and `load` (isize). To support sparklines and the detail panel, the client must be updated to deserialize the full `WorkerLoadInfo` structure from the protocol crate:

```rust
// Current (insufficient):
struct WorkerLoad { worker: String, load: isize }

// Updated (matches protocol WorkerLoadInfo):
struct WorkerLoad {
    worker: String,
    worker_type: Option<String>,
    load: isize,
    details: Option<WorkerLoadResponse>,  // Contains Vec<SchedulerLoadSnapshot>
}
```

This gives access to `gen_throughput`, `cache_hit_rate`, `token_usage`, `num_running_reqs`, `num_waiting_reqs`, `num_used_tokens`, `max_total_num_tokens`, and `utilization` per DP rank.

### Responsive Layout

Adapt based on `frame.area().width`:

| Width | Behavior |
|-------|----------|
| ≥ 120 | Full layout — all columns, all sparklines |
| 100-119 | Hide lower-priority table columns (Mode, Labels) |
| 80-99 | Collapse stats bar to single compact row, hide sparklines |
| < 80 | Minimal mode — essential data only (health, worker list) |

### Confirmation Dialogs

Destructive actions (delete worker, flush cache) show centered overlay dialog:

```
 ┌─ Confirm Delete ──────────────────┐
 │ Delete worker wk-d4e5f6?          │
 │ URL: http://gpu-02:8000           │
 │                                    │
 │         [y] Yes  [n] No           │
 └────────────────────────────────────┘
```

### Help Overlay

`?` toggles full-screen help modal. Context-aware: shows keybindings relevant to current view and mode. Lists all commands available in command mode.

## Architecture

### File Structure

```
tui/src/
├── main.rs          # Entry point, terminal setup, CLI args
├── app.rs           # App state, key handling, input modes
├── client.rs        # HTTP client for SMG API (updated with new methods)
├── event.rs         # Event handler (keyboard, resize, tick)
├── state.rs         # Shared gateway state + sparkline buffers
├── types.rs         # View enum, InputMode enum
├── gpu.rs           # GPU detection via nvidia-smi (Phase 2)
├── launcher.rs      # Local backend process spawning + health polling (Phase 2)
├── lib.rs           # Module exports
└── ui/
    ├── mod.rs       # Main render dispatcher
    ├── stats_bar.rs # Persistent top stats strip (replaces old header.rs)
    ├── tabs.rs      # Tab bar rendering (replaces old header.rs)
    ├── footer.rs    # Keybindings + status messages
    ├── pulse.rs     # Pulse dashboard (2-column)
    ├── workers.rs   # Workers table
    ├── detail.rs    # Worker detail bottom panel
    ├── models.rs    # Models table + detail
    ├── traffic.rs   # Traffic/rate limit view (Phase 2)
    ├── mesh.rs      # Mesh/cluster topology view (Phase 2)
    ├── logs.rs      # Log tail bottom pane (Phase 2)
    ├── dialog.rs    # Confirmation dialogs
    ├── action_menu.rs # Worker action menu + add menu + GPU picker
    ├── filter.rs    # Filter/command input overlay
    ├── help.rs      # Help modal
    └── sparkline.rs # Sparkline rendering utility
```

Note: the existing `header.rs` is replaced by `stats_bar.rs` + `tabs.rs`. The old header combined logo, tabs, and connection status into one component. The new design separates the persistent stats bar (always visible, data-driven) from the tab navigation bar.

### State Changes

**New fields in `GatewayState`:**
- `throughput_history: VecDeque<f64>` — rolling aggregate throughput (capacity: 20)
- `cache_hit_history: VecDeque<f64>` — rolling aggregate cache hit rate (capacity: 20)
- `per_worker_throughput: HashMap<String, VecDeque<f64>>` — per-worker throughput history
- `per_worker_cache_hit: HashMap<String, VecDeque<f64>>` — per-worker cache hit history

**New fields in `App`:**
- `show_detail: bool` — worker detail panel visibility
- `show_logs: bool` — log pane visibility (Phase 2)
- `show_action_menu: bool` — action menu visibility
- `action_menu_index: usize` — selected action in menu
- `add_menu_state: Option<AddMenuState>` — state machine for add worker flow (provider select → API key prompt → submit, or config → GPU pick → launch for Phase 2)
- `gpu_list: Vec<GpuInfo>` — detected GPUs (Phase 2)
- `gpu_selection: Vec<bool>` — per-GPU toggle state (Phase 2)
- `gpu_auto_select: bool` — auto vs manual GPU selection (Phase 2)
- `spawned_processes: Vec<SpawnedWorker>` — tracked child processes (Phase 2)

### New Modules

**`gpu.rs` (Phase 2):**
- `detect_gpus() -> Result<Vec<GpuInfo>>` — runs nvidia-smi, parses CSV output
- `GpuInfo { index: u32, name: String, memory_total_mb: u64, memory_used_mb: u64, memory_free_mb: u64, utilization_pct: u32 }`
- `is_idle(&self) -> bool` — returns true if utilization = 0% and memory used < 5% of total
- `auto_select_gpus(gpus: &[GpuInfo], count: usize) -> Result<Vec<u32>>` — picks first N idle GPUs, errors if not enough idle

**`launcher.rs` (Phase 2):**
- `LaunchConfig { model: String, runtime: Runtime (SGLang/VLlm), tp: u32, dp: u32, port: u16, gpus: Vec<u32> }`
- `spawn_backend(config: LaunchConfig) -> Result<SpawnedWorker>` — spawns process with CUDA_VISIBLE_DEVICES
- `SpawnedWorker { child: tokio::process::Child, port: u16, config: LaunchConfig }`
- Background task: poll health endpoint, auto-register on success, report error on failure
- `cleanup_processes(processes: &mut Vec<SpawnedWorker>, kill: bool)` — SIGTERM/SIGKILL or detach

### Dependencies

No new crate dependencies expected. Uses existing:
- `tokio::process::Command` for spawning backends (Phase 2)
- `ratatui` widgets for sparklines (built-in `Sparkline` widget or custom Unicode renderer)
- Existing `reqwest` client for API calls

## CLI Options

Existing (unchanged):
- `--gateway-url` (default: http://localhost:30000)
- `--metrics-url` (default: http://localhost:29000)
- `--poll-interval` (default: 3 seconds)
- `--api-key` (env: SMG_API_KEY)

No new CLI options needed.
