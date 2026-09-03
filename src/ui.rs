use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, prelude::Rect};

use crate::app::AppState;
use crate::models::ItemAction;

// This file decides how all the boxes on the screen are arranged.
// It is like drawing a little map for the app.
//
// Every rectangle here is just a box on the terminal screen:
// - input box at the top
// - results list in the middle
// - info panel to the right
// - status bar at the bottom
#[derive(Debug, Clone, Copy)]
pub struct UiLayout {
    pub input: Rect,
    pub results: Rect,
    pub info: Rect,
    pub status: Rect,
}

impl UiLayout {
    // `Rect.height` includes the border rows. Ratatui's list uses only the inside of
    // the box for items, so remove one row for the top border and one for the bottom.
    // `saturating_sub` prevents an underflow if a very small terminal gives us a box
    // that is shorter than two rows.
    pub fn results_viewport_height(&self) -> usize {
        self.results.height.saturating_sub(2) as usize
    }

    // Mouse events contain absolute terminal coordinates. Pass those coordinates and
    // this panel's rectangle to the shared boundary check; the result is true when the
    // point is inside the panel's full rectangle, including the border.
    pub fn info_contains(&self, column: u16, row: u16) -> bool {
        point_in_rect(self.info, column, row)
    }

    // First reject clicks outside the whole results rectangle. Then calculate the first
    // usable row: `results.y + 1` skips the top border. The bottom boundary is also
    // exclusive, so `height - 1` skips the bottom border. Finally subtract the first
    // usable row to convert an absolute screen row into a zero-based list row.
    pub fn results_row_at(&self, column: u16, row: u16) -> Option<usize> {
        if point_in_rect(self.results, column, row) == false {
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

// The screen is split into three main parts:
// 1) the input box at the top
// 2) the results list in the middle
// 3) the status bar at the bottom
//
// A second split happens inside the middle section: the results list takes the left side,
// and the info panel takes the right side. That way the user can see both the list of choices
// and the details about the currently selected item side-by-side.
pub fn compute_layout(area: Rect) -> UiLayout {
    // Split the full terminal rectangle vertically. The first three rows are reserved
    // for the input, the middle gets every remaining row but at least three, and the
    // final row is reserved for status text. `split` returns the resulting rectangles
    // in the same order as these constraints.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    // Take the middle rectangle from the first split and divide it horizontally.
    // The percentages are relative to the body's width, so the result list gets 55%
    // and the information panel gets the remaining 45%.
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
    // Rect stores a start coordinate plus a size. Add the size to get the exclusive
    // right and bottom edges. The four comparisons implement a half-open rectangle:
    // the left/top edges count, while the right/bottom edges do not.
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    x >= rect.x && x < right && y >= rect.y && y < bottom
}

fn info_inner_width(area: Rect) -> usize {
    // The panel's width includes its left and right border columns. Remove both before
    // passing the width to the wrapper, otherwise wrapped text would touch or cross a border.
    area.width.saturating_sub(2) as usize
}

fn wrap_words_with_prefix(text: &str, first_prefix: &str, next_prefix: &str, width: usize) -> Vec<String> {
    // Trim outer whitespace, then turn the remaining text into words. The function
    // builds complete output strings because the first line has one prefix (for example
    // "Summary: ") and later lines have a different indentation prefix.
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![first_prefix.to_string()];
    }

    let mut output = Vec::new();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let mut index = 0usize;
    let mut carry: Option<String> = None;
    let mut first_line = true;

    // Continue while either a word has not been read yet or `carry` contains the part
    // of a long word that did not fit on the previous line.
    while index < words.len() || carry.is_some() {
        // Choose the prefix before calculating space. Prefix characters consume terminal
        // columns just like the visible value does.
        let prefix = if first_line { first_prefix } else { next_prefix };
        let available = width.saturating_sub(prefix.chars().count()).max(1);
        let mut line = String::new();
        let mut emitted_chunk_line = false;

        // Fill one line. The inner loop may end because the next word does not fit, or
        // because it had to be split into a chunk and a remainder.
        while index < words.len() || carry.is_some() {
            // Prefer the saved remainder. Otherwise copy the next word and advance the
            // index immediately, because that word is now being processed.
            let word = if let Some(value) = carry.take() {
                value
            } else {
                let value = words[index].to_string();
                index += 1;
                value
            };

            if line.is_empty() {
                if word.chars().count() <= available {
                    // No separator is needed at the start of a line.
                    line.push_str(&word);
                    continue;
                }

                // A word longer than the available width cannot be moved intact. Take
                // exactly the number of characters that fit, emit that chunk, and save
                // the remaining characters so the outer loop handles them next.
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

            // A normal word needs one extra column for the space before it.
            let candidate_len = line.chars().count() + 1 + word.chars().count();
            if candidate_len <= available {
                // The candidate fits, so add the separator and then the word.
                line.push(' ');
                line.push_str(&word);
            } else {
                // The word was taken from `words`, but it belongs on the next line.
                // Put it in `carry` so it is not lost.
                carry = Some(word);
                break;
            }
        }

        if !line.is_empty() {
            // A normal line accumulated words, so attach its prefix and store it.
            output.push(format!("{}{}", prefix, line));
            first_line = false;
        } else if emitted_chunk_line == false {
            // No normal text or chunk was emitted. Stop rather than loop forever.
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
    // Build the visible label once and use spaces of the same length for continuation
    // lines. Wrapping is done before styling so the first line can be split into a
    // colored label span and a colored value span.
    let prefix = format!("{}: ", label);
    let indent = " ".repeat(prefix.chars().count());
    let wrapped = wrap_words_with_prefix(value, &prefix, &indent, width);

    for (idx, content) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            // Remove the known prefix from the wrapped string. `unwrap_or` keeps the
            // whole string if a future caller supplies an unexpected prefix.
            let value_text = content
                .strip_prefix(&prefix)
                .unwrap_or(content.as_str())
                .to_string();
            lines.push(Line::from(vec![
                Span::styled(prefix.clone(), label_style),
                Span::styled(value_text, value_style),
            ]));
        } else {
            // Continuation lines begin with indentation, so remove that indentation and
            // render it separately to preserve the alignment.
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

fn info_warning_lines(app: &AppState) -> Vec<Line<'static>> {
    // The app uses a `Warning:` prefix to mark warning status messages. Return no lines
    // for ordinary statuses; otherwise clone the message because the returned Lines own
    // their text and add a blank line to separate the warning from the details.
    if !app.status.starts_with("Warning:") {
        return Vec::new();
    }

    vec![
        Line::from(vec![Span::styled(
            app.status.clone(),
            Style::default().fg(Color::Rgb(255, 165, 0)).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ]
}

pub fn render(frame: &mut Frame, app: &AppState) {
    // Ask Ratatui for the current terminal rectangle, calculate the four child rectangles,
    // and pass the same app state to each renderer. Rendering does not change app state;
    // it only turns the current state into terminal widgets.
    let layout = compute_layout(frame.area());

    // Each helper owns one rectangle, which keeps input, results, details, and status
    // drawing independent while they all share the layout calculated above.
    render_input(frame, layout.input, app);
    render_results(frame, layout.results, app);
    render_info(frame, layout.info, app);
    render_status(frame, layout.status, app);
}

// Draw the box where the user types text.
fn render_input(frame: &mut Frame, area: Rect, app: &AppState) {
    // Borrow the input string for the paragraph, then attach a bordered block. The title
    // comes from app state because it changes between Query, Sub Items, and Flag Input.
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .title(app.input_title())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));

    // Draw the widget first. The cursor is positioned separately because it is terminal
    // state rather than part of the Paragraph's text.
    frame.render_widget(input, area);
    // Add one column and one row to move from the outer border to the text area. `cursor`
    // is a byte offset, but it is converted here to a terminal column for the existing
    // ASCII-oriented input display.
    frame.set_cursor_position((area.x + 1 + app.cursor as u16, area.y + 1));
}

// Draw the list of possible items.
// The selected one is highlighted and the user can move up and down through it.
//
// This function does two jobs:
// - if the app is in compose mode, it shows a help screen instead of the normal list
// - otherwise, it renders the ranked result list and highlights the active item
fn render_results(frame: &mut Frame, area: Rect, app: &AppState) {
    if app.is_compose_mode() {
    // Compose mode takes priority over normal results. Read the saved compose values,
    // choose one message describing whether another step exists, and build exactly
    // three ListItems for the temporary explanation panel.
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

        // This branch has finished drawing its replacement list, so return before the
        // normal ranked-result code below can run.
        frame.render_widget(compose_list, area);
        return;
    }

    // Convert ranked data into ListItems. When there are no ranked entries, choose a
    // message based on installer mode. Otherwise follow each RankedItem's original
    // `index` back into `app.items` and copy that item's title into a visible row.
    let list_items: Vec<ListItem> = if app.ranked.is_empty() {
        let empty_message = if app.input.is_empty()
            && app.items.iter().all(|item| item.provider == "installer")
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
        // The title is selected before the Block is built so focus is visible without
        // changing the result data itself.
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

    // ListState is separate from List: it stores which row is selected and where the
    // list's viewport starts. Create it even for an empty list, then fill those values
    // only when a real ranked result exists.
    let mut state = ListState::default();
    if !app.ranked.is_empty() {
        // `selected` is an index into the ranked list. The scroll offset is calculated
        // from the inside height so Ratatui shows the selected row within the viewport.
        state.select(Some(app.selected));
        *state.offset_mut() = app.results_scroll(area.height.saturating_sub(2) as usize);
    }

    frame.render_stateful_widget(list, area, &mut state);
}

// The status bar explains what is happening and tells the user which keys do what.
fn render_status(frame: &mut Frame, area: Rect, app: &AppState) {
    // Choose one static control description for each interaction mode, then prepend the
    // dynamic status and rejected-item count. The final string is rendered as one paragraph.
    let controls = if app.is_compose_mode() {
        "Type: input | Enter: confirm | Space: continue chain | Esc: cancel"
    } else {
        "Tab: focus pane | Up/Down: active pane | PgUp/PgDn: info | Enter: launch | Mouse: scroll/click/double-click | Esc: quit"
    };
    let text = format!("{} | Rejected: {} | {}", app.status, app.rejected_items, controls);
    let status = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, area);
}

// The info panel shows more details about the current selection, like its provider,
// summary, and action hints.
//
// This is the part the user reads when they want to know:
// "What is this item? Why is it ranked here? What happens if I press Enter?"
fn render_info(frame: &mut Frame, area: Rect, app: &AppState) {
    // Build the block and usable text width once. The rest of the function has two branches:
    // compose mode describes temporary input state, while normal mode describes the selected item.
    let info_title = if app.is_info_focused() {
        " Info [Focus] "
    } else {
        " Info "
    };
    let block = Block::default().title(info_title).borders(Borders::ALL);
    let width = info_inner_width(area);

    if app.is_compose_mode() {
        // Read each optional compose value with a display fallback. These fallbacks keep
        // the info panel renderable even if a caller creates an incomplete state.
        let prompt = app.compose_prompt().unwrap_or("value");
        let provider = app.compose_parent_provider().unwrap_or("unknown");
        let title = app.compose_parent_title().unwrap_or("item");
        // Show `(empty)` instead of a blank value so the user can tell that the prompt
        // is waiting for input rather than failing to render.
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

        // Start with any warning, then append labeled rows. The helper wraps long values
        // before adding styled spans, so every row remains inside the panel width.
        let mut lines: Vec<Line<'static>> = info_warning_lines(app);
        lines.extend(vec![
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
        ]);
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

        // `lines.len()` counts all lines we want to display.
        // `area.height - 2` leaves out the top and bottom border rows.
        // Subtracting the visible height tells us how many lines overflow.
        // `saturating_sub` returns 0 when everything fits and avoids underflow
        // when the terminal is too small. The final cast matches `info_scroll`'s type.
        let max_scroll = lines
            .len()
            .saturating_sub(area.height.saturating_sub(2) as usize) as u16;

        // Give the lines and border to Ratatui's paragraph widget.
        let info = Paragraph::new(lines)
            .block(block)
            // Keep vertical scrolling inside the available content.
            // The second value disables horizontal scrolling.
            .scroll((app.info_scroll.min(max_scroll), 0))
            // Wrap long lines, but do not remove their surrounding spaces.
            .wrap(Wrap { trim: false });

        // Draw the paragraph inside the info panel.
        frame.render_widget(info, area);

        // Compose mode is finished, so do not run the normal item-rendering code below.
        return;
    }

    // In normal mode, use the selected ranked entry as an indirection: ranked entries
    // store original item indexes, so the index must be used to fetch the real AppItem.
    let Some(selected_ranked) = app.ranked.get(app.selected) else {
        // No ranked entry means there is no item to describe. Still render warnings and
        // a clear message inside the bordered panel.
        let mut lines = info_warning_lines(app);
        lines.push(Line::from(Span::styled(
            "No selection",
            Style::default().fg(Color::DarkGray),
        )));
        let empty = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(empty, area);
        return;
    };

    // This lookup is safe because ranking was created from the same `items` vector.
    // The ranked entry's index points back to the source item.
    let selected_item = &app.items[selected_ranked.index];

    let mut lines: Vec<Line<'static>> = info_warning_lines(app);
    lines.extend(vec![
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
    ]);
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

    // Add the optional fields only when the provider supplied some. Each field becomes
    // one line with a gray label and a white value.
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
    // Select a hint from the action shape and submenu state. This does not execute the
    // action; it only tells the user which key will trigger the next state transition.
    let action_hint: String = match &selected_item.action {
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
        action_hint.as_str(),
        Style::default().fg(Color::Cyan),
        Style::default().fg(Color::White),
        width,
    );

    // Compute the largest useful vertical offset and clamp the user's stored scroll
    // position before giving it to Ratatui.
    let max_scroll = lines
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize) as u16;
    let info = Paragraph::new(lines)
        .block(block)
        .scroll((app.info_scroll.min(max_scroll), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(info, area);
}
