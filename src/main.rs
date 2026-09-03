// This file is the "starter" of the app.
// It reads command-line arguments, decides what mode the app should run in,
// sets up the terminal screen, and keeps the main loop alive.
mod app;
mod matcher;
mod models;
mod path_discovery;
mod providers;
mod ui;

use std::io::{self, Write, stdout};
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute, style, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde::Serialize;

use app::AppState;
use providers::{load_all_items, load_all_items_from_args};

// A tiny helper shape used when we want to print all discovered items in JSON.
#[derive(Serialize)]
struct JsonCatalog {
    items: Vec<models::AppItem>,
    rejected: Vec<String>,
}

// This is the very first thing that runs when the program starts.
// It decides whether to show the UI, show help, or print JSON data.
fn main() -> Result<()> {
    // Read everything the user typed after the program name.
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // Remove the path-flags option before provider loading sees the remaining arguments.
    // This mode prints discovered sub-items and exits instead of starting the TUI.
    // Special command: "tuisual --path-flags git" asks for all the flag choices
    // that can be discovered for a command like git.
    let path_flags_query = take_option(&mut args, "--path-flags");
    if let Some(command_name) = path_flags_query {
        // Convert the command name into submenu data, serialize it as JSON, and stop.
        let sub_items = path_discovery::discover_path_sub_items(&command_name);
        println!("{}", serde_json::to_string(&sub_items)?);
        return Ok(());
    }

    // Remove app-owned options so only provider arguments remain in `args`.
    let json_mode = take_flag(&mut args, "--json");
    let provider_query = take_option(&mut args, "--query");

    if let Some(query) = provider_query {
        // Providers read this environment variable instead of receiving a second
        // custom argument. Set it before invoking either JSON or TUI loading.
        unsafe {
            std::env::set_var("TUISUAL_PROVIDER_QUERY", query);
        }
    }

    if json_mode {
        // In JSON mode, load either the whole catalog or the requested providers.
        let report = if args.is_empty() {
            load_all_items()
        } else {
            load_all_items_from_args(&args)
        };
        // Serialize both successful items and rejection messages so scripts can inspect both.
        println!(
            "{}",
            serde_json::to_string(&JsonCatalog {
                items: report.items,
                rejected: report.rejected,
            })?
        );
        return Ok(());
    }

    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        // Help is checked after app-owned options are removed, then exits without terminal setup.
        print_help();
        return Ok(());
    }

    // Normal mode needs the same load decision, but keeps the report in memory for the TUI.
    let load_report = if args.is_empty() {
        load_all_items()
    } else {
        load_all_items_from_args(&args)
    };
    // Preserve the useful unknown-provider message before moving the report's items into AppState.
    let unknown_flag_warning = load_report
        .rejected
        .iter()
        .find(|entry| entry.contains("no providers matched requested flags"))
        .cloned();

    // Enter alternate-screen/raw terminal mode only after loading has succeeded.
    let mut terminal = setup_terminal()?;
    let mut app = AppState::new(load_report.items, load_report.rejected.len());
    if let Some(warning) = unknown_flag_warning {
        app.set_status(format!("Warning: {}", warning));
    }

    // Always restore terminal state after the event loop returns, even when the loop reports an error.
    let run_result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    run_result
}

// Remove a flag like "--json" from the argument list if it is present.
// Returns true if it was there, false otherwise.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    // Find the first exact argument. `position` returns its vector index, if present.
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        // Removing shifts later arguments left and leaves provider arguments intact.
        args.remove(index);
        true
    } else {
        false
    }
}

// Remove an option and its value from the argument list.
// Example: "--query hello" becomes "Some("hello")".
//
// This is how the app eats command-line flags that it knows about, while leaving the rest
// of the arguments available for provider loading. It is a little bit like sorting the user's
// typed instructions and separating the ones meant for the app itself from the ones meant for
// the providers.
fn take_option(args: &mut Vec<String>, option: &str) -> Option<String> {
    // Look for the option name. `?` returns None when the option was not supplied.
    let index = args.iter().position(|arg| arg == option)?;
    // Remove the option first; its value now occupies the same index.
    args.remove(index);
    if index < args.len() {
        // Remove and return the following argument as the option's value.
        Some(args.remove(index))
    } else {
        // A bare option has no value, so report that to the caller.
        None
    }
}

// Show the friendly information page for the user.
fn print_help() {
    // Print the fixed usage text before discovering provider names dynamically.
    println!("Tuisual");
    println!("Terminal launcher with provider-based item discovery.");
    println!();
    println!("Usage:");
    println!("  tuisual                Show provider catalog");
    println!("  tuisual [flags]        Load items from matching providers");
    println!("  tuisual -h, --help     Show this help page");
    println!();
    println!("Examples:");
    println!("  tuisual -P");
    println!("  tuisual --path-launcher");
    println!();
    println!("Discovered providers:");

    // The catalog loader supplies the provider names and their advertised short flags.
    let report = load_all_items();
    // Keep only catalog items, then turn each item into the two strings needed for one help row.
    let mut rows: Vec<(String, String)> = report
        .items
        .into_iter()
        .filter(|item| item.provider == "catalog")
        .map(|item| {
            let short_flag = item
                .info
                .fields
                .iter()
                .find(|field| field.label == "Short Flag")
                .map(|field| field.value.clone())
                .unwrap_or_else(|| "(none)".to_string());
            (item.title, short_flag)
        })
        .collect();

    // Sort by provider name so help output is stable regardless of filesystem order.
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    if rows.is_empty() {
        println!("  (no providers discovered)");
    } else {
        // Format each discovered provider as a long flag plus its short-flag display value.
        for (name, short_flag) in rows {
            println!("  --{:<22} {}", name, short_flag);
        }
    }
}

// The app wants to control the whole terminal screen, so it switches into a
// special mode where it can draw its own interface and catch key and mouse events.
fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    // Raw mode sends individual keys immediately instead of waiting for Enter.
    enable_raw_mode()?;
    let mut out = stdout();
    // Alternate-screen mode gives the TUI a temporary screen, and mouse capture makes
    // mouse events available to the application.
    execute!(out, terminal::EnterAlternateScreen, event::EnableMouseCapture)?;
    // Wrap stdout in Ratatui's Crossterm backend and create the terminal abstraction.
    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

// Put the terminal back to its normal mode when the app is done.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    // Only undo raw mode and alternate-screen state if setup actually enabled raw mode.
    if terminal::is_raw_mode_enabled()? {
        disable_raw_mode()?;
        // Leave the temporary screen and release mouse capture before returning to the shell.
        execute!(
            terminal.backend_mut(),
            terminal::LeaveAlternateScreen,
            event::DisableMouseCapture
        )?;
    }
    // The cursor may have been hidden by the TUI, so explicitly make it visible again.
    terminal.show_cursor()?;
    Ok(())
}

// This is the heart of the UI loop.
// It keeps redrawing the screen and reacts to keyboard and mouse input.
//
// The loop is very simple conceptually:
// 1. ask the app if there is background work to finish
// 2. redraw the UI for the user
// 3. check if the user pressed a key or moved the mouse
// 4. if a command should be run, launch it and show the result
// 5. quit if the user asked to exit
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> Result<()> {
    loop {
        // Before each screen draw, check for any background work that should finish.
        // Process delayed provider work before drawing so the frame reflects fresh state.
        app.poll_action_results();
        // Ratatui redraws the whole current frame from AppState.
        terminal.draw(|frame| ui::render(frame, app))?;

        // Wait briefly for input. The timeout also lets the loop poll delayed work repeatedly.
        if event::poll(Duration::from_millis(50))? {
            // Read one event and route it to the matching AppState or mouse handler.
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => handle_mouse_event(terminal, app, mouse)?,
                // A later draw uses the new terminal size, so no state change is needed here.
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // Actions are queued by AppState and executed outside the drawing code.
        if let Some(command) = app.take_pending_shell_command() {
            let message = run_shell_command_in_foreground(
                terminal,
                &command.command,
                !command.exit_after,
            )?;

            // Exit-after commands finish the process; other commands return to the TUI
            // with their success or failure message in the status bar.
            if command.exit_after {
                return Ok(());
            }

            app.set_status(message);
        }

        // Check this after processing the current event and any queued command.
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

// Mouse clicks are handled like a simple map:
// if the user clicks in the results area, move the highlighted item.
// If they click the info panel, scroll the info text.
fn handle_mouse_event(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    mouse: MouseEvent,
) -> Result<()> {
    // Compose input owns the keyboard interaction, so mouse actions are ignored there.
    if app.is_compose_mode() {
        return Ok(());
    }

    // Recalculate layout from the terminal's current dimensions because a resize may have happened.
    let size = terminal.size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let layout = ui::compute_layout(area);

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            // Scroll the results list when the pointer is over it; otherwise scroll info text.
            if layout.results_row_at(mouse.column, mouse.row).is_some() {
                app.move_selection_by(-1);
            } else if layout.info_contains(mouse.column, mouse.row) {
                app.scroll_info_by(-2);
            }
        }
        MouseEventKind::ScrollDown => {
            if layout.results_row_at(mouse.column, mouse.row).is_some() {
                app.move_selection_by(1);
            } else if layout.info_contains(mouse.column, mouse.row) {
                app.scroll_info_by(2);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Convert the clicked screen row into a ranked-list index before selecting it.
            if let Some(row) = layout.results_row_at(mouse.column, mouse.row)
                && let Some(index) = app.result_index_for_row(row, layout.results_viewport_height())
            {
                app.click_result_index(index);
            }
        }
        _ => {}
    }

    Ok(())
}

// This is the wrapper that runs a command outside the TUI temporarily.
// It is a bit like pausing the game, doing the real action in the normal terminal, and then
// putting the app back to its drawing state. This is how a launched command can run in the real shell
// without the app losing its own screen layout.
fn run_shell_command_in_foreground(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    command: &str,
    return_to_tui: bool,
) -> Result<String> {
    // Temporarily give the real shell control of the terminal so command output is normal.
    suspend_terminal(terminal)?;

    // Use the user's shell when available, falling back to `sh`.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    // `-lc` lets the shell interpret the complete command string, including pipes and redirects.
    let output = Command::new(&shell).arg("-lc").arg(command).status();

    // Remove leftover control state and place the cursor after the command's output.
    normalize_terminal_after_foreground_command()?;

    if return_to_tui {
        // Keep the output visible until the user confirms they are ready to return.
        println!("Press Enter to return to Tuisual...");
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);

        // Re-enter the alternate screen and clear it before the next TUI frame.
        resume_terminal(terminal)?;
    }

    // Turn the process result into a short status message for AppState.
    let message = match output {
        Ok(status) if status.success() => "Action completed".to_string(),
        Ok(status) => format!("Action failed: exit {:?} (shell: {})", status.code(), shell),
        Err(err) => format!("Action failed to start: {}", err),
    };

    Ok(message)
}

fn normalize_terminal_after_foreground_command() -> Result<()> {
    // Use a fresh stdout handle because the TUI backend is temporarily suspended.
    let mut out = stdout();
    // Move to the last row without exceeding the terminal's valid row range.
    let (_, rows) = terminal::size().unwrap_or((80, 24));
    let bottom_row = rows.saturating_sub(1);

    // Reset colors, show the cursor, clear the current line, and move output to a clean line.
    execute!(
        out,
        style::ResetColor,
        cursor::Show,
        cursor::MoveTo(0, bottom_row),
        terminal::Clear(terminal::ClearType::CurrentLine)
    )?;
    writeln!(out)?;
    out.flush()?;

    Ok(())
}

fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    // Undo the two terminal features enabled by setup before running a foreground command.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        terminal::LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    // Restore raw input, alternate-screen drawing, and mouse capture after the command ends.
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture
    )?;
    // Discard the old frame so the next draw starts with a clean screen.
    terminal.clear()?;
    Ok(())
}
