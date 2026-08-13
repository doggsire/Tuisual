use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, prelude::Rect};

use crate::app::AppState;

pub fn render(frame: &mut Frame, app: &AppState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(sections[1]);

    render_input(frame, sections[0], app);
    render_results(frame, body[0], app);
    render_info(frame, body[1], app);
    render_status(frame, sections[2], app);
}

fn render_input(frame: &mut Frame, area: Rect, app: &AppState) {
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .title(" Query ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));

    frame.render_widget(input, area);
    frame.set_cursor_position((area.x + 1 + app.cursor as u16, area.y + 1));
}

fn render_results(frame: &mut Frame, area: Rect, app: &AppState) {
    let list_items: Vec<ListItem> = if app.ranked.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            "No matching items",
            Style::default().fg(Color::DarkGray),
        )]))]
    } else {
        app.ranked
            .iter()
            .map(|ranked| {
                let item = &app.items[ranked.index];
                ListItem::new(Line::from(vec![
                    Span::styled(item.title.clone(), Style::default().fg(Color::White)),
                    Span::styled("  ", Style::default()),
                    Span::styled(item.subtitle.clone(), Style::default().fg(Color::Gray)),
                ]))
            })
            .collect()
    };

    let list = List::new(list_items)
        .block(Block::default().title(" Results ").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ");

    let mut state = ListState::default();
    if !app.ranked.is_empty() {
        state.select(Some(app.selected));
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_status(frame: &mut Frame, area: Rect, app: &AppState) {
    let text = format!(
        "{} | Rejected: {} | Enter: launch | Up/Down: select | Esc: quit",
        app.status, app.rejected_items
    );
    let status = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, area);
}

fn render_info(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default().title(" Info ").borders(Borders::ALL);

    let Some(selected_ranked) = app.ranked.get(app.selected) else {
        let empty = Paragraph::new("No selection")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    };

    let selected_item = &app.items[selected_ranked.index];

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Provider: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.provider.as_str(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.id.as_str(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.title.as_str(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Details: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.subtitle.as_str(), Style::default().fg(Color::Gray)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Summary: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                selected_item.info.summary.as_str(),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Rank: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("{} of {}", app.selected + 1, app.ranked.len()),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Score: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_ranked.score.to_string(), Style::default().fg(Color::White)),
        ]),
    ];

    if !selected_item.info.fields.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Fields:",
            Style::default().fg(Color::Cyan),
        )]));
        for field in &selected_item.info.fields {
            lines.push(Line::from(vec![
                Span::styled(format!("- {}: ", field.label), Style::default().fg(Color::Gray)),
                Span::styled(field.value.as_str(), Style::default().fg(Color::White)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Action: ", Style::default().fg(Color::Cyan)),
        Span::styled("Press Enter to launch", Style::default().fg(Color::White)),
    ]));

    let info = Paragraph::new(lines).block(block);
    frame.render_widget(info, area);
}
