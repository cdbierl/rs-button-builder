//! Terminal sessions, pane membership, selection, and process scheduling.

use super::ui;
use crate::layout_config::{ButtonConfig, TerminalStyle};
use crate::terminal::{Terminal, Wake};
use std::time::{Duration, Instant};

pub(super) struct TerminalTab {
    id: u64,
    pane: usize,
    pub(super) label: String,
    pub(super) command: String,
    pub(super) button_index: Option<usize>,
    pub(super) terminal: Terminal,
    pub(super) style: TerminalStyle,
    pub(super) scrollback: usize,
    pub(super) scroll_fraction: f32,
    pub(super) selection: ui::TerminalSelection,
    pub(super) text_cache: ui::TerminalTextCache,
}

impl TerminalTab {
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn pane(&self) -> usize {
        self.pane
    }

    fn poll(&mut self) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.poll();
        self.update_view(previous);
    }

    pub(super) fn resize(&mut self, cols: usize, rows: usize) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.resize(cols, rows);
        self.update_view(previous);
    }

    fn update_view(&mut self, previous_scrolled_lines: usize) {
        self.selection.prepare(&self.terminal.screen);
        // Output and viewport resizing can both move the viewed text into history.
        if self.scrollback > 0 {
            self.scrollback = self.scrollback.saturating_add(
                self.terminal
                    .screen
                    .scrolled_lines
                    .saturating_sub(previous_scrolled_lines),
            );
        }
    }

    pub(super) fn copy_text(&self) -> String {
        self.selection
            .text(&self.terminal.screen)
            .unwrap_or_else(|| self.terminal.screen.text())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Split {
    Right,
    Below,
}

struct TerminalPanes {
    split: Option<Split>,
    fraction: f32,
    selected: [Option<u64>; 2],
    focused: usize,
}

impl Default for TerminalPanes {
    fn default() -> Self {
        Self {
            split: None,
            fraction: 0.5,
            selected: [None; 2],
            focused: 0,
        }
    }
}

pub(super) struct TerminalWorkspace {
    tabs: Vec<TerminalTab>,
    panes: TerminalPanes,
    active_tab: Option<usize>,
    next_tab_id: u64,
    next_poll_tab: usize,
    terminal_wake: Wake,
    reveal_active_tab: bool,
    focus_terminal: bool,
    terminal_size: (usize, usize),
}

impl TerminalWorkspace {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        Self {
            tabs: Vec::new(),
            panes: TerminalPanes::default(),
            active_tab: None,
            next_tab_id: 1,
            next_poll_tab: 0,
            terminal_wake: std::sync::Arc::new(move || {
                ctx.request_repaint_after(Duration::from_millis(33));
            }),
            reveal_active_tab: false,
            focus_terminal: false,
            terminal_size: (80, 24),
        }
    }

    pub(super) fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    // UI components can update per-tab views without adding or removing sessions.
    // IDs and pane membership remain private to this module.
    pub(super) fn tabs_mut(&mut self) -> &mut [TerminalTab] {
        &mut self.tabs
    }

    pub(super) fn active_index(&self) -> Option<usize> {
        self.active_tab
    }

    pub(super) fn focused_pane(&self) -> usize {
        self.panes.focused
    }

    pub(super) fn split(&self) -> Option<Split> {
        self.panes.split
    }

    pub(super) fn split_fraction(&self) -> f32 {
        self.panes.fraction
    }

    pub(super) fn set_split_fraction(&mut self, fraction: f32) {
        self.panes.fraction = fraction;
    }

    pub(super) fn focus_requested(&self, pane: usize) -> bool {
        self.focus_terminal && self.panes.focused == pane
    }

    pub(super) fn acknowledge_focus(&mut self) {
        self.focus_terminal = false;
    }

    pub(super) fn reveal_requested(&self, pane: usize) -> bool {
        self.reveal_active_tab && self.panes.focused == pane
    }

    pub(super) fn acknowledge_tab_reveal(&mut self, pane: usize) {
        if self.panes.focused == pane {
            self.reveal_active_tab = false;
        }
    }

    pub(super) fn resize_target(&mut self, pane: usize, cols: usize, rows: usize) {
        if self.panes.focused == pane {
            self.terminal_size = (cols, rows);
        }
    }

    #[cfg(test)]
    pub(super) fn terminal_size(&self) -> (usize, usize) {
        self.terminal_size
    }

    #[cfg(test)]
    pub(super) fn next_tab_id(&self) -> u64 {
        self.next_tab_id
    }

    pub(super) fn running(&self) -> bool {
        self.tabs.iter().any(|tab| tab.terminal.is_running())
    }

    pub(super) fn active(&self) -> Option<&TerminalTab> {
        self.active_tab.and_then(|index| self.tabs.get(index))
    }

    pub(super) fn select_tab(&mut self, index: usize) {
        self.active_tab = Some(index);
        let tab = &self.tabs[index];
        self.panes.focused = tab.pane;
        self.panes.selected[tab.pane] = Some(tab.id);
        self.reveal_active_tab = true;
        self.focus_terminal = true;
    }

    pub(super) fn close_tab(&mut self, index: usize) {
        let active_closed = self.active_tab == Some(index);
        let pane = self.tabs[index].pane;
        // Dropping a terminal requests termination; reaping happens off the UI thread.
        self.tabs.remove(index);
        self.active_tab = match self.active_tab {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            active => active,
        };
        if active_closed {
            if self.panes.split.is_some() {
                self.active_tab = self
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(other, tab)| *other >= index && tab.pane == pane)
                    .or_else(|| {
                        self.tabs
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, tab)| tab.pane == pane)
                    })
                    .map(|(index, _)| index)
                    .or(self.active_tab);
            }
            self.focus_terminal = true;
            self.reveal_active_tab = true;
        }
        self.normalize_panes();
    }

    pub(super) fn pane_tab(&self, pane: usize) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.pane == pane && Some(tab.id) == self.panes.selected[pane])
            .or_else(|| self.tabs.iter().position(|tab| tab.pane == pane))
    }

    pub(super) fn focus_pane(&mut self, pane: usize) {
        if let Some(index) = self.pane_tab(pane) {
            self.select_tab(index);
        } else {
            self.panes.focused = pane;
            self.active_tab = None;
        }
    }

    pub(super) fn split_terminal(&mut self, index: usize, split: Split) {
        if self.panes.split.is_some() || self.tabs.len() < 2 {
            return;
        }
        let remaining = self
            .active_tab
            .filter(|&active| active != index)
            .or_else(|| (0..self.tabs.len()).find(|&other| other != index))
            .unwrap();
        self.tabs[index].pane = 1;
        self.panes.split = Some(split);
        self.panes.fraction = 0.5;
        self.panes.selected[0] = Some(self.tabs[remaining].id);
        self.select_tab(index);
    }

    pub(super) fn move_terminal(&mut self, index: usize) {
        if self.panes.split.is_none() {
            return;
        }
        self.tabs[index].pane = 1 - self.tabs[index].pane;
        self.select_tab(index);
        self.normalize_panes();
    }

    pub(super) fn merge_terminals(&mut self) {
        for tab in &mut self.tabs {
            tab.pane = 0;
        }
        self.panes.split = None;
        self.panes.focused = 0;
        self.panes.selected = [self.active_tab.map(|index| self.tabs[index].id), None];
        self.focus_terminal = true;
        self.reveal_active_tab = true;
    }

    fn normalize_panes(&mut self) {
        if self.panes.split.is_some()
            && (0..2).any(|pane| !self.tabs.iter().any(|tab| tab.pane == pane))
        {
            self.merge_terminals();
        }
        for pane in 0..2 {
            self.panes.selected[pane] = self.pane_tab(pane).map(|index| self.tabs[index].id);
        }
        if let Some(index) = self.active_tab {
            self.panes.focused = self.tabs[index].pane;
            self.panes.selected[self.panes.focused] = Some(self.tabs[index].id);
        }
    }

    pub(super) fn focus_running_button(&mut self, index: usize) -> bool {
        if let Some(tab) = self
            .tabs
            .iter()
            .position(|tab| tab.button_index == Some(index) && tab.terminal.is_running())
        {
            self.select_tab(tab);
            true
        } else {
            false
        }
    }

    pub(super) fn start_button(
        &mut self,
        index: usize,
        button: &ButtonConfig,
        style: &TerminalStyle,
    ) -> Result<(), String> {
        let mut terminal = Terminal::start(button, self.terminal_size.0, self.terminal_size.1)?;
        terminal
            .watch(self.terminal_wake.clone())
            .map_err(|error| format!("Could not watch terminal: {error}"))?;
        self.tabs.push(TerminalTab {
            id: self.next_tab_id,
            pane: self.panes.focused,
            label: button.label.clone(),
            command: button.command.clone(),
            button_index: Some(index),
            terminal,
            style: style.clone(),
            scrollback: 0,
            scroll_fraction: 0.0,
            selection: ui::TerminalSelection::default(),
            text_cache: ui::TerminalTextCache::default(),
        });
        self.next_tab_id += 1;
        self.select_tab(self.tabs.len() - 1);
        Ok(())
    }

    pub(super) fn detach_buttons(&mut self) {
        for tab in &mut self.tabs {
            tab.button_index = None;
        }
    }

    pub(super) fn detach_button(&mut self, index: usize) {
        for tab in &mut self.tabs {
            if tab.button_index == Some(index) {
                tab.button_index = None;
            }
        }
    }

    pub(super) fn swap_button_bindings(&mut self, source: usize, target: usize) {
        for tab in &mut self.tabs {
            tab.button_index = tab.button_index.map(|index| {
                if index == source {
                    target
                } else if index == target {
                    source
                } else {
                    index
                }
            });
        }
    }

    pub(super) fn cycle_tab(&mut self, forward: bool) {
        let indices: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| (tab.pane == self.panes.focused).then_some(index))
            .collect();
        if let Some(position) = indices
            .iter()
            .position(|&index| Some(index) == self.active_tab)
        {
            let next = if forward {
                (position + 1) % indices.len()
            } else {
                (position + indices.len() - 1) % indices.len()
            };
            self.select_tab(indices[next]);
        }
    }

    /// Return the last completed label for the application's temporary notice.
    pub(super) fn poll(&mut self, ctx: &egui::Context) -> Option<String> {
        // Share a frame budget and rotate the starting tab so noisy tabs cannot starve others.
        let deadline = Instant::now() + Duration::from_millis(4);
        let count = self.tabs.len();
        for _ in 0..count {
            if Instant::now() >= deadline {
                ctx.request_repaint_after(Duration::from_millis(33));
                break;
            }
            let index = self.next_poll_tab % count;
            self.tabs[index].poll();
            self.next_poll_tab = (index + 1) % count;
        }
        let mut completed = None;
        // Remove only after polling has observed success and drained all terminal output.
        for index in (0..self.tabs.len()).rev() {
            if self.tabs[index].terminal.should_auto_close() {
                completed = Some(self.tabs[index].label.clone());
                self.close_tab(index);
                ctx.request_repaint();
            }
        }
        completed
    }

    pub(super) fn schedule_repaint(&self, ctx: &egui::Context) {
        if self.tabs.iter().any(|tab| tab.terminal.has_pending_work()) {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        if self.tabs.iter().any(|tab| tab.terminal.needs_poll()) {
            // Check exits even when descendants retain the PTY; also recover missed wakeups.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}
