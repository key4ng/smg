# smg-tui

Terminal dashboard for [Shepherd Model Gateway](../README.md), inspired by k9s.

Connects to a running SMG instance via its REST APIs and provides real-time monitoring of workers, cluster state, and traffic.

## Install

```bash
cargo build -p smg-tui --release
# Binary at: target/release/smg-tui
```

## Usage

```bash
smg-tui --gateway-url http://localhost:30000
```

### CLI Options

| Flag               | Default                    | Env Var       | Description                  |
|--------------------|----------------------------|---------------|------------------------------|
| `--gateway-url`    | `http://localhost:30000`   |               | SMG gateway base URL         |
| `--metrics-url`    | `http://localhost:29000`   |               | Prometheus endpoint URL      |
| `--poll-interval`  | `3`                        |               | Polling interval (seconds)   |
| `--api-key`        |                            | `SMG_API_KEY` | API key for auth'd endpoints |

## Views

Switch views with number keys `1`-`5`.

| Key | View      | Description                                              |
|-----|-----------|----------------------------------------------------------|
| `1` | Pulse     | Dashboard: worker summary, cluster info, rate limits, load chart |
| `2` | Workers   | Scrollable table of all workers with health and load     |
| `3` | Models    | *(coming soon)*                                          |
| `4` | Traffic   | *(coming soon)*                                          |
| `5` | Mesh      | *(coming soon)*                                          |

## Key Bindings

### Navigation

| Key          | Action                    |
|--------------|---------------------------|
| `1`-`5`      | Switch view               |
| `j` / `Down` | Move selection down       |
| `k` / `Up`   | Move selection up         |
| `q` / `Ctrl+C` | Quit                   |
| `?`          | Toggle help overlay       |
| `Esc`        | Close overlay/clear filter|

### Workers View

| Key | Action                              |
|-----|-------------------------------------|
| `a` | Add worker (opens command mode)     |
| `d` | Delete selected worker (with confirmation) |
| `/` | Filter workers (substring match)    |
| `:` | Enter command mode                  |

### Commands

Enter command mode with `:`, then type:

```
:add <url> [--provider <p>] [--runtime <r>]
:delete <id>
:quit
```

**Providers:** `openai`, `anthropic`, `gemini`, `xai`
**Runtimes:** `sglang` (default), `vllm`, `trtllm`, `external`

Specifying `--provider` automatically sets `--runtime external`.

#### Examples

```
:add http://api.openai.com --provider openai
:add http://api.anthropic.com -p anthropic
:add http://localhost:8000 -r vllm
:add http://localhost:8001
:delete abc123
```

## Screenshots

```
┌─ SMG ──────────────────────────────────────────── ● connected ┐
│  1:Pulse  2:Workers  3:Models  4:Traffic  5:Mesh              │
├───────────────────────────┬───────────────────────────────────┤
│  Workers                  │  Cluster                          │
│  Total:    4              │  Node:     node-1                 │
│  Healthy:  3              │  Size:     3                      │
│  Load:     12             │                                   │
│                           │  Stores:                          │
│  Regular:  2  Prefill: 1  │    ● workers                     │
│  Decode: 1                │    ● rate_limits                  │
├───────────────────────────┼───────────────────────────────────┤
│  Rate Limiting            │  Worker Loads                     │
│  Limit:     1000          │  worker-1  ████░░░░░░  4          │
│  Current:   250           │  worker-2  ██████░░░░  6          │
│  Remaining: 750           │  worker-3  ██░░░░░░░░  2          │
│                           │                                   │
│  ████████░░░░░░░░░░ 25%   │                                   │
└───────────────────────────┴───────────────────────────────────┘
 q quit  1-5 view  / filter  : cmd  ? help
```
