use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::matcher::{RankedItem, rank_items};
use crate::models::{AppItem, ItemAction};

#[derive(Debug)]
pub struct ActionResult {
    pub message: String,
}

#[derive(Debug)]
pub struct AppState {
    pub input: String,
    pub cursor: usize,
    pub items: Vec<AppItem>,
    pub ranked: Vec<RankedItem>,
    pub selected: usize,
    pub status: String,
    pub rejected_items: usize,
    pub should_quit: bool,
    action_rx: Option<Receiver<ActionResult>>,
}

impl AppState {
    pub fn new(items: Vec<AppItem>, rejected_items: usize) -> Self {
        let ranked = rank_items("", &items);
        let status = if items.is_empty() {
            "No items loaded".to_string()
        } else {
            format!("Ready: {} items", items.len())
        };

        Self {
            input: String::new(),
            cursor: 0,
            items,
            ranked,
            selected: 0,
            status,
            rejected_items,
            should_quit: false,
            action_rx: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('q') if self.input.is_empty() => self.should_quit = true,
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char(ch);
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.input.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Up => self.select_prev(),
            KeyCode::Down => self.select_next(),
            KeyCode::Enter => self.launch_selected(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.cursor = 0;
                self.recompute_rankings();
            }
            _ => {}
        }
    }

    pub fn poll_action_results(&mut self) {
        if let Some(rx) = &self.action_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.status = result.message;
                    self.action_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Action channel disconnected".to_string();
                    self.action_rx = None;
                }
            }
        }
    }

    pub fn selected_item(&self) -> Option<&AppItem> {
        let ranked = self.ranked.get(self.selected)?;
        self.items.get(ranked.index)
    }

    fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.recompute_rankings();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let previous = self
            .input
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
            .map(|(idx, ch)| (idx, ch.len_utf8()));

        if let Some((idx, len)) = previous {
            self.input.drain(idx..idx + len);
            self.cursor = idx;
            self.recompute_rankings();
        }
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.ranked.len() {
            self.selected += 1;
        }
    }

    fn recompute_rankings(&mut self) {
        self.ranked = rank_items(&self.input, &self.items);
        self.selected = 0;

        if self.ranked.is_empty() {
            self.status = "No matches".to_string();
        } else {
            self.status = format!("{} matches", self.ranked.len());
        }
    }

    fn launch_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.status = "No item selected".to_string();
            return;
        };

        match item.action {
            ItemAction::ShellCommand(command) => {
                let (tx, rx) = mpsc::channel();
                self.action_rx = Some(rx);
                self.status = format!("Running: {}", item.title);

                thread::spawn(move || {
                    let output = Command::new("sh").arg("-lc").arg(&command).output();

                    let message = match output {
                        Ok(result) if result.status.success() => {
                            let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
                            if stdout.is_empty() {
                                "Action completed".to_string()
                            } else {
                                format!("Action completed: {}", stdout)
                            }
                        }
                        Ok(result) => {
                            let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
                            if stderr.is_empty() {
                                format!("Action failed: exit {:?}", result.status.code())
                            } else {
                                format!("Action failed: {}", stderr)
                            }
                        }
                        Err(err) => format!("Action failed to start: {}", err),
                    };

                    let _ = tx.send(ActionResult { message });
                });
            }
            ItemAction::ProviderHint(message) => {
                self.status = message;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::models::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};

    fn test_item(title: &str, command: &str) -> AppItem {
        let provider_item = ProviderItem {
            id: title.to_lowercase(),
            title: title.to_string(),
            subtitle: "test subtitle".to_string(),
            info: ItemInfo {
                summary: "test summary".to_string(),
                fields: vec![InfoField {
                    label: "test".to_string(),
                    value: "value".to_string(),
                }],
            },
            action: ItemAction::ShellCommand(command.to_string()),
        };

        AppItem::from_provider_item("test", provider_item).expect("valid test item")
    }

    #[test]
    fn typing_snaps_selection_to_top() {
        let mut app = AppState::new(
            vec![
                test_item("Notes", "echo notes"),
                test_item("Git Status", "echo git"),
                test_item("Calendar", "echo calendar"),
            ],
            0,
        );

        app.selected = 2;
        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('g')));

        assert_eq!(app.selected, 0);
        assert!(!app.ranked.is_empty());
    }
}
