mod app;
mod config;
mod event;
mod llm;
mod markdown;
mod store;
mod ui;

use std::io;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use app::App;
use config::{default_config_path, load_config};
use event::keybindings::resolve_key;
use event::types::{Action, AppEvent};
use llm::ProviderRegistry;
use llm::anthropic::AnthropicProvider;
use llm::google::GoogleProvider;
use llm::ollama::OllamaProvider;
use llm::openai::OpenAiProvider;
use llm::types::ModelInfo;

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
    let mut registry = ProviderRegistry::new();
    for (name, provider_config) in &config.providers {
        let models: Vec<ModelInfo> = provider_config
            .models
            .iter()
            .map(|m| ModelInfo::new(m))
            .collect();

        match provider_config.provider_type.as_str() {
            "ollama" => {
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("http://localhost:11434");
                let provider = OllamaProvider::new(name, base_url, models);
                registry.register(Box::new(provider));
                tracing::info!("Registered Ollama provider: {name}");
            }
            "anthropic" => {
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://api.anthropic.com/v1/messages");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = AnthropicProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered Anthropic provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
            "google" => {
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://generativelanguage.googleapis.com/v1beta");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = GoogleProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered Google provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
            _ => {
                // Default: OpenAI-compatible
                let api_key_env = provider_config.api_key_env.as_deref().unwrap_or("");
                let base_url = provider_config
                    .base_url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1");
                if let Ok(api_key) = std::env::var(api_key_env) {
                    let provider = OpenAiProvider::new(name, api_key, base_url, models);
                    registry.register(Box::new(provider));
                    tracing::info!("Registered OpenAI provider: {name}");
                } else {
                    tracing::warn!("Skipping provider {name}: {api_key_env} not set");
                }
            }
        }
    }

    // Initialize conversation store
    let data_dir = &config.general.data_dir;
    let store = store::json_store::JsonStore::new(data_dir)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to initialize store: {e}"))?;
    tracing::info!("Store initialized at {}", data_dir.display());

    // Initialize app
    let mut app = App::new(config, registry).with_store(store);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Event loop
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _event_handle = event::spawn_event_loop(tx, Duration::from_millis(250));

    // Main loop
    while app.running {
        terminal.draw(|frame| ui::render(&app, frame))?;

        if let Some(event) = rx.recv().await {
            let action = match event {
                AppEvent::Key(key) => resolve_key(key, app.mode, app.focus),
                AppEvent::Resize(w, h) => Action::Resize(w, h),
                AppEvent::Tick => Action::Tick,
            };

            app.update(action).await;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    tracing::info!("lazyllm exiting");
    Ok(())
}
