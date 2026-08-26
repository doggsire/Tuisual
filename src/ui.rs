use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, prelude::Rect};

use crate::app::AppState;
use crate::models::ItemAction;

#[derive(Debug, Clone, Copy)]
pub struct UiLayout {
    pub input: Rect,
    pub results: Rect,
    pub info: Rect,
    pub status: Rect,
}

impl UiLayout {
    pub fn results_viewport_height(&self) -> usize {
        self.results.height.saturating_sub(2) as usize
    }

    pub fn info_contains(&self, column: u16, row: u16) -> bool {
        point_in_rect(self.info, column, row)
    }

    pub fn results_row_at(&self, column: u16, row: u16) -> Option<usize> {
        if !point_in_rect(self.results, column, row) {
            return None;
        }

        let inner_top = self.results.y.saturating_add(1);
        let inner_bottom = self
            .results
            .y
            .saturating_add(self.results.height.saturating_sub(1));

        if row < inner_top || row >= inner_bottom {
            return None;
        }

        Some(row.saturating_sub(inner_top) as usize)
    }
}

pub fn compute_layout(area: Rect) -> UiLayout {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(sections[1]);

    UiLayout {
        input: sections[0],
        results: body[0],
        info: body[1],
        status: sections[2],
    }
}

fn point_in_rect(rect: Rect, x: u16, y: u16) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    x >= rect.x && x < right && y >= rect.y && y < bottom
}

fn info_inner_width(area: Rect) -> usize {
    area.width.saturating_sub(2) as usize
}

fn wrap_words_with_prefix(text: &str, first_prefix: &str, next_prefix: &str, width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![first_prefix.to_string()];
    }

    let mut output = Vec::new();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let mut index = 0usize;
    let mut carry: Option<String> = None;
    let mut first_line = true;

    while index < words.len() || carry.is_some() {
        let prefix = if first_line { first_prefix } else { next_prefix };
        let available = width.saturating_sub(prefix.chars().count()).max(1);
        let mut line = String::new();
        let mut emitted_chunk_line = false;

        while index < words.len() || carry.is_some() {
            let word = if let Some(value) = carry.take() {
                value
            } else {
                let value = words[index].to_string();
                index += 1;
                value
            };

            if line.is_empty() {
                if word.chars().count() <= available {
                    line.push_str(&word);
                    continue;
                }

                let chunk: String = word.chars().take(available).collect();
                output.push(format!("{}{}", prefix, chunk));

                let remainder: String = word.chars().skip(available).collect();
                if !remainder.is_empty() {
                    carry = Some(remainder);
                }

                first_line = false;
                emitted_chunk_line = true;
                break;
            }

            let candidate_len = line.chars().count() + 1 + word.chars().count();
            if candidate_len <= available {
                line.push(' ');
                line.push_str(&word);
            } else {
                carry = Some(word);
                break;
            }
        }

        if !line.is_empty() {
            output.push(format!("{}{}", prefix, line));
            first_line = false;
        } else if !emitted_chunk_line {
            break;
        }
    }

    output
}

fn push_wrapped_labeled_lines(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    label_style: Style,
    value_style: Style,
    width: usize,
) {
    let prefix = format!("{}: ", label);
    let indent = " ".repeat(prefix.chars().count());
    let wrapped = wrap_words_with_prefix(value, &prefix, &indent, width);

    for (idx, content) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            let value_text = content
                .strip_prefix(&prefix)
                .unwrap_or(content.as_str())
                .to_string();
            lines.push(Line::from(vec![
                Span::styled(prefix.clone(), label_style),
                Span::styled(value_text, value_style),
            ]));
        } else {
            let value_text = content
                .strip_prefix(&indent)
                .unwrap_or(content.as_str())
                .to_string();
            lines.push(Line::from(vec![
                Span::styled(indent.clone(), Style::default()),
                Span::styled(value_text, value_style),
            ]));
        }
    }
}

pub fn render(frame: &mut Frame, app: &AppState) {
    let layout = compute_layout(frame.area());

    render_input(frame, layout.input, app);
    render_results(frame, layout.results, app);
    render_info(frame, layout.info, app);
    render_status(frame, layout.status, app);
}

fn render_input(frame: &mut Frame, area: Rect, app: &AppState) {
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .title(app.input_title())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));

    frame.render_widget(input, area);
    frame.set_cursor_position((area.x + 1 + app.cursor as u16, area.y + 1));
}

fn render_results(frame: &mut Frame, area: Rect, app: &AppState) {
    if app.is_compose_mode() {
        let prompt = app.compose_prompt().unwrap_or("value");
        let parent = app.compose_parent_title().unwrap_or("item");
        let state_line = if app.compose_requires_next_sub_items() {
            "Required chain: Enter or Space continues"
        } else if app.compose_has_next_sub_items() {
            "Optional chain: Enter launches, Space continues"
        } else {
            "Final step: Enter launches"
        };

        let compose_list = List::new(vec![
            ListItem::new(Line::from(vec![
                Span::styled("Input Mode", Style::default().fg(Color::Yellow)),
                Span::styled("  ", Style::default()),
                Span::styled(parent, Style::default().fg(Color::White)),
            ])),
            ListItem::new(Line::from(vec![Span::styled(
                format!("Prompt: {}", prompt),
                Style::default().fg(Color::Gray),
            )])),
            ListItem::new(Line::from(vec![Span::styled(
                state_line,
                Style::default().fg(Color::DarkGray),
            )])),
        ])
        .block(Block::default().title(" Results ").borders(Borders::ALL));

        frame.render_widget(compose_list, area);
        return;
    }

    let list_items: Vec<ListItem> = if app.ranked.is_empty() {
        let empty_message = if app.input.is_empty()
            && app.items.iter().all(|item| item.provider == "pkg-manager")
        {
            "Type to search packages"
        } else {
            "No matching items"
        };

        vec![ListItem::new(Line::from(vec![Span::styled(
            empty_message,
            Style::default().fg(Color::DarkGray),
        )]))]
    } else {
        app.ranked
            .iter()
            .map(|ranked| {
                let item = &app.items[ranked.index];
                ListItem::new(Line::from(vec![Span::styled(
                    item.title.clone(),
                    Style::default().fg(Color::White),
                )]))
            })
            .collect()
    };

    let results_title = if app.is_results_focused() {
        " Results [Focus] "
    } else {
        " Results "
    };

    let list = List::new(list_items)
        .block(Block::default().title(results_title).borders(Borders::ALL))
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
        *state.offset_mut() = app.results_scroll(area.height.saturating_sub(2) as usize);
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_status(frame: &mut Frame, area: Rect, app: &AppState) {
    let controls = if app.is_compose_mode() {
        "Type: input | Enter: confirm | Space: continue chain | Esc: cancel"
    } else {
        "Tab: focus pane | Up/Down: active pane | PgUp/PgDn: info | Enter: launch | Mouse: scroll/click/double-click | Esc: quit"
    };
    let text = format!("{} | Rejected: {} | {}", app.status, app.rejected_items, controls);
    let status = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, area);
}

fn render_info(frame: &mut Frame, area: Rect, app: &AppState) {
    let info_title = if app.is_info_focused() {
        " Info [Focus] "
    } else {
        " Info "
    };
    let block = Block::default().title(info_title).borders(Borders::ALL);
    let width = info_inner_width(area);

    if app.is_compose_mode() {
        let prompt = app.compose_prompt().unwrap_or("value");
        let provider = app.compose_parent_provider().unwrap_or("unknown");
        let title = app.compose_parent_title().unwrap_or("item");
        let raw_input = if app.input.trim().is_empty() {
            "(empty)".to_string()
        } else {
            app.input.trim().to_string()
        };
        let base = app.compose_base_command().unwrap_or("(unknown)");
        let preview = app
            .compose_preview_command()
            .unwrap_or_else(|| "(preview unavailable)".to_string());

        let next_step = if app.compose_requires_next_sub_items() {
            "A required next step is queued after this input."
        } else if app.compose_has_next_sub_items() {
            "Optional next steps are available after this input."
        } else {
            "This is the final input step."
        };

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(Color::Cyan)),
                Span::styled("Compose Input", Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("Provider: ", Style::default().fg(Color::Cyan)),
                Span::styled(provider.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Target: ", Style::default().fg(Color::Cyan)),
                Span::styled(title.to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
        ];
        push_wrapped_labeled_lines(
            &mut lines,
            "Prompt",
            prompt,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::White),
            width,
        );
        push_wrapped_labeled_lines(
            &mut lines,
            "Input",
            &raw_input,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::White),
            width,
        );
        lines.extend(vec![
            Line::from(""),
        ]);
        push_wrapped_labeled_lines(
            &mut lines,
            "Base",
            base,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::Gray),
            width,
        );
        push_wrapped_labeled_lines(
            &mut lines,
            "Preview",
            &preview,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::White),
            width,
        );
        lines.extend(vec![
            Line::from(""),
        ]);
        push_wrapped_labeled_lines(
            &mut lines,
            "Next",
            next_step,
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::Gray),
            width,
        );

        let max_scroll = lines
            .len()
            .saturating_sub(area.height.saturating_sub(2) as usize) as u16;
        let info = Paragraph::new(lines)
            .block(block)
            .scroll((app.info_scroll.min(max_scroll), 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(info, area);
        return;
    }

    let Some(selected_ranked) = app.ranked.get(app.selected) else {
        let empty = Paragraph::new("No selection")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, area);
        return;
    };

    let selected_item = &app.items[selected_ranked.index];

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled("Provider: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.provider.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.id.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Cyan)),
            Span::styled(selected_item.title.clone(), Style::default().fg(Color::White)),
        ]),
    ];
    push_wrapped_labeled_lines(
        &mut lines,
        "Details",
        &selected_item.subtitle,
        Style::default().fg(Color::Cyan),
        Style::default().fg(Color::Gray),
        width,
    );
    lines.push(Line::from(""));
    push_wrapped_labeled_lines(
        &mut lines,
        "Summary",
        &selected_item.info.summary,
        Style::default().fg(Color::Cyan),
        Style::default().fg(Color::White),
        width,
    );
    lines.extend(vec![
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
    ]);

    if !selected_item.info.fields.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Fields:",
            Style::default().fg(Color::Cyan),
        )]));
        for field in &selected_item.info.fields {
            lines.push(Line::from(vec![
                Span::styled(format!("- {}: ", field.label), Style::default().fg(Color::Gray)),
                Span::styled(field.value.clone(), Style::default().fg(Color::White)),
            ]));
        }
    }

    lines.push(Line::from(""));
    let action_hint = match &selected_item.action {
        ItemAction::ShellCommandWithFlag(config) => {
            if selected_item.sub_items.is_empty() {
                format!("Press Space for {} then Enter", config.prompt)
            } else {
                format!("Press Space for {} (Space again to continue chain)", config.prompt)
            }
        }
        _ => {
            if !selected_item.sub_items.is_empty() {
                if selected_item.require_sub_item {
                    "Press Enter or Space to continue required sub-items".to_string()
                } else {
                    "Press Space to open sub-items, Enter for base action".to_string()
                }
            } else {
                "Press Enter to launch".to_string()
            }
        }
    };
    push_wrapped_labeled_lines(
        &mut lines,
        "Action",
        &action_hint,
        Style::default().fg(Color::Cyan),
        Style::default().fg(Color::White),
        width,
    );

    let max_scroll = lines
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize) as u16;
    let info = Paragraph::new(lines)
        .block(block)
        .scroll((app.info_scroll.min(max_scroll), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(info, area);
}
