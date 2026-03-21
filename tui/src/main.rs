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

    /// API key for authenticated endpoints (also reads SMG_API_KEY env var).
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

    let client = SmgClient::new(
        cli.gateway_url.clone(),
        cli.metrics_url.clone(),
        cli.api_key.clone(),
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
                let child = tokio::process::Command::new("smg")
                    .args([
                        "launch",
                        "--port",
                        &port.to_string(),
                        "--prometheus-port",
                        &metrics_port.to_string(),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match child {
                    Ok(child) => {
                        // Wait a moment for startup
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        tracing::info!("Gateway started (pid {})", child.id().unwrap_or(0));
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
