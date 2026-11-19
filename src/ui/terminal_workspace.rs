//! Nested terminal geometry, divider interaction, and viewport composition.
use super::super::workspace::{PaneNode, Split};
use crate::app::ButtonBuilder;
use egui::{Rect, Sense, Stroke, UiBuilder, Vec2};
use egui_material3::get_global_color;

pub(super) const DIVIDER_WIDTH: f32 = 8.0;
pub(super) const MIN_PANE_SIZE: Vec2 = Vec2::new(240.0, 220.0);
pub(super) const PANE_PADDING: f32 = 12.0;

struct Divider {
    id: usize,
    direction: Split,
    rect: Rect,
    region: Rect,
    limits: (f32, f32),
}

#[derive(Default)]
struct Geometry {
    panes: Vec<(usize, Rect)>,
    dividers: Vec<Divider>,
}

fn minimum_size(node: &PaneNode) -> Vec2 {
    match node {
        PaneNode::Leaf(_) => MIN_PANE_SIZE,
        PaneNode::Split {
            direction,
            children,
            ..
        } => {
            let first = minimum_size(&children[0]);
            let second = minimum_size(&children[1]);
            match direction {
                Split::Right => {
                    Vec2::new(first.x + DIVIDER_WIDTH + second.x, first.y.max(second.y))
                }
                Split::Below => {
                    Vec2::new(first.x.max(second.x), first.y + DIVIDER_WIDTH + second.y)
                }
            }
        }
    }
}

fn geometry(node: &PaneNode, region: Rect, output: &mut Geometry) {
    match node {
        PaneNode::Leaf(pane) => output.panes.push((pane.id, region)),
        PaneNode::Split {
            id,
            direction,
            fraction,
            children,
        } => {
            let horizontal = *direction == Split::Right;
            let span = if horizontal {
                region.width()
            } else {
                region.height()
            };
            let usable = (span - DIVIDER_WIDTH).max(0.0);
            let first = minimum_size(&children[0]);
            let second = minimum_size(&children[1]);
            let (first, second) = if horizontal {
                (first.x, second.x)
            } else {
                (first.y, second.y)
            };
            // When the window is too small, distribute space proportionally. Retain
            // the preferred ratio so enlarging the window restores the user's layout.
            let limits = if usable >= first + second {
                (first / usable, 1.0 - second / usable)
            } else {
                let ratio = first / (first + second);
                (ratio, ratio)
            };
            let at = usable * fraction.clamp(limits.0, limits.1);
            let gutter = DIVIDER_WIDTH.min(span.max(0.0));
            let divider = if horizontal {
                Rect::from_min_size(
                    region.min + Vec2::new(at, 0.0),
                    Vec2::new(gutter, region.height()),
                )
            } else {
                Rect::from_min_size(
                    region.min + Vec2::new(0.0, at),
                    Vec2::new(region.width(), gutter),
                )
            };
            let regions = if horizontal {
                [
                    Rect::from_min_max(region.min, egui::pos2(divider.left(), region.bottom())),
                    Rect::from_min_max(egui::pos2(divider.right(), region.top()), region.max),
                ]
            } else {
                [
                    Rect::from_min_max(region.min, egui::pos2(region.right(), divider.top())),
                    Rect::from_min_max(egui::pos2(region.left(), divider.bottom()), region.max),
                ]
            };
            output.dividers.push(Divider {
                id: *id,
                direction: *direction,
                rect: divider,
                region,
                limits,
            });
            for (child, region) in children.iter().zip(regions) {
                geometry(child, region, output);
            }
        }
    }
}

impl ButtonBuilder {
    pub(in crate::app) fn terminal_workspace(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let mut layout = Geometry::default();
        geometry(self.workspace.layout(), rect, &mut layout);
        let divided = layout.panes.len() > 1;
        for divider in layout.dividers {
            let horizontal = divider.direction == Split::Right;
            let response = ui.interact(
                divider.rect,
                ui.id().with(("terminal_divider", divider.id)),
                Sense::drag(),
            );
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(if horizontal {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let span = if horizontal {
                    divider.region.width()
                } else {
                    divider.region.height()
                };
                let offset = if horizontal {
                    pointer.x - divider.region.left()
                } else {
                    pointer.y - divider.region.top()
                };
                let fraction = ((offset - DIVIDER_WIDTH / 2.0) / (span - DIVIDER_WIDTH).max(1.0))
                    .clamp(divider.limits.0, divider.limits.1);
                self.workspace.set_split_fraction(divider.id, fraction);
                ui.ctx().request_repaint();
            }
            let color = get_global_color(if response.hovered() || response.dragged() {
                "primary"
            } else {
                "outlineVariant"
            });
            ui.painter().line_segment(
                if horizontal {
                    [divider.rect.center_top(), divider.rect.center_bottom()]
                } else {
                    [divider.rect.left_center(), divider.rect.right_center()]
                },
                Stroke::new(2.0_f32, color),
            );
        }
        for (pane, region) in layout.panes {
            // A toolbar can close its last tab during this pass. Skip removed leaves;
            // queued split/move/merge actions are applied after rendering the workspace.
            if !self.workspace.has_pane(pane) {
                continue;
            }
            let padding = if divided {
                PANE_PADDING
                    .min(region.width() / 2.0)
                    .min(region.height() / 2.0)
            } else {
                0.0
            };
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("terminal_region", pane))
                    .max_rect(region.shrink(padding)),
            );
            child.set_clip_rect(ui.clip_rect().intersect(region));
            if child.rect_contains_pointer(region)
                && child.input(|input| input.pointer.any_pressed())
            {
                self.workspace.focus_pane(pane);
            }
            self.terminal_panel(&mut child, pane);
            if divided && self.workspace.focused_pane() == pane {
                ui.painter().rect_stroke(
                    region,
                    super::CORNER_RADIUS,
                    Stroke::new(2.0_f32, get_global_color("primary")),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if self.workspace.apply_pane_actions() {
            ui.ctx().request_repaint();
        }
        ui.advance_cursor_after_rect(rect);
    }
}
