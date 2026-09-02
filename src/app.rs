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
    pending_pkg_lookup: Option<PendingPkgLookup>,
    last_pkg_lookup_query: Option<String>,
}

impl AppState {
    pub fn new(items: Vec<AppItem>, rejected_items: usize) -> Self {
        let is_pkg_manager_only = !items.is_empty() && items.iter().all(|item| item.provider == "pkg-manager");
        let ranked = if is_pkg_manager_only {
            Vec::new()
        } else {
            rank_items("", &items)
        };
        let status = if items.is_empty() {
            "No items loaded".to_string()
        } else if is_pkg_manager_only {
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
            pending_pkg_lookup: None,
            last_pkg_lookup_query: None,
        }
    }

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
            KeyCode::Char('q') if self.input.is_empty() => self.should_quit = true,
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
                if self.try_force_pkg_lookup() {
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
        self.process_debounced_pkg_lookup();
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

    fn recompute_rankings(&mut self) {
        if self.input.is_empty() && self.is_pkg_manager_only() {
            self.ranked.clear();
            self.selected = 0;
            self.info_scroll = 0;
            self.last_click = None;
            self.pending_pkg_lookup = None;
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

        self.schedule_pkg_lookup();
    }

    fn is_pkg_manager_only(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|item| item.provider == "pkg-manager")
    }

    fn pkg_lookup_debounce(query: &str) -> Duration {
        if query.chars().count() <= 3 {
            Duration::from_millis(180)
        } else {
            Duration::from_millis(120)
        }
    }

    fn schedule_pkg_lookup(&mut self) {
        if !self.is_pkg_manager_only() {
            self.pending_pkg_lookup = None;
            return;
        }

        let query = self.input.trim();
        if query.chars().count() < 2 {
            self.pending_pkg_lookup = None;
            return;
        }

        if self.last_pkg_lookup_query.as_deref() == Some(query) {
            return;
        }

        self.pending_pkg_lookup = Some(PendingPkgLookup {
            query: query.to_string(),
            due_at: Instant::now() + Self::pkg_lookup_debounce(query),
        });
    }

    fn process_debounced_pkg_lookup(&mut self) {
        let should_run = self
            .pending_pkg_lookup
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.due_at);

        if !should_run {
            return;
        }

        let Some(pending) = self.pending_pkg_lookup.take() else {
            return;
        };

        if self.input.trim() != pending.query {
            return;
        }

        if self.last_pkg_lookup_query.as_deref() == Some(pending.query.as_str()) {
            return;
        }

        self.lookup_pkg_manager_query(&pending.query);
    }

    fn try_force_pkg_lookup(&mut self) -> bool {
        if !self.is_pkg_manager_only() {
            return false;
        }

        let query = self.input.trim().to_string();
        if query.chars().count() < 2 {
            return false;
        }

        if self.last_pkg_lookup_query.as_deref() == Some(query.as_str()) {
            return false;
        }

        self.pending_pkg_lookup = None;
        self.lookup_pkg_manager_query(&query);
        true
    }

    fn lookup_pkg_manager_query(&mut self, query: &str) {
        let args = vec!["--pkg-manager".to_string()];
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
        self.last_pkg_lookup_query = Some(query.to_string());
        self.recompute_rankings();
    }

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
        self.pending_pkg_lookup = None;
        self.last_pkg_lookup_query = None;
        self.recompute_rankings();

        if self.items.is_empty() {
            self.status = format!("Provider '{}' returned no items", provider_name);
        } else {
            self.status = format!("Loaded provider '{}' ({} items)", provider_name, self.items.len());
        }
    }

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

    #[test]
    fn pkg_manager_provider_hides_results_until_query_is_typed() {
        let mut app = AppState::new(
            vec![
                test_item_with_provider("pkg-manager", "firefox", "echo firefox"),
                test_item_with_provider("pkg-manager", "vlc", "echo vlc"),
            ],
            0,
        );

        assert!(app.ranked.is_empty());
        assert_eq!(app.status, "Type a package name to search");

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('f')));
        assert!(!app.ranked.is_empty());
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
