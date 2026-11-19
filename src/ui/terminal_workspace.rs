//! Two terminal panes with a draggable divider and independent tab selections.
use crate::app::ButtonBuilder;
use egui::{Rect, Sense, Stroke, UiBuilder, Vec2};
use egui_material3::get_global_color;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Split {
    Right,
    Below,
}

pub(in crate::app) struct TerminalPanes {
    pub split: Option<Split>,
    pub fraction: f32,
    pub selected: [Option<u64>; 2],
    pub focused: usize,
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

impl ButtonBuilder {
    pub(in crate::app) fn pane_tab(&self, pane: usize) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.pane == pane && Some(tab.id) == self.panes.selected[pane])
            .or_else(|| self.tabs.iter().position(|tab| tab.pane == pane))
    }

    pub(in crate::app) fn focus_pane(&mut self, pane: usize) {
        if let Some(index) = self.pane_tab(pane) {
            self.select_tab(index);
        } else {
            self.panes.focused = pane;
            self.active_tab = None;
        }
    }

    pub(in crate::app) fn split_terminal(&mut self, index: usize, split: Split) {
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

    pub(in crate::app) fn move_terminal(&mut self, index: usize) {
        if self.panes.split.is_none() {
            return;
        }
        self.tabs[index].pane = 1 - self.tabs[index].pane;
        self.select_tab(index);
        self.normalize_panes();
    }

    pub(in crate::app) fn merge_terminals(&mut self) {
        for tab in &mut self.tabs {
            tab.pane = 0;
        }
        self.panes.split = None;
        self.panes.focused = 0;
        self.panes.selected = [self.active_tab.map(|index| self.tabs[index].id), None];
        self.focus_terminal = true;
        self.reveal_active_tab = true;
    }

    pub(in crate::app) fn normalize_panes(&mut self) {
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

    pub(in crate::app) fn terminal_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(split) = self.panes.split else {
            self.terminal_panel(ui, 0);
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let vertical_divider = split == Split::Right;
        let span = if vertical_divider {
            rect.width()
        } else {
            rect.height()
        };
        let usable = (span - 8.0).max(1.0);
        let minimum = (if vertical_divider { 180.0 } else { 150.0 } / usable).min(0.45);
        self.panes.fraction = self.panes.fraction.clamp(minimum, 1.0 - minimum);
        let divider_at = usable * self.panes.fraction;
        let divider = if vertical_divider {
            Rect::from_min_size(
                rect.min + Vec2::new(divider_at, 0.0),
                Vec2::new(8.0, rect.height()),
            )
        } else {
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, divider_at),
                Vec2::new(rect.width(), 8.0),
            )
        };
        let response = ui.interact(divider, ui.id().with("terminal_divider"), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(if vertical_divider {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let offset = if vertical_divider {
                pointer.x - rect.left()
            } else {
                pointer.y - rect.top()
            };
            self.panes.fraction = ((offset - 4.0) / usable).clamp(minimum, 1.0 - minimum);
            ui.ctx().request_repaint();
        }
        let regions = if vertical_divider {
            [
                Rect::from_min_max(rect.min, egui::pos2(divider.left(), rect.bottom())),
                Rect::from_min_max(egui::pos2(divider.right(), rect.top()), rect.max),
            ]
        } else {
            [
                Rect::from_min_max(rect.min, egui::pos2(rect.right(), divider.top())),
                Rect::from_min_max(egui::pos2(rect.left(), divider.bottom()), rect.max),
            ]
        };
        for (pane, region) in regions.into_iter().enumerate() {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("terminal_region", pane))
                    .max_rect(region.shrink(8.0)),
            );
            child.set_clip_rect(ui.clip_rect().intersect(region));
            if child.rect_contains_pointer(region)
                && child.input(|input| input.pointer.any_pressed())
            {
                self.focus_pane(pane);
            }
            self.terminal_panel(&mut child, pane);
            if self.panes.focused == pane {
                ui.painter().rect_stroke(
                    region,
                    super::CORNER_RADIUS,
                    Stroke::new(2.0_f32, get_global_color("primary")),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let color = get_global_color(if response.hovered() || response.dragged() {
            "primary"
        } else {
            "outlineVariant"
        });
        ui.painter().line_segment(
            if vertical_divider {
                [divider.center_top(), divider.center_bottom()]
            } else {
                [divider.left_center(), divider.right_center()]
            },
            Stroke::new(2.0_f32, color),
        );
        ui.advance_cursor_after_rect(rect);
    }
}
