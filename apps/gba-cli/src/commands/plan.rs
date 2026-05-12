//! Handler for the `gba plan` command.
//!
//! Opens a TUI-based multi-turn planning conversation for the given feature
//! slug, renders a chat interface using ratatui, and finalizes the plan
//! into specs, a task list, and a git worktree.

use std::io;

use crossterm::{
    event::KeyCode,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use gba_core::{GbaContext, PlanEngine};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::info;

use crate::tui::{
    app::{App, AppState, MessageRole},
    event::{Event, EventHandler},
    ui,
};

/// Execute the plan command with a TUI interface.
///
/// Discovers the project context, applies CLI overrides, creates a
/// `PlanEngine`, launches the interactive TUI chat, and finalizes the
/// plan when the user is done.
///
/// # Errors
///
/// Returns an error if context discovery, engine initialization, the
/// planning workflow, or terminal setup/teardown fails.
pub async fn execute(
    slug: &str,
    model_override: Option<&str>,
    budget_override: Option<f64>,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut ctx = GbaContext::load(&cwd).await?;
    if !ctx.is_initialized().await {
        anyhow::bail!("Project not initialized. Run `gba init` first.");
    }

    ctx.apply_overrides(model_override, budget_override);

    let mut engine = PlanEngine::new(ctx, slug.to_owned())?;

    // Set up the terminal for TUI rendering.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui_loop(&mut terminal, &mut engine, slug).await;

    // Restore the terminal regardless of outcome.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main TUI event loop.
///
/// Drives the interactive chat between the user and the planning engine,
/// rendering after each event and handling input/response cycles.
async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: &mut PlanEngine,
    slug: &str,
) -> anyhow::Result<()> {
    let mut app = App::new(slug.to_owned());
    let mut event_handler = EventHandler::new(250);

    // Start the planning session.
    app.set_state(AppState::Waiting);
    app.push_message(
        MessageRole::System,
        "Starting planning session...".to_owned(),
    );
    terminal.draw(|f| ui::draw(f, &app))?;

    let response = engine.start().await?;
    app.push_message(MessageRole::Assistant, response);
    app.set_state(AppState::Input);

    // Event loop.
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        match event_handler.next().await? {
            Event::Key(key) => {
                if app.state() == AppState::Input && key.code == KeyCode::Enter {
                    handle_enter(&mut app, terminal, engine).await?;
                    continue;
                }
                if app.on_key(key) {
                    break;
                }
            }
            Event::Tick | Event::Resize => {
                // Redraw is handled at the top of the loop.
            }
        }
    }

    Ok(())
}

/// Handle the Enter key: finalize or send a message.
async fn handle_enter(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: &mut PlanEngine,
) -> anyhow::Result<()> {
    let input = app.take_input();
    let trimmed = input.trim();

    if trimmed.is_empty() || trimmed == "/done" {
        // Finalize the plan.
        app.set_state(AppState::Waiting);
        app.push_message(MessageRole::System, "Finalizing plan...".to_owned());
        terminal.draw(|f| ui::draw(f, app))?;

        let result = engine.finalize().await?;
        info!(
            feature_number = result.feature_number,
            slug = %result.slug,
            turns = result.turns,
            cost_usd = result.cost_usd,
            "plan finalized"
        );
        app.push_message(
            MessageRole::System,
            format!(
                "Plan complete! Feature #{:04} '{}' ({} turns, ${:.2})",
                result.feature_number, result.slug, result.turns, result.cost_usd
            ),
        );
        app.set_state(AppState::Done);
        return Ok(());
    }

    // Send user message to the engine.
    app.push_message(MessageRole::User, input.clone());
    app.set_state(AppState::Waiting);
    terminal.draw(|f| ui::draw(f, app))?;

    let response = engine.send(&input).await?;
    app.push_message(MessageRole::Assistant, response);
    app.set_state(AppState::Input);

    Ok(())
}
