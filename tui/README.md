# smg-tui

Terminal dashboard for [Shepherd Model Gateway](../README.md).

Connects to a running SMG instance via its REST APIs and provides real-time monitoring of workers, circuit breakers, throughput, and an interactive chat playground.

## Install

```bash
cargo build -p smg-tui --release
# Binary at: target/release/smg-tui
```

## Usage

```bash
# Connect to a running gateway
smg-tui --gateway-url http://localhost:30000

# Auto-start the gateway if not running
smg-tui --auto-start

# With API key for authenticated endpoints
OPENAI_API_KEY=sk-... smg-tui
```

### CLI Options

| Flag               | Default                    | Env Var                                      | Description                  |
|--------------------|----------------------------|----------------------------------------------|------------------------------|
| `--gateway-url`    | `http://localhost:30000`   |                                              | SMG gateway base URL         |
| `--metrics-url`    | `http://localhost:29000`   |                                              | Prometheus endpoint URL      |
| `--poll-interval`  | `3`                        |                                              | Polling interval (seconds)   |
| `--api-key`        |                            | `SMG_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` | API key for auth             |
| `--auto-start`     | `false`                    |                                              | Start gateway if not reachable |

### Auto-Start

When `--auto-start` is passed and the gateway is not reachable:

1. Tries `smg launch` binary first, falls back to `cargo run -p smg`
2. Polls the health endpoint until the gateway is ready (up to 120s)
3. Kills the gateway process on TUI exit

## Views

Switch views with number keys `1`-`5`.

| Key | View      | Description                                              |
|-----|-----------|----------------------------------------------------------|
| `1` | Pulse     | Dashboard: worker health, GPU status, throughput sparklines, rate limits |
| `2` | Workers   | Scrollable table with health, load, and detail panel     |
| `3` | Chat      | Interactive chat with streaming responses                |
| `4` | Traffic   | *(coming soon)*                                          |
| `5` | Mesh      | *(coming soon)*                                          |

### Stats Bar

The top header shows four cards:

| Card             | Description                                      |
|------------------|--------------------------------------------------|
| WORKERS          | Total worker count + health status               |
| CIRCUIT BREAKERS | Active breaker count + open/closed status         |
| THROUGHPUT       | tok/s (local workers) or req/s (external workers) |
| AVG LOAD         | Average worker load with gauge bar               |

### Pulse View

Two-column dashboard with adaptive panels:

- **Worker Health** — per-worker status with colored dots
- **GPUs** — GPU utilization, memory, temperature (when nvidia-smi available)
- **Rate Limits** — current usage with gauge bar
- **Throughput** — sparkline over 60s window
- **Token Usage** — per-worker usage bars (local workers)
- **Cache Hit Rate** — sparkline over 60s window

Panels auto-hide when data is unavailable (e.g., no GPUs, no mesh).

### Workers View

| Key       | Action                              |
|-----------|-------------------------------------|
| `j`/`Down`| Move selection down                 |
| `k`/`Up`  | Move selection up                   |
| `Enter`   | Toggle detail panel                 |
| `a`       | Add worker (quick-add presets)      |
| `d`       | Delete selected worker              |
| `e`       | Edit worker (action menu)           |
| `/`       | Filter workers                      |

The detail panel (toggled with Enter) shows:
- Worker configuration (URL, runtime, health, type)
- Models served by the worker
- Load by DP rank with gauge bars
- Per-worker throughput and cache hit sparklines

### Chat View

Interactive chat with models through the SMG gateway.

| Key          | Action                    |
|--------------|---------------------------|
| `Enter`      | Send message              |
| `Tab`        | Cycle model (gpt-5.4 family) |
| `Shift+Tab`  | Cycle endpoint (chat/responses) |
| `Esc`        | Cancel streaming / clear input |
| `Up`/`Down`  | Scroll conversation       |

Features:
- Streaming responses with live cursor
- Markdown rendering (bold, italic, code, headings, bullets, code blocks)
- Multi-turn conversation
  - **Chat completions** (`/v1/chat`): sends full conversation history
  - **Responses API** (`/v1/responses`): uses `previous_response_id` for efficient multi-turn
- Title bar shows current model, endpoint, and multi-turn mode

### Worker Actions

Quick-add presets for external providers:
1. OpenAI (`https://api.openai.com/v1`)
2. Anthropic (`https://api.anthropic.com/v1`)
3. xAI (`https://api.x.ai/v1`)
4. Gemini (`https://generativelanguage.googleapis.com/v1`)

Action menu (`e` key):
- Update priority / cost / API key
- Flush cache
- Toggle health check

### Commands

Enter command mode with `:`, then type:

```
:add <url> [--provider <p>] [--runtime <r>]
:delete <id>
:priority <number>
:cost <number>
:flush-cache
:toggle-health
:quit
```

## Key Bindings

| Key             | Action                    |
|-----------------|---------------------------|
| `1`-`5`         | Switch view               |
| `j` / `Down`    | Move selection down       |
| `k` / `Up`      | Move selection up         |
| `q` / `Ctrl+C`  | Quit                      |
| `?`             | Toggle help overlay       |
| `/`             | Filter mode               |
| `:`             | Command mode              |
| `Esc`           | Close overlay/clear filter|
