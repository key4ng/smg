use std::{
    io,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use smg_tui::{
    app::App,
    client::SmgClient,
    state::{spawn_poller, GatewayState},
};

#[derive(Parser)]
#[command(
    name = "smg-tui",
    about = "Terminal dashboard for Shepherd Model Gateway"
)]
struct Cli {
    /// SMG gateway base URL.
    #[arg(long, default_value = "http://localhost:30000")]
    gateway_url: String,

    /// Prometheus / metrics endpoint URL.
    #[arg(long, default_value = "http://localhost:29000")]
    metrics_url: String,

    /// Polling interval in seconds.
    #[arg(long, default_value_t = 3)]
    poll_interval: u64,

    /// API key for authenticated endpoints.
    /// Reads from: --api-key flag, SMG_API_KEY, OPENAI_API_KEY, or ANTHROPIC_API_KEY env vars.
    #[arg(long, env = "SMG_API_KEY")]
    api_key: Option<String>,

    /// Automatically start the SMG gateway if not reachable.
    #[arg(long, default_value_t = false)]
    auto_start: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Tracing (file/stderr only — stdout is the TUI)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter("smg_tui=info")
        .init();

    // Resolve API key: --api-key > SMG_API_KEY > OPENAI_API_KEY > ANTHROPIC_API_KEY
    let api_key = cli.api_key.clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());

    let client = SmgClient::new(
        cli.gateway_url.clone(),
        cli.metrics_url.clone(),
        api_key,
    );

    // Auto-start gateway if requested and not reachable
    let _gateway_child = if cli.auto_start {
        match client.check_health().await {
            Ok(_) => {
                tracing::info!("Gateway already running at {}", cli.gateway_url);
                None
            }
            Err(_) => {
                tracing::info!(
                    "Gateway not reachable at {}, starting automatically...",
                    cli.gateway_url
                );
                let port = extract_port(&cli.gateway_url).unwrap_or(30000);
                let metrics_port = extract_port(&cli.metrics_url).unwrap_or(29000);
                // Try `smg` binary first, fall back to `cargo run -p smg`
                let launch_args = [
                    "launch",
                    "--port",
                    &port.to_string(),
                    "--prometheus-port",
                    &metrics_port.to_string(),
                ];
                let child = match tokio::process::Command::new("smg")
                    .args(&launch_args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(child) => Ok(child),
                    Err(_) => {
                        tracing::info!("'smg' not in PATH, trying 'cargo run -p smg'...");
                        let mut cargo_args = vec!["run", "-p", "smg", "--"];
                        cargo_args.extend_from_slice(&launch_args);
                        tokio::process::Command::new("cargo")
                            .args(&cargo_args)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                    }
                };
                match child {
                    Ok(child) => {
                        // Poll health endpoint until gateway is ready (up to 120s for cargo builds)
                        tracing::info!(
                            "Waiting for gateway to become ready (pid {})...",
                            child.id().unwrap_or(0)
                        );
                        let deadline = tokio::time::Instant::now()
                            + tokio::time::Duration::from_secs(120);
                        loop {
                            if tokio::time::Instant::now() >= deadline {
                                tracing::warn!("Gateway did not become ready within 120s, continuing anyway");
                                break;
                            }
                            if client.check_health().await.is_ok() {
                                tracing::info!("Gateway is ready");
                                break;
                            }
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                        Some(child)
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to start gateway: {e}. \
                             Make sure 'smg' is in your PATH (cargo install --path model_gateway)."
                        );
                        return Err(e.into());
                    }
                }
            }
        }
    } else {
        None
    };

    let state = Arc::new(RwLock::new(GatewayState::default()));

    spawn_poller(client.clone(), Arc::clone(&state), cli.poll_interval);

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook that restores the terminal
    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        panic_hook(info);
    }));

    // Run
    let mut app = App::new(state, client);
    let result = app.run(&mut terminal).await;

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // If we auto-started the gateway, kill it on exit
    if let Some(mut child) = _gateway_child {
        tracing::info!("Shutting down auto-started gateway...");
        let _ = child.kill().await;
    }

    result
}

/// Extract port from a URL like "http://localhost:30000".
fn extract_port(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}
