use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::env;
use std::time::{Duration, Instant};

use crate::matcher::{RankedItem, rank_items};
use crate::models::{ActionSubItem, AppItem, InfoField, ItemAction, ItemInfo, ShellCommandWithFlag};
use crate::path_discovery::discover_path_sub_items;
use crate::providers::load_all_items_from_args;

#[derive(Debug)]
pub struct PendingShellCommand {
    pub command: String,
    pub exit_after: bool,
}

#[derive(Debug)]
struct ComposeState {
    config: ShellCommandWithFlag,
    previous_query: String,
    previous_cursor: usize,
    next_sub_items: Vec<ActionSubItem>,
    require_sub_item: bool,
    parent_provider: String,
    parent_title: String,
}

#[derive(Debug)]
struct ViewState {
    items: Vec<AppItem>,
    input: String,
    cursor: usize,
}

#[derive(Debug)]
struct LastClickState {
    index: usize,
    at: Instant,
}

#[derive(Debug)]
struct PendingPkgLookup {
    query: String,
    due_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Results,
    Info,
}

// This is the brain of the app.
// It keeps track of the typed query, the currently selected result,
// the list of items loaded from providers, and whether the user is in a
// special "compose a command" mode.
#[derive(Debug)]
pub struct AppState {
    pub input: String,
    pub cursor: usize,
    pub items: Vec<AppItem>,
    pub ranked: Vec<RankedItem>,
    pub selected: usize,
    pub status: String,
    pub rejected_items: usize,
    pub info_scroll: u16,
    pub pane_focus: PaneFocus,
    pub should_quit: bool,
    pending_shell_command: Option<PendingShellCommand>,
    compose_state: Option<ComposeState>,
    view_stack: Vec<ViewState>,
    last_click: Option<LastClickState>,
    pending_installer_lookup: Option<PendingPkgLookup>,
    last_installer_lookup_query: Option<String>,
}

impl AppState {
    // Build a fresh app state from the items that were loaded.
    // If the app is showing only installer items, the list is kept hidden until
    // the user starts typing a package name.
    pub fn new(items: Vec<AppItem>, rejected_items: usize) -> Self {
        let is_installer_only = !items.is_empty() && items.iter().all(|item| item.provider == "installer");
        let ranked = if is_installer_only {
            Vec::new()
        } else {
            rank_items("", &items)
        };
        let status = if items.is_empty() {
            "No items loaded".to_string()
        } else if is_installer_only {
            "Type a package name to search".to_string()
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
            info_scroll: 0,
            pane_focus: PaneFocus::Results,
            should_quit: false,
            pending_shell_command: None,
            compose_state: None,
            view_stack: Vec::new(),
            last_click: None,
            pending_installer_lookup: None,
            last_installer_lookup_query: None,
        }
    }

    // This is the main key handler.
    // It is the biggest decision point in the app.
    //
    // Think of it like a classroom bell system:
    // - if the user is in "compose mode", we do a different set of rules
    // - if the user is typing in the search box, letters add text
    // - if the user presses Enter, the selected item launches
    // - if the user presses Tab, focus moves between the results and info panels
    // - if the user presses Space, the app may open a sub-menu or start a command builder
    //
    // This function decides which behavior should happen for every key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.compose_state.is_some() {
            self.handle_compose_key(key);
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => self.toggle_focus(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                if self.compose_state.is_some() {
                    self.cancel_compose_mode();
                } else if !self.close_sub_items_view() {
                    self.should_quit = true;
                }
            }
            KeyCode::Char(' ') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.try_start_compose_mode() {
                    return;
                }

                if self.try_open_sub_items_view() {
                    return;
                }

                self.insert_char(' ');
            }
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
            KeyCode::Home => {
                if self.is_info_focused() {
                    self.info_scroll = 0;
                } else {
                    self.cursor = 0;
                }
            }
            KeyCode::End => {
                if self.is_info_focused() {
                    self.info_scroll = u16::MAX;
                } else {
                    self.cursor = self.input.len();
                }
            }
            KeyCode::PageUp => {
                if self.is_info_focused() {
                    self.scroll_info_by(-8);
                }
            }
            KeyCode::PageDown => {
                if self.is_info_focused() {
                    self.scroll_info_by(8);
                }
            }
            KeyCode::Up => {
                if self.is_info_focused() {
                    self.scroll_info_by(-1);
                } else {
                    self.select_prev();
                }
            }
            KeyCode::Down => {
                if self.is_info_focused() {
                    self.scroll_info_by(1);
                } else {
                    self.select_next();
                }
            }
            KeyCode::Enter => {
                if self.try_force_installer_lookup() {
                    return;
                }
                self.launch_selected();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.cursor = 0;
                self.recompute_rankings();
            }
            _ => {}
        }
    }

    // This is the special key handler used while the user is typing a command value.
    //
    // Example: the user picks a command like "apt install" and then the app asks for
    // a package name. While that prompt is open, the search box is not "searching" the
    // item list anymore. Instead, it is collecting one piece of text to finish the command.
    //
    // So this function uses a different rule set:
    // - Enter often confirms the final command
    // - Space may continue a chain of sub-choices
    // - Esc cancels the command building
    // - Backspace and arrow keys still move around the current typed text
    fn handle_compose_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.cancel_compose_mode();
            }
            KeyCode::Enter => {
                if self.compose_requires_next_sub_items() {
                    self.advance_compose_chain();
                } else {
                    self.launch_composed_command();
                }
            }
            KeyCode::Char(' ') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.compose_has_next_sub_items() {
                    self.advance_compose_chain();
                } else {
                    self.insert_char(' ');
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
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
            _ => {}
        }
    }

    pub fn poll_action_results(&mut self) {
        self.process_debounced_installer_lookup();
    }

    pub fn take_pending_shell_command(&mut self) -> Option<PendingShellCommand> {
        self.pending_shell_command.take()
    }

    pub fn set_status(&mut self, message: String) {
        self.status = message;
    }

    pub fn input_title(&self) -> &str {
        if self.compose_state.is_some() {
            " Flag Input "
        } else if !self.view_stack.is_empty() {
            " Sub Items "
        } else {
            " Query "
        }
    }

    pub fn is_results_focused(&self) -> bool {
        self.pane_focus == PaneFocus::Results
    }

    pub fn is_info_focused(&self) -> bool {
        self.pane_focus == PaneFocus::Info
    }

    fn toggle_focus(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Results => PaneFocus::Info,
            PaneFocus::Info => PaneFocus::Results,
        };

        self.status = match self.pane_focus {
            PaneFocus::Results => "Focus: Results".to_string(),
            PaneFocus::Info => "Focus: Info".to_string(),
        };
    }

    pub fn is_compose_mode(&self) -> bool {
        self.compose_state.is_some()
    }

    pub fn compose_parent_title(&self) -> Option<&str> {
        self.compose_state
            .as_ref()
            .map(|state| state.parent_title.as_str())
    }

    pub fn compose_parent_provider(&self) -> Option<&str> {
        self.compose_state
            .as_ref()
            .map(|state| state.parent_provider.as_str())
    }

    pub fn compose_prompt(&self) -> Option<&str> {
        self.compose_state
            .as_ref()
            .map(|state| state.config.prompt.as_str())
    }

    pub fn compose_base_command(&self) -> Option<&str> {
        self.compose_state
            .as_ref()
            .map(|state| state.config.command.as_str())
    }

    pub fn compose_preview_command(&self) -> Option<String> {
        self.compose_state
            .as_ref()
            .map(|state| self.compose_command(&state.config, self.input.trim()))
    }

    pub fn compose_has_next_sub_items(&self) -> bool {
        self.compose_state
            .as_ref()
            .is_some_and(|state| !state.next_sub_items.is_empty())
    }

    pub fn compose_requires_next_sub_items(&self) -> bool {
        self.compose_state
            .as_ref()
            .is_some_and(|state| state.require_sub_item && !state.next_sub_items.is_empty())
    }

    pub fn selected_item(&self) -> Option<&AppItem> {
        let ranked = self.ranked.get(self.selected)?;
        self.items.get(ranked.index)
    }

    fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        if self.compose_state.is_none() {
            self.recompute_rankings();
        }
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
            if self.compose_state.is_none() {
                self.recompute_rankings();
            }
        }
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.on_selection_changed();
        }
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.ranked.len() {
            self.selected += 1;
            self.on_selection_changed();
        }
    }

    pub fn select_index(&mut self, index: usize) {
        if self.ranked.is_empty() {
            return;
        }

        let clamped = index.min(self.ranked.len().saturating_sub(1));
        if self.selected != clamped {
            self.selected = clamped;
            self.on_selection_changed();
        }
    }

    pub fn move_selection_by(&mut self, delta: isize) {
        if self.ranked.is_empty() || delta == 0 {
            return;
        }

        let current = self.selected as isize;
        let max = self.ranked.len().saturating_sub(1) as isize;
        let next = (current + delta).clamp(0, max) as usize;
        self.select_index(next);
    }

    pub fn results_scroll(&self, viewport_height: usize) -> usize {
        if viewport_height == 0 || self.selected < viewport_height {
            0
        } else {
            self.selected + 1 - viewport_height
        }
    }

    pub fn result_index_for_row(&self, row: usize, viewport_height: usize) -> Option<usize> {
        if self.ranked.is_empty() || viewport_height == 0 || row >= viewport_height {
            return None;
        }

        let offset = self.results_scroll(viewport_height);
        let index = offset + row;
        if index < self.ranked.len() {
            Some(index)
        } else {
            None
        }
    }

    pub fn click_result_index(&mut self, index: usize) {
        if index >= self.ranked.len() {
            return;
        }

        const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);
        let now = Instant::now();
        let double_click = self
            .last_click
            .as_ref()
            .is_some_and(|last| last.index == index && now.duration_since(last.at) <= DOUBLE_CLICK_WINDOW);

        self.select_index(index);
        self.last_click = Some(LastClickState { index, at: now });

        if double_click {
            self.launch_selected();
        }
    }

    pub fn scroll_info_by(&mut self, delta: i16) {
        if delta < 0 {
            self.info_scroll = self.info_scroll.saturating_sub(delta.unsigned_abs());
            return;
        }

        self.info_scroll = self.info_scroll.saturating_add(delta as u16);
    }

    fn on_selection_changed(&mut self) {
        self.info_scroll = 0;
    }

    // Re-sort the results every time the user types or deletes a character.
    // This is the moment when the UI decides which item looks most like the query.
    //
    // The search algorithm uses the `matcher` module to give every item a score.
    // Example: if the query is "git", then items whose title starts with "git"
    // get a much bigger score than a random item whose title only contains the letters
    // later in the string.
    //
    // This method also updates the status text so the user sees messages like
    // "No matches" or "3 matches".
    fn recompute_rankings(&mut self) {
        if self.input.is_empty() && self.is_installer_only() {
            self.ranked.clear();
            self.selected = 0;
            self.info_scroll = 0;
            self.last_click = None;
            self.pending_installer_lookup = None;
            self.status = "Type a package name to search".to_string();
            return;
        }

        self.ranked = rank_items(&self.input, &self.items);
        self.selected = 0;
        self.info_scroll = 0;
        self.last_click = None;

        if self.ranked.is_empty() {
            self.status = "No matches".to_string();
        } else {
            self.status = format!("{} matches", self.ranked.len());
        }

        self.schedule_installer_lookup();
    }

    fn is_installer_only(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|item| item.provider == "installer")
    }

    fn installer_lookup_debounce(query: &str) -> Duration {
        if query.chars().count() <= 3 {
            Duration::from_millis(180)
        } else {
            Duration::from_millis(120)
        }
    }

    // This method is the "wait a tiny bit before asking the installer provider for new data" step.
    //
    // Why this matters:
    // 1. the user may type several letters quickly
    // 2. we do not want to reload the package list after every single key press
    // 3. so we save the current query and set a short future time to run the lookup
    // 4. when that time arrives, the app fetches fresh package results once
    fn schedule_installer_lookup(&mut self) {
        if !self.is_installer_only() {
            self.pending_installer_lookup = None;
            return;
        }

        let query = self.input.trim();
        if query.chars().count() < 2 {
            self.pending_installer_lookup = None;
            return;
        }

        if self.last_installer_lookup_query.as_deref() == Some(query) {
            return;
        }

        self.pending_installer_lookup = Some(PendingPkgLookup {
            query: query.to_string(),
            due_at: Instant::now() + Self::installer_lookup_debounce(query),
        });
    }

    // This is the moment when the delayed package search finally fires.
    //
    // Step by step:
    // 1. check whether the timer has reached its deadline
    // 2. if not, do nothing
    // 3. if yes, take the saved lookup request out of the queue
    // 4. make sure the user is still typing the same query
    // 5. if the lookup has not already run for that exact query, fetch new results
    fn process_debounced_installer_lookup(&mut self) {
        let should_run = self
            .pending_installer_lookup
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.due_at);

        if !should_run {
            return;
        }

        let Some(pending) = self.pending_installer_lookup.take() else {
            return;
        };

        if self.input.trim() != pending.query {
            return;
        }

        if self.last_installer_lookup_query.as_deref() == Some(pending.query.as_str()) {
            return;
        }

        self.lookup_installer_query(&pending.query);
    }

    // This is the manual override for the installer search.
    //
    // Imagine the user presses Enter before the usual debounce timer finishes.
    // We still want the package list to refresh immediately for that query, so this method forces the
    // lookup to run right away if the user is in the installer-only mode and has typed enough text.
    fn try_force_installer_lookup(&mut self) -> bool {
        if !self.is_installer_only() {
            return false;
        }

        let query = self.input.trim().to_string();
        if query.chars().count() < 2 {
            return false;
        }

        if self.last_installer_lookup_query.as_deref() == Some(query.as_str()) {
            return false;
        }

        self.pending_installer_lookup = None;
        self.lookup_installer_query(&query);
        true
    }

    // For the installer provider, the app does a delayed search.
    // This lets the user type a few letters before asking the provider for a new list
    // of package results, instead of reloading every single keystroke.
    //
    // Why is this useful? If the user types "fire" and the app asks the installer
    // provider after every letter, it might do too much work and feel jumpy.
    // Instead, the app waits a tiny bit, then refreshes the package list once the user
    // seems to be done typing.
    fn lookup_installer_query(&mut self, query: &str) {
        let args = vec!["--installer".to_string()];
        let previous = env::var_os("TUISUAL_PROVIDER_QUERY");

        unsafe {
            env::set_var("TUISUAL_PROVIDER_QUERY", query);
        }

        let load_report = load_all_items_from_args(&args);

        match previous {
            Some(value) => unsafe {
                env::set_var("TUISUAL_PROVIDER_QUERY", value);
            },
            None => unsafe {
                env::remove_var("TUISUAL_PROVIDER_QUERY");
            },
        }

        self.items = load_report.items;
        self.rejected_items = load_report.rejected.len();
        self.last_installer_lookup_query = Some(query.to_string());
        self.recompute_rankings();
    }

    // When the user presses Enter, this method turns the selected item into an action.
    // It may open a nested menu, start a prompt, or launch a shell command.
    //
    // This is the "do the thing" step. The item is already chosen and ranked, and now
    // we decide what that choice means in the real world.
    fn launch_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            self.status = "No item selected".to_string();
            return;
        };

        if self.handle_required_sub_items(item.clone()) {
            return;
        }

        self.launch_item(item);
    }

    // This turns an AppItem into a real action.
    // A plain command runs immediately, a "with flag" action opens a small typing prompt,
    // and a provider hint tells the app to load a different provider.
    //
    // In other words: the list is only a menu. Every menu item has a destination.
    // This function performs that destination.
    fn launch_item(&mut self, item: AppItem) {
        match item.action {
            ItemAction::ShellCommand(command) => {
                self.status = format!("Running: {}", item.title);
                self.pending_shell_command = Some(PendingShellCommand {
                    command,
                    exit_after: false,
                });
            }
            ItemAction::ShellCommandExit(command) => {
                self.status = format!("Launching and exiting: {}", item.title);
                self.pending_shell_command = Some(PendingShellCommand {
                    command,
                    exit_after: true,
                });
            }
            ItemAction::ShellCommandWithFlag(config) => {
                self.start_compose_mode(
                    config,
                    item.sub_items,
                    item.require_sub_item,
                    item.provider,
                    item.title,
                );
            }
            ItemAction::ProviderHint(message) => {
                self.load_provider_by_name(&message);
            }
        }
    }

    // Some actions are not complete until the user picks one more step.
    // Example: a command might need a specific flag, or a package manager might require
    // a sub-selection before it can run.
    //
    // This function checks whether the chosen item has a required "next step" and then
    // either opens that next menu or starts the command prompt immediately.
    fn handle_required_sub_items(&mut self, item: AppItem) -> bool {
        if !item.require_sub_item || item.sub_items.is_empty() {
            return false;
        }

        if item.sub_items.len() == 1 {
            let Some(next_item) = self.build_sub_item_from_parent(&item, &item.sub_items[0]) else {
                self.status = "Required sub-item is not compatible with this action".to_string();
                return true;
            };

            match next_item.action {
                ItemAction::ShellCommandWithFlag(config) => {
                    self.start_compose_mode(
                        config,
                        next_item.sub_items,
                        next_item.require_sub_item,
                        next_item.provider,
                        next_item.title,
                    );
                }
                _ => {
                    if self.handle_required_sub_items(next_item.clone()) {
                        return true;
                    }
                    self.launch_item(next_item);
                }
            }

            return true;
        }

        self.open_sub_items_for_parent(item);
        true
    }

    fn try_start_compose_mode(&mut self) -> bool {
        let Some(item) = self.selected_item().cloned() else {
            return false;
        };

        let ItemAction::ShellCommandWithFlag(config) = item.action else {
            return false;
        };

        self.start_compose_mode(
            config,
            item.sub_items,
            item.require_sub_item,
            item.provider,
            item.title,
        );
        true
    }

    // "Compose mode" is used when a command needs extra text from the user,
    // such as a package name or a flag value. The app temporarily swaps the
    // normal search input for a one-line command builder.
    //
    // We save the previous query and cursor so the app can restore the search box
    // after the command is built. This lets the user go back to their original search
    // after finishing the extra input step.
    fn start_compose_mode(
        &mut self,
        config: ShellCommandWithFlag,
        next_sub_items: Vec<ActionSubItem>,
        require_sub_item: bool,
        parent_provider: String,
        parent_title: String,
    ) {
        let prompt = config.prompt.clone();
        let has_next_sub_items = !next_sub_items.is_empty();
        let previous_query = self.input.clone();
        let previous_cursor = self.cursor;
        self.compose_state = Some(ComposeState {
            config,
            previous_query,
            previous_cursor,
            next_sub_items,
            require_sub_item,
            parent_provider,
            parent_title,
        });
        self.input.clear();
        self.cursor = 0;
        self.info_scroll = 0;
        self.status = if has_next_sub_items && require_sub_item {
            format!(
                "Compose mode: type {} then Enter or Space for required next step (Esc to cancel)",
                prompt
            )
        } else if has_next_sub_items {
            format!(
                "Compose mode: type {} then Space for next step or Enter to launch (Esc to cancel)",
                prompt
            )
        } else {
            format!("Compose mode: type {} then Enter (Esc to cancel)", prompt)
        };
    }

    fn launch_composed_command(&mut self) {
        let Some(state) = self.compose_state.take() else {
            return;
        };

        let command = self.compose_command(&state.config, self.input.trim());
        self.input = state.previous_query;
        self.cursor = state.previous_cursor.min(self.input.len());

        self.pending_shell_command = Some(PendingShellCommand {
            command,
            exit_after: state.config.exit_after,
        });

        self.recompute_rankings();
    }

    fn advance_compose_chain(&mut self) {
        let Some(state) = self.compose_state.take() else {
            return;
        };

        if state.next_sub_items.is_empty() {
            self.pending_shell_command = Some(PendingShellCommand {
                command: self.compose_command(&state.config, self.input.trim()),
                exit_after: state.config.exit_after,
            });
            self.input = state.previous_query;
            self.cursor = state.previous_cursor.min(self.input.len());
            self.recompute_rankings();
            return;
        }

        let command = self.compose_command(&state.config, self.input.trim());
        self.input = state.previous_query;
        self.cursor = state.previous_cursor.min(self.input.len());

        let parent = AppItem {
            provider: state.parent_provider,
            id: format!("compose:{}", state.parent_title.to_ascii_lowercase().replace(' ', "-")),
            title: state.parent_title,
            subtitle: "Composed step".to_string(),
            info: ItemInfo {
                summary: "Continue selecting chained sub-items.".to_string(),
                fields: vec![InfoField {
                    label: "Composed Command".to_string(),
                    value: command.clone(),
                }],
            },
            action: if state.config.exit_after {
                ItemAction::ShellCommandExit(command)
            } else {
                ItemAction::ShellCommand(command)
            },
            require_sub_item: true,
            sub_items: state.next_sub_items,
        };

        if !self.handle_required_sub_items(parent.clone()) {
            self.open_sub_items_for_parent(parent);
        }
    }

    fn cancel_compose_mode(&mut self) {
        let Some(state) = self.compose_state.take() else {
            return;
        };

        self.input = state.previous_query;
        self.cursor = state.previous_cursor.min(self.input.len());
        self.recompute_rankings();
        self.status = "Compose cancelled".to_string();
    }

    fn compose_command(&self, config: &ShellCommandWithFlag, input_value: &str) -> String {
        let trimmed = input_value.trim();
        if trimmed.is_empty() {
            return config.command.clone();
        }

        format!("{} {}{}", config.command, config.flag_prefix, trimmed)
    }

    fn load_provider_by_name(&mut self, provider_name: &str) {
        let args = vec![format!("--{}", provider_name)];
        let load_report = load_all_items_from_args(&args);

        self.items = load_report.items;
        self.rejected_items = load_report.rejected.len();
        self.pending_installer_lookup = None;
        self.last_installer_lookup_query = None;
        self.recompute_rankings();

        if self.items.is_empty() {
            self.status = format!("Provider '{}' returned no items", provider_name);
        } else {
            self.status = format!("Loaded provider '{}' ({} items)", provider_name, self.items.len());
        }
    }

    // This is the "open the next menu layer" step.
    //
    // If the selected item has child options, then the user is not finished yet. We open a temporary
    // submenu so they can pick one more action before the final command is launched.
    // For path-launcher items, we may discover flag choices dynamically at this moment and build them on the fly.
    fn try_open_sub_items_view(&mut self) -> bool {
        let Some(parent) = self.selected_item().cloned() else {
            return false;
        };

        if parent.sub_items.is_empty() && parent.provider == "path-launcher" {
            let discovered = discover_path_sub_items(&parent.title);
            if discovered.is_empty() {
                self.status = format!("No sub-items discovered for '{}'", parent.title);
                return true;
            }

            return self.open_sub_items_for_parent(AppItem {
                sub_items: discovered,
                ..parent
            });
        }

        if parent.sub_items.is_empty() {
            return false;
        }

        if parent.require_sub_item && parent.sub_items.len() == 1 {
            self.handle_required_sub_items(parent);
            return true;
        }

        self.open_sub_items_for_parent(parent)
    }

    // A parent item can have child choices. This method turns those children into a
    // temporary new list so the user can pick one more step before launching the final command.
    //
    // The app saves the old list in `view_stack`, switches to the child list, and then
    // later returns to the old one when the user hits Esc.
    // This is how nested menus are built without losing the main search result list.
    //
    // Step by step:
    // 1. turn each child sub-item into a real AppItem that includes the combined action
    // 2. save the current main list as the previous view so we can go back later
    // 3. replace the current list with the submenu
    // 4. clear the search box because we are now browsing the child items, not the parent list
    // 5. recalculate rankings for the new submenu and show a message that tells the user how to go back
    fn open_sub_items_for_parent(&mut self, parent: AppItem) -> bool {
        let mut sub_items = Vec::with_capacity(parent.sub_items.len());
        for sub in &parent.sub_items {
            let Some(item) = self.build_sub_item_from_parent(&parent, sub) else {
                self.status = "Sub-items are only supported for shell command actions".to_string();
                return true;
            };
            sub_items.push(item);
        }

        self.view_stack.push(ViewState {
            items: self.items.clone(),
            input: self.input.clone(),
            cursor: self.cursor,
        });

        self.items = sub_items;
        self.input.clear();
        self.cursor = 0;
        self.info_scroll = 0;
        self.last_click = None;
        self.recompute_rankings();
        self.status = format!("Sub-items for '{}' (Esc to go back)", parent.title);
        true
    }

    // This is the reverse of the submenu open step.
    //
    // We pop the old view off the stack, restore its items and search text, and then re-rank the list.
    // This is how the app goes back to the parent menu without losing the original selection or query.
    fn close_sub_items_view(&mut self) -> bool {
        let Some(previous) = self.view_stack.pop() else {
            return false;
        };

        self.items = previous.items;
        self.input = previous.input;
        self.cursor = previous.cursor.min(self.input.len());
        self.info_scroll = 0;
        self.last_click = None;
        self.recompute_rankings();
        self.status = "Returned to main results".to_string();
        true
    }

    // This is the glue that turns a parent action plus a child option into one new action.
    //
    // The child often means: "add this flag" or "ask for more input". The parent action gives the
    // base command, and the child modifies it. This function makes that combined result into a new item.
    fn build_sub_item_from_parent(&self, parent: &AppItem, sub: &ActionSubItem) -> Option<AppItem> {
        let action = self.apply_sub_item_action(&parent.action, sub)?;
        let flags = if sub.flags.is_empty() {
            "(none)".to_string()
        } else {
            sub.flags.join(" ")
        };

        let subtitle = if sub.subtitle.trim().is_empty() {
            format!("{} [{}]", parent.subtitle, flags)
        } else {
            sub.subtitle.clone()
        };

        let mut fields = parent.info.fields.clone();
        fields.push(InfoField {
            label: "Parent".to_string(),
            value: parent.title.clone(),
        });
        fields.push(InfoField {
            label: "Flags".to_string(),
            value: flags,
        });

        Some(AppItem {
            provider: parent.provider.clone(),
            id: format!("{}::{}", parent.id, sub.id),
            title: sub.title.clone(),
            subtitle,
            info: ItemInfo {
                summary: format!("Sub-item of '{}'.", parent.title),
                fields,
            },
            action,
            require_sub_item: sub.require_sub_item,
            sub_items: sub.sub_items.clone(),
        })
    }

    // This is the part that turns a menu child into the final command behavior.
    //
    // Example:
    // - base action: "echo hello"
    // - sub item: flags = ["--help"]
    // - result: "echo hello --help"
    //
    // If the sub-item also has an input prompt, like "Enter a package name", then the
    // app creates a special "with flag" action that asks the user for the missing text.
    fn apply_sub_item_action(&self, base: &ItemAction, sub: &ActionSubItem) -> Option<ItemAction> {
        let flag_suffix = if sub.flags.is_empty() {
            String::new()
        } else {
            format!(" {}", sub.flags.join(" "))
        };

        match base {
            ItemAction::ShellCommand(command) => {
                let combined = format!("{}{}", command, flag_suffix);
                if let Some(input) = &sub.input {
                    Some(ItemAction::ShellCommandWithFlag(ShellCommandWithFlag {
                        command: combined,
                        flag_prefix: input.flag_prefix.clone(),
                        prompt: input.prompt.clone(),
                        exit_after: sub.exit_after.unwrap_or(false),
                    }))
                } else {
                    if sub.exit_after.unwrap_or(false) {
                        Some(ItemAction::ShellCommandExit(combined))
                    } else {
                        Some(ItemAction::ShellCommand(combined))
                    }
                }
            }
            ItemAction::ShellCommandExit(command) => {
                let combined = format!("{}{}", command, flag_suffix);
                if let Some(input) = &sub.input {
                    Some(ItemAction::ShellCommandWithFlag(ShellCommandWithFlag {
                        command: combined,
                        flag_prefix: input.flag_prefix.clone(),
                        prompt: input.prompt.clone(),
                        exit_after: sub.exit_after.unwrap_or(true),
                    }))
                } else {
                    if sub.exit_after == Some(false) {
                        Some(ItemAction::ShellCommand(combined))
                    } else {
                        Some(ItemAction::ShellCommandExit(combined))
                    }
                }
            }
            ItemAction::ShellCommandWithFlag(config) => {
                let combined = format!("{}{}", config.command, flag_suffix);
                if let Some(input) = &sub.input {
                    Some(ItemAction::ShellCommandWithFlag(ShellCommandWithFlag {
                        command: combined,
                        flag_prefix: input.flag_prefix.clone(),
                        prompt: input.prompt.clone(),
                        exit_after: sub.exit_after.unwrap_or(config.exit_after),
                    }))
                } else {
                    Some(ItemAction::ShellCommandWithFlag(ShellCommandWithFlag {
                        command: combined,
                        flag_prefix: config.flag_prefix.clone(),
                        prompt: config.prompt.clone(),
                        exit_after: sub.exit_after.unwrap_or(config.exit_after),
                    }))
                }
            }
            ItemAction::ProviderHint(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::models::{
        ActionSubItem, AppItem, InfoField, ItemAction, ItemInfo, ProviderItem, SubItemInput,
    };

    // These helper functions are not the app logic itself.
    // They are test-building tools that create small fake items with the same structure
    // that the real provider data uses.
    //
    // The purpose is simple: each test wants a tiny, predictable item so it can check
    // one behavior without depending on real files, real shell commands, or a real provider.
    //
    // In other words: the helpers make the tests easy to read and easy to repeat.
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
            require_sub_item: false,
            sub_items: vec![],
        };

        AppItem::from_provider_item("test", provider_item).expect("valid test item")
    }

    // This helper is almost the same as the one above, but it adds a provider name.
    //
    // Some tests need to check behavior that is specific to one provider, like the
    // installer provider. By tagging the item with a provider name, we can simulate
    // real app data without loading any real provider files.
    fn test_item_with_provider(provider: &str, title: &str, command: &str) -> AppItem {
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
            require_sub_item: false,
            sub_items: vec![],
        };

        AppItem::from_provider_item(provider, provider_item).expect("valid test item")
    }

    // This test checks the special installer mode.
    //
    // The app keeps the installer results hidden until the user types a real package query.
    // That is why the ranked list starts empty even though the app was given items.
    //
    // The test proves two things:
    // 1. the results are hidden before the user types anything
    // 2. once the user types a letter like "f", the filter is ready to show matches
    #[test]
    fn installer_provider_hides_results_until_query_is_typed() {
        let mut app = AppState::new(
            vec![
                test_item_with_provider("installer", "firefox", "echo firefox"),
                test_item_with_provider("installer", "vlc", "echo vlc"),
            ],
            0,
        );

        assert!(app.ranked.is_empty());
        assert_eq!(app.status, "Type a package name to search");

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('f')));
        assert!(!app.ranked.is_empty());
    }

    // This test checks a very common user behavior:
    // the user is looking at a list, then types a letter that should move the best match
    // to the top of the ranked list.
    //
    // We force the selection to a lower slot first, then type a key. If the app is working,
    // the new ranking will snap the selection back to the most relevant result.
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

    // This test checks that typing a letter into an empty query adds that letter to the input,
    // instead of treating the key as a quit command or doing something else.
    //
    // It makes sure the app behaves like a normal text field when the user is searching.
    #[test]
    fn q_is_inserted_in_an_empty_query() {
        let mut app = AppState::new(vec![test_item("Notes", "echo notes")], 0);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('q')));

        assert_eq!(app.input, "q");
        assert!(!app.should_quit);
    }

    // This test covers a parent item that requires a child value before it can finish.
    //
    // The app should not launch the command immediately. Instead, it should open the special
    // "flag input" mode because there is exactly one required sub-item and it expects user text.
    #[test]
    fn enter_on_required_parent_opens_input_for_single_sub_item() {
        let provider_item = ProviderItem {
            id: "parent".to_string(),
            title: "Parent".to_string(),
            subtitle: "Requires sub-item".to_string(),
            info: ItemInfo {
                summary: "summary".to_string(),
                fields: vec![],
            },
            action: ItemAction::ShellCommandExit("demo".to_string()),
            require_sub_item: true,
            sub_items: vec![ActionSubItem {
                id: "name".to_string(),
                title: "Name".to_string(),
                subtitle: "Input name".to_string(),
                flags: vec![],
                exit_after: Some(true),
                require_sub_item: false,
                input: Some(SubItemInput {
                    flag_prefix: "name=".to_string(),
                    prompt: "name".to_string(),
                }),
                sub_items: vec![],
            }],
        };

        let item = AppItem::from_provider_item("test", provider_item).expect("valid item");
        let mut app = AppState::new(vec![item], 0);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Enter));

        assert_eq!(app.input_title(), " Flag Input ");
    }

    // This test checks the same idea as the one above, but this time the user presses Space
    // instead of Enter.
    //
    // In a required single-step flow, the app should still skip the submenu and jump straight
    // into the prompt, because there is no real choice to make. The user only needs to provide
    // the missing input value.
    #[test]
    fn space_on_required_parent_skips_single_sub_item_menu() {
        let provider_item = ProviderItem {
            id: "parent".to_string(),
            title: "Parent".to_string(),
            subtitle: "Requires sub-item".to_string(),
            info: ItemInfo {
                summary: "summary".to_string(),
                fields: vec![],
            },
            action: ItemAction::ShellCommandExit("demo".to_string()),
            require_sub_item: true,
            sub_items: vec![ActionSubItem {
                id: "name".to_string(),
                title: "Name".to_string(),
                subtitle: "Input name".to_string(),
                flags: vec![],
                exit_after: Some(true),
                require_sub_item: false,
                input: Some(SubItemInput {
                    flag_prefix: "name=".to_string(),
                    prompt: "name".to_string(),
                }),
                sub_items: vec![],
            }],
        };

        let item = AppItem::from_provider_item("test", provider_item).expect("valid item");
        let mut app = AppState::new(vec![item], 0);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(' ')));

        assert_eq!(app.input_title(), " Flag Input ");
        assert!(app.view_stack.is_empty());
    }

    // This test proves that nested required single-choice steps are also skipped.
    //
    // The parent item has a required sub-item, and that sub-item itself has a required child.
    // Even though there are multiple layers, the user should not be forced to click through a
    // menu for the only available option. The app should still jump straight to the prompt.
    #[test]
    fn space_skips_nested_required_single_sub_item_menus() {
        let provider_item = ProviderItem {
            id: "parent".to_string(),
            title: "Parent".to_string(),
            subtitle: "Requires nested sub-items".to_string(),
            info: ItemInfo {
                summary: "summary".to_string(),
                fields: vec![],
            },
            action: ItemAction::ShellCommandExit("demo".to_string()),
            require_sub_item: true,
            sub_items: vec![ActionSubItem {
                id: "level1".to_string(),
                title: "Level 1".to_string(),
                subtitle: "Only choice".to_string(),
                flags: vec!["--one".to_string()],
                exit_after: Some(true),
                require_sub_item: true,
                input: None,
                sub_items: vec![ActionSubItem {
                    id: "level2".to_string(),
                    title: "Level 2".to_string(),
                    subtitle: "Input value".to_string(),
                    flags: vec![],
                    exit_after: Some(true),
                    require_sub_item: false,
                    input: Some(SubItemInput {
                        flag_prefix: "name=".to_string(),
                        prompt: "name".to_string(),
                    }),
                    sub_items: vec![],
                }],
            }],
        };

        let item = AppItem::from_provider_item("test", provider_item).expect("valid item");
        let mut app = AppState::new(vec![item], 0);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(' ')));

        assert_eq!(app.input_title(), " Flag Input ");
        assert!(app.view_stack.is_empty());
    }

    // This is the most layered version of the same behavior.
    //
    // A command starts in compose mode. The user types one value, then presses Enter.
    // The app must recognize that the nested required single-choice chain should still be skipped,
    // and it should stay in the input prompt without opening a submenu for the intermediate layer.
    #[test]
    fn enter_after_compose_input_skips_nested_required_single_sub_item_menu() {
        let provider_item = ProviderItem {
            id: "parent".to_string(),
            title: "Parent".to_string(),
            subtitle: "Prompt then nested required sub-items".to_string(),
            info: ItemInfo {
                summary: "summary".to_string(),
                fields: vec![],
            },
            action: ItemAction::ShellCommandWithFlag(crate::models::ShellCommandWithFlag {
                command: "demo".to_string(),
                flag_prefix: "name=".to_string(),
                prompt: "name".to_string(),
                exit_after: true,
            }),
            require_sub_item: true,
            sub_items: vec![ActionSubItem {
                id: "level1".to_string(),
                title: "Level 1".to_string(),
                subtitle: "Only choice".to_string(),
                flags: vec!["--one".to_string()],
                exit_after: Some(true),
                require_sub_item: true,
                input: None,
                sub_items: vec![ActionSubItem {
                    id: "level2".to_string(),
                    title: "Level 2".to_string(),
                    subtitle: "Input value".to_string(),
                    flags: vec![],
                    exit_after: Some(true),
                    require_sub_item: false,
                    input: Some(SubItemInput {
                        flag_prefix: "icon=".to_string(),
                        prompt: "icon".to_string(),
                    }),
                    sub_items: vec![],
                }],
            }],
        };

        let item = AppItem::from_provider_item("test", provider_item).expect("valid item");
        let mut app = AppState::new(vec![item], 0);

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(' ')));
        assert_eq!(app.input_title(), " Flag Input ");

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('x')));
        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Enter));

        assert_eq!(app.input_title(), " Flag Input ");
        assert!(app.view_stack.is_empty());
        assert_eq!(app.input, "");
    }
}
