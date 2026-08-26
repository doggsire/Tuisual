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
    let load_report = if args.is_empty() {
        load_all_items()
    } else {
        load_all_items_from_args(&args)
    };

    let mut terminal = setup_terminal()?;
    let mut app = AppState::new(load_report.items, load_report.rejected.len());

    let run_result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    run_result
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
            let message = run_shell_command_in_foreground(
                terminal,
                &command.command,
                !command.exit_after,
            )?;

            if command.exit_after {
                return Ok(());
            }

            app.set_status(message);
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
            if let Some(row) = layout.results_row_at(mouse.column, mouse.row) {
                if let Some(index) = app.result_index_for_row(row, layout.results_viewport_height()) {
                    app.click_result_index(index);
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn run_shell_command_in_foreground(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    command: &str,
    return_to_tui: bool,
) -> Result<String> {
    suspend_terminal(terminal)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let output = Command::new(&shell).arg("-lc").arg(command).status();

    normalize_terminal_after_foreground_command()?;

    if return_to_tui {
        println!("Press Enter to return to Tuisual...");
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);

        resume_terminal(terminal)?;
    }

    let message = match output {
        Ok(status) if status.success() => "Action completed".to_string(),
        Ok(status) => format!("Action failed: exit {:?} (shell: {})", status.code(), shell),
        Err(err) => format!("Action failed to start: {}", err),
    };

    Ok(message)
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
