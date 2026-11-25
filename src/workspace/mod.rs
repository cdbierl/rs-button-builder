//! Terminal sessions, pane membership, selection, and process scheduling.

mod panes;

use super::ui;
use crate::layout_config::{ButtonConfig, TerminalStyle};
use crate::terminal::{Terminal, Wake, screen::Screen};
pub(super) use panes::PaneNode;
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
    pub(super) line_wrap: bool,
    pub(super) paused_screen: Option<Screen>,
    pub(super) search: ui::TerminalSearch,
    pub(super) horizontal_scroll: f32,
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
        let was_running = self.terminal.is_running();
        let pid = self.terminal.pid();
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.poll();
        if was_running && !self.terminal.is_running() {
            log::info!(
                "Terminal exited: tab={} label={:?} pid={} status={}",
                self.id,
                self.label,
                pid.unwrap_or_default(),
                self.terminal.status
            );
        }
        self.update_view(previous);
    }

    pub(super) fn resize(&mut self, cols: usize, rows: usize) {
        let previous = self.terminal.screen.scrolled_lines;
        self.terminal.resize(cols, rows);
        self.update_view(previous);
    }

    pub(super) fn set_line_wrap(&mut self, enabled: bool) {
        log::debug!(
            "Terminal line wrap changed: tab={} label={:?} enabled={enabled}",
            self.id,
            self.label
        );
        self.line_wrap = enabled;
        self.horizontal_scroll = 0.0;
        self.scrollback = 0;
        self.selection.clear();
    }

    pub(super) fn display_paused(&self) -> bool {
        self.paused_screen.is_some()
    }

    pub(super) fn open_search(&mut self) {
        if !self.search.open {
            let selection = self.selection.text(self.screen());
            self.search.prefill_selection(selection.as_deref());
            self.search.resume_on_close = !self.display_paused();
            if self.search.resume_on_close {
                self.paused_screen = Some(self.terminal.screen.clone());
                self.scrollback = 0;
                self.scroll_fraction = 0.0;
                self.selection.clear();
            }
            self.search.open = true;
            let screen = self.paused_screen.as_ref().unwrap_or(&self.terminal.screen);
            self.search.rebuild(screen);
            log::debug!(
                "Terminal find opened: tab={} label={:?}",
                self.id,
                self.label
            );
        }
        self.search.request_focus = true;
    }

    pub(super) fn close_search(&mut self) {
        if !self.search.open {
            return;
        }
        self.search.open = false;
        self.search.request_focus = false;
        if self.search.resume_on_close {
            self.paused_screen = None;
            self.scrollback = 0;
            self.scroll_fraction = 0.0;
        }
        self.search.resume_on_close = false;
        log::debug!(
            "Terminal find closed: tab={} label={:?}",
            self.id,
            self.label
        );
    }

    pub(super) fn set_display_paused(&mut self, paused: bool) {
        if self.search.open && !paused {
            self.search.resume_on_close = false;
            self.search.open = false;
        }
        log::debug!(
            "Terminal display updates changed: tab={} label={:?} paused={paused}",
            self.id,
            self.label
        );
        self.paused_screen = paused.then(|| self.terminal.screen.clone());
        self.scrollback = 0;
        self.scroll_fraction = 0.0;
        self.selection.clear();
    }

    pub(super) fn screen(&self) -> &Screen {
        self.paused_screen.as_ref().unwrap_or(&self.terminal.screen)
    }

    fn update_view(&mut self, previous_scrolled_lines: usize) {
        if self.display_paused() {
            return;
        }
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
            .text(self.screen())
            .unwrap_or_else(|| self.screen().text())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Split {
    Right,
    Below,
}

pub(super) enum PaneAction {
    Split { tab: u64, direction: Split },
    Move { tab: u64, pane: usize },
    Merge,
    Close { tab: u64 },
}

pub(super) struct TerminalWorkspace {
    tabs: Vec<TerminalTab>,
    layout: PaneNode,
    focused_pane: usize,
    next_pane_id: usize,
    next_divider_id: usize,
    pane_actions: Vec<PaneAction>,
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
            layout: PaneNode::leaf(0, None),
            focused_pane: 0,
            next_pane_id: 1,
            next_divider_id: 1,
            pane_actions: Vec::new(),
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
        self.focused_pane
    }

    pub(super) fn layout(&self) -> &PaneNode {
        &self.layout
    }

    pub(super) fn has_pane(&self, pane: usize) -> bool {
        self.layout.pane(pane).is_some()
    }

    pub(super) fn pane_ids(&self) -> Vec<usize> {
        self.layout.ids()
    }

    pub(super) fn split(&self) -> Option<Split> {
        match self.layout {
            PaneNode::Split { direction, .. } => Some(direction),
            PaneNode::Leaf(_) => None,
        }
    }

    pub(super) fn set_split_fraction(&mut self, divider: usize, fraction: f32) {
        self.layout.set_fraction(divider, fraction);
    }

    pub(super) fn queue_pane_action(&mut self, action: PaneAction) {
        self.pane_actions.push(action);
    }

    // Apply structural changes after all panes have rendered from the same layout.
    pub(super) fn apply_pane_actions(&mut self) -> bool {
        let actions = std::mem::take(&mut self.pane_actions);
        let changed = !actions.is_empty();
        for action in actions {
            match action {
                PaneAction::Split { tab, direction } => {
                    if let Some(index) = self.tabs.iter().position(|t| t.id == tab) {
                        self.split_terminal(index, direction);
                    }
                }
                PaneAction::Move { tab, pane } => {
                    if let Some(index) = self.tabs.iter().position(|t| t.id == tab) {
                        self.move_terminal(index, pane);
                    }
                }
                PaneAction::Merge => self.merge_terminals(),
                PaneAction::Close { tab } => {
                    if let Some(index) = self.tabs.iter().position(|terminal| terminal.id == tab) {
                        log::info!(
                            "Terminal close requested from tab menu: tab={} label={:?} running={}",
                            tab,
                            self.tabs[index].label,
                            self.tabs[index].terminal.is_running()
                        );
                        self.close_tab(index);
                    }
                }
            }
        }
        changed
    }

    pub(super) fn focus_requested(&self, pane: usize) -> bool {
        self.focus_terminal && self.focused_pane == pane
    }

    pub(super) fn acknowledge_focus(&mut self) {
        self.focus_terminal = false;
    }

    pub(super) fn reveal_requested(&self, pane: usize) -> bool {
        self.reveal_active_tab && self.focused_pane == pane
    }

    pub(super) fn acknowledge_tab_reveal(&mut self, pane: usize) {
        if self.focused_pane == pane {
            self.reveal_active_tab = false;
        }
    }

    pub(super) fn resize_target(&mut self, pane: usize, cols: usize, rows: usize) {
        if self.focused_pane == pane {
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
        log::debug!(
            "Terminal selected: tab={} label={:?} pane={}",
            tab.id,
            tab.label,
            tab.pane
        );
        self.focused_pane = tab.pane;
        self.layout.pane_mut(tab.pane).unwrap().selected = Some(tab.id);
        self.reveal_active_tab = true;
        self.focus_terminal = true;
    }

    pub(super) fn close_tab(&mut self, index: usize) {
        let active_closed = self.active_tab == Some(index);
        let pane = self.tabs[index].pane;
        log::debug!(
            "Terminal closed: tab={} label={:?} pane={} running={}",
            self.tabs[index].id,
            self.tabs[index].label,
            pane,
            self.tabs[index].terminal.is_running()
        );
        let pane_order = self.pane_ids();
        // Dropping a terminal requests termination; reaping happens off the UI thread.
        self.tabs.remove(index);
        self.active_tab = match self.active_tab {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            active => active,
        };
        if active_closed {
            let position = pane_order.iter().position(|&id| id == pane).unwrap();
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
                .or_else(|| {
                    pane_order[position + 1..]
                        .iter()
                        .chain(pane_order[..position].iter().rev())
                        .find_map(|&pane| self.pane_tab(pane))
                });
            self.focus_terminal = true;
            self.reveal_active_tab = true;
        }
        self.normalize_panes();
    }

    pub(super) fn pane_tab(&self, pane: usize) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| {
                tab.pane == pane && Some(tab.id) == self.layout.pane(pane).and_then(|p| p.selected)
            })
            .or_else(|| self.tabs.iter().position(|tab| tab.pane == pane))
    }

    pub(super) fn focus_pane(&mut self, pane: usize) {
        if let Some(index) = self.pane_tab(pane) {
            self.select_tab(index);
        } else if self.has_pane(pane) {
            self.focused_pane = pane;
            self.active_tab = None;
        }
    }

    pub(super) fn can_split(&self, index: usize) -> bool {
        let pane = self.tabs[index].pane;
        self.tabs.iter().filter(|tab| tab.pane == pane).count() >= 2
    }

    pub(super) fn split_terminal(&mut self, index: usize, split: Split) {
        if !self.can_split(index) {
            return;
        }
        let pane = self.tabs[index].pane;
        let remaining = self
            .pane_tab(pane)
            .filter(|&other| other != index)
            .or_else(|| {
                self.tabs
                    .iter()
                    .position(|tab| tab.pane == pane && tab.id != self.tabs[index].id)
            })
            .unwrap();
        let new_pane = self.next_pane_id;
        if !self
            .layout
            .split_pane(pane, new_pane, self.next_divider_id, split)
        {
            return;
        }
        self.next_pane_id += 1;
        self.next_divider_id += 1;
        self.tabs[index].pane = new_pane;
        log::debug!(
            "Terminal pane split: tab={} direction={split:?} source_pane={pane} new_pane={new_pane}",
            self.tabs[index].id
        );
        self.layout.pane_mut(pane).unwrap().selected = Some(self.tabs[remaining].id);
        self.select_tab(index);
    }

    pub(super) fn move_terminal(&mut self, index: usize, pane: usize) {
        if self.layout.pane(pane).is_none() || self.tabs[index].pane == pane {
            return;
        }
        let source = self.tabs[index].pane;
        log::debug!(
            "Terminal moved: tab={} source_pane={source} destination_pane={pane}",
            self.tabs[index].id
        );
        self.tabs[index].pane = pane;
        self.select_tab(index);
        self.normalize_panes();
    }

    pub(super) fn merge_terminals(&mut self) {
        log::debug!(
            "Terminal panes merged: pane_count={}",
            self.pane_ids().len()
        );
        for tab in &mut self.tabs {
            tab.pane = 0;
        }
        self.layout = PaneNode::leaf(0, self.active_tab.map(|index| self.tabs[index].id));
        self.focused_pane = 0;
        self.focus_terminal = true;
        self.reveal_active_tab = true;
    }

    fn normalize_panes(&mut self) {
        let layout = std::mem::replace(&mut self.layout, PaneNode::leaf(0, None));
        self.layout = layout
            .retain(&|pane| self.tabs.iter().any(|tab| tab.pane == pane))
            .unwrap_or_else(|| PaneNode::leaf(0, None));
        for pane in self.pane_ids() {
            let selected = self.pane_tab(pane).map(|index| self.tabs[index].id);
            self.layout.pane_mut(pane).unwrap().selected = selected;
        }
        if let Some(index) = self.active_tab {
            self.focused_pane = self.tabs[index].pane;
            self.layout.pane_mut(self.focused_pane).unwrap().selected = Some(self.tabs[index].id);
        } else {
            self.focused_pane = self.pane_ids()[0];
        }
    }

    pub(super) fn focus_running_button(&mut self, index: usize) -> bool {
        let matches =
            |tab: &TerminalTab| tab.button_index == Some(index) && tab.terminal.is_running();
        let tab = self
            .active_tab
            .filter(|&tab| matches(&self.tabs[tab]))
            .or_else(|| self.tabs.iter().rposition(matches));
        if let Some(tab) = tab {
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
        let pid = terminal
            .pid()
            .expect("a newly started terminal has a child");
        terminal.screen.preserve_long_lines();
        terminal
            .watch(self.terminal_wake.clone())
            .map_err(|error| format!("Could not watch terminal: {error}"))?;
        let tab_id = self.next_tab_id;
        let pane = self.focused_pane;
        self.tabs.push(TerminalTab {
            id: tab_id,
            pane,
            label: button.label.clone(),
            command: button.command.clone(),
            button_index: Some(index),
            terminal,
            style: style.clone(),
            scrollback: 0,
            scroll_fraction: 0.0,
            line_wrap: false,
            paused_screen: None,
            search: ui::TerminalSearch::default(),
            horizontal_scroll: 0.0,
            selection: ui::TerminalSelection::default(),
            text_cache: ui::TerminalTextCache::default(),
        });
        log::info!(
            "Terminal started: tab={tab_id} label={:?} button={} pane={pane} pid={pid} logging={}",
            button.label,
            index,
            button.log_output
        );
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
            .filter_map(|(index, tab)| (tab.pane == self.focused_pane).then_some(index))
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

    pub(super) fn open_search(&mut self) {
        if let Some(index) = self.active_tab {
            self.tabs[index].open_search();
            self.focus_terminal = false;
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
            if !self.tabs[index].display_paused() && self.tabs[index].terminal.should_auto_close() {
                log::info!(
                    "Terminal auto-closed after success: tab={} label={:?}",
                    self.tabs[index].id,
                    self.tabs[index].label
                );
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
