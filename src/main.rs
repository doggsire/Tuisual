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

use app::AppState;
use providers::{load_all_items, load_all_items_from_args};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }

    let load_report = if args.is_empty() {
        load_all_items()
    } else {
        load_all_items_from_args(&args)
    };
    let unknown_flag_warning = load_report
        .rejected
        .iter()
        .find(|entry| entry.contains("no providers matched requested flags"))
        .cloned();

    let mut terminal = setup_terminal()?;
    let mut app = AppState::new(load_report.items, load_report.rejected.len());
    if let Some(warning) = unknown_flag_warning {
        app.set_status(format!("Warning: {}", warning));
    }

    let run_result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    run_result
}

fn print_help() {
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

    let report = load_all_items();
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

    rows.sort_by(|a, b| a.0.cmp(&b.0));

    if rows.is_empty() {
        println!("  (no providers discovered)");
    } else {
        for (name, short_flag) in rows {
            println!("  --{:<22} {}", name, short_flag);
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, terminal::EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    if terminal::is_raw_mode_enabled()? {
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            terminal::LeaveAlternateScreen,
            event::DisableMouseCapture
        )?;
    }
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> Result<()> {
    loop {
        app.poll_action_results();
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => handle_mouse_event(terminal, app, mouse)?,
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if let Some(command) = app.take_pending_shell_command() {
            let outcome = run_shell_command_in_foreground(terminal, &command.command)?;

            // Only exit immediately on success; on failure stay in the TUI so the
            // error is visible instead of silently closing without launching anything.
            if command.exit_after && outcome.success {
                return Ok(());
            }

            show_command_result_and_wait(terminal)?;
            app.set_status(outcome.message);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_mouse_event(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    mouse: MouseEvent,
) -> Result<()> {
    if app.is_compose_mode() {
        return Ok(());
    }

    let size = terminal.size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let layout = ui::compute_layout(area);

    match mouse.kind {
        MouseEventKind::ScrollUp => {
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

struct ShellCommandOutcome {
    message: String,
    success: bool,
}

fn run_shell_command_in_foreground(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    command: &str,
) -> Result<ShellCommandOutcome> {
    suspend_terminal(terminal)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let output = Command::new(&shell).arg("-lc").arg(command).status();

    normalize_terminal_after_foreground_command()?;

    let (message, success) = match output {
        Ok(status) if status.success() => ("Action completed".to_string(), true),
        Ok(status) => (
            format!("Action failed: exit {:?} (shell: {})", status.code(), shell),
            false,
        ),
        Err(err) => (format!("Action failed to start: {}", err), false),
    };

    Ok(ShellCommandOutcome { message, success })
}

fn show_command_result_and_wait(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    println!("Press Enter to return to Tuisual...");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    resume_terminal(terminal)
}

fn normalize_terminal_after_foreground_command() -> Result<()> {
    let mut out = stdout();
    let (_, rows) = terminal::size().unwrap_or((80, 24));
    let bottom_row = rows.saturating_sub(1);

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
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture
    )?;
    terminal.clear()?;
    Ok(())
}
