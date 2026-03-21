use std::io;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use lazyllm::app::App;
use lazyllm::config::{default_config_path, load_config};
use lazyllm::event::keybindings::resolve_key;
use lazyllm::event::types::{Action, AppEvent};
use lazyllm::llm;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Setup logging to file (avoid polluting TUI)
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("lazyllm")
        .join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let log_file = std::fs::File::create(log_dir.join("lazyllm.log"))?;
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_env_filter("lazyllm=debug")
        .init();

    tracing::info!("lazyllm starting");

    // Load config
    let config_path = default_config_path();
    let config = load_config(&config_path)?;
    tracing::info!("Config loaded from {}", config_path.display());

    // Register LLM providers from config
    let registry = llm::build_registry(&config);

    // Initialize conversation store (SQLite)
    let data_dir = &config.general.data_dir;
    let store = lazyllm::store::sqlite_store::SqliteStore::new(data_dir)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to initialize store: {e}"))?;
    tracing::info!("Store initialized at {}", data_dir.display());

    // Initialize app
    let mut app = App::new(config, registry).with_store(store);

    // Initialize MCP servers
    app.init_mcp().await;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Event loop
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _event_handle = lazyllm::event::spawn_event_loop(tx, Duration::from_millis(33));

    // Main loop
    while app.is_running() {
        terminal.draw(|frame| lazyllm::ui::render(&app, frame))?;

        if let Some(event) = rx.recv().await {
            let action = match event {
                AppEvent::Key(key) => resolve_key(key, app.mode(), app.focus()),
                AppEvent::Resize(w, h) => Action::Resize(w, h),
                AppEvent::Tick => Action::Tick,
            };

            app.update(action).await;
        }
    }

    // Shut down MCP servers
    app.shutdown_mcp().await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    tracing::info!("lazyllm exiting");
    Ok(())
}
