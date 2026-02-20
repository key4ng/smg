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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Tracing (file/stderr only — stdout is the TUI)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter("smg_tui=info")
        .init();

    let client = SmgClient::new(cli.gateway_url, cli.metrics_url, cli.api_key);
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

    result
}
