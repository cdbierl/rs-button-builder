//! Per-tab terminal find state and its compact content overlay.

use super::{CORNER_RADIUS, text_button};
use crate::terminal::screen::{Cell, Screen};
use egui::{FontFamily, FontId, RichText, Vec2};
use egui_material3::get_global_color;
use regex::RegexBuilder;
use std::ops::Range;

const MAX_SELECTION_QUERY_CHARS: usize = 120;
const DROP_DOWN_HEIGHT: f32 = 40.0;
const DROP_DOWN_TIME: f32 = 0.14;
const DROP_DOWN_HORIZONTAL_MARGIN: f32 = 16.0;
const DROP_DOWN_TOP_INSET: f32 = 2.0;
const DROP_DOWN_MAX_WIDTH: f32 = 470.0;

#[derive(Clone)]
pub(super) struct SearchMatch {
    pub row: usize,
    pub columns: Range<usize>,
}

#[derive(Default)]
pub(in crate::app) struct TerminalSearch {
    pub(in crate::app) open: bool,
    pub(super) query: String,
    pub(super) case_sensitive: bool,
    pub(super) regex: bool,
    pub(super) matches: Vec<SearchMatch>,
    pub(super) current: Option<usize>,
    pub(super) error: Option<String>,
    pub(in crate::app) request_focus: bool,
    pub(in crate::app) resume_on_close: bool,
}

pub(super) enum SearchAction {
    None,
    Close,
    Navigate(SearchMatch),
}

impl TerminalSearch {
    pub(in crate::app) fn prefill_selection(&mut self, selection: Option<&str>) {
        let Some(selection) = selection.filter(|text| {
            !text.contains('\r')
                && !text.contains('\n')
                && text.chars().count() <= MAX_SELECTION_QUERY_CHARS
        }) else {
            return;
        };
        self.query = selection.to_owned();
        self.regex = false;
        self.current = Some(0);
    }

    pub(in crate::app) fn rebuild(&mut self, screen: &Screen) {
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            self.current = None;
            return;
        }
        let expression = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let matcher = match RegexBuilder::new(&expression)
            .case_insensitive(!self.case_sensitive)
            .build()
        {
            Ok(matcher) => matcher,
            Err(error) => {
                self.error = Some(error.to_string());
                self.current = None;
                return;
            }
        };
        let history = if screen.is_alternate() {
            0
        } else {
            screen.history.len()
        };
        let first = if screen.is_alternate() {
            0
        } else {
            screen.scrolled_lines.saturating_sub(history)
        };
        for index in 0..history + screen.rows {
            let line = if index < history {
                &screen.history[index]
            } else {
                screen.line(index - history)
            };
            let (text, offsets) = searchable_line(line);
            for found in matcher.find_iter(text.trim_end()) {
                if found.is_empty() {
                    continue;
                }
                self.matches.push(SearchMatch {
                    row: first + index,
                    columns: byte_column(&offsets, found.start())
                        ..byte_column(&offsets, found.end()),
                });
            }
        }
        if self.matches.is_empty() {
            self.current = None;
        } else {
            self.current = Some(self.current.unwrap_or(0).min(self.matches.len() - 1));
        }
    }

    fn rebuild_from_start(&mut self, screen: &Screen) -> SearchAction {
        self.current = Some(0);
        self.rebuild(screen);
        self.current
            .and_then(|current| self.matches.get(current).cloned())
            .map_or(SearchAction::None, SearchAction::Navigate)
    }

    fn navigate(&mut self, previous: bool) -> SearchAction {
        if self.matches.is_empty() {
            return SearchAction::None;
        }
        let current = self.current.unwrap_or(0);
        let next = if previous {
            (current + self.matches.len() - 1) % self.matches.len()
        } else {
            (current + 1) % self.matches.len()
        };
        self.current = Some(next);
        SearchAction::Navigate(self.matches[next].clone())
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        pane: egui::Rect,
        screen: &Screen,
    ) -> SearchAction {
        let openness = ctx.animate_bool_with_time_and_easing(
            id.with("open"),
            self.open,
            DROP_DOWN_TIME,
            egui::emath::easing::cubic_out,
        );
        if openness == 0.0 {
            return SearchAction::None;
        }
        let width =
            (pane.width() - 2.0 * DROP_DOWN_HORIZONTAL_MARGIN).clamp(170.0, DROP_DOWN_MAX_WIDTH);
        let position = egui::pos2(
            pane.center().x - width / 2.0,
            pane.top() + DROP_DOWN_TOP_INSET - DROP_DOWN_HEIGHT * (1.0 - openness),
        );
        let previous = ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::G,
            ))
        });
        let next = ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::G,
            ))
        });
        let mut action = if previous || next {
            self.navigate(previous)
        } else {
            SearchAction::None
        };
        egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(pane));
                egui::Frame::new()
                    .fill(get_global_color("surfaceContainerHigh"))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        get_global_color("outlineVariant"),
                    ))
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: CORNER_RADIUS,
                        se: CORNER_RADIUS,
                    })
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_width(width - 8.0);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| {
                            let show_options = width >= 410.0;
                            let show_status = width >= 320.0;
                            let show_navigation = width >= 370.0;
                            let reserved = 32.0
                                + if show_options { 72.0 } else { 0.0 }
                                + if show_status { 70.0 } else { 0.0 }
                                + if show_navigation { 72.0 } else { 0.0 };
                            let field_width = (ui.available_width() - reserved).max(90.0);
                            let field = ui.add_sized(
                                [field_width, 32.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .id(id.with("query"))
                                    .vertical_align(egui::Align::Center)
                                    .hint_text("Find"),
                            );
                            if self.request_focus {
                                field.request_focus();
                                self.request_focus = false;
                            }
                            if field.changed() {
                                action = self.rebuild_from_start(screen);
                            }
                            let query_active = field.has_focus() || field.lost_focus();
                            if query_active
                                && ui.input(|input| {
                                    input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                                })
                            {
                                action = self.navigate(false);
                                field.request_focus();
                            }
                            if query_active
                                && ui.input(|input| {
                                    input.key_pressed(egui::Key::Enter) && input.modifiers.shift
                                })
                            {
                                action = self.navigate(true);
                                field.request_focus();
                            }
                            if query_active
                                && ui.input(|input| input.key_pressed(egui::Key::Escape))
                            {
                                action = SearchAction::Close;
                            }

                            if show_options && toggle(ui, "Aa", self.case_sensitive, "Match case") {
                                self.case_sensitive = !self.case_sensitive;
                                action = self.rebuild_from_start(screen);
                            }
                            if show_options
                                && toggle(ui, ".*", self.regex, "Use regular expression")
                            {
                                self.regex = !self.regex;
                                action = self.rebuild_from_start(screen);
                            }
                            let status = if self.query.is_empty() {
                                String::new()
                            } else if self.error.is_some() {
                                "Invalid".into()
                            } else if let Some(current) = self.current {
                                format!("{} of {}", current + 1, self.matches.len())
                            } else {
                                "0 of 0".into()
                            };
                            if show_status {
                                let response = ui.add_sized(
                                    [66.0, 32.0],
                                    egui::Label::new(RichText::new(status).small()).truncate(),
                                );
                                if let Some(error) = &self.error {
                                    response.on_hover_text(error);
                                }
                            }
                            if show_navigation
                                && ui
                                    .add(icon_button("arrow_upward"))
                                    .on_hover_text("Previous match (Shift+Enter or Ctrl+Shift+G)")
                                    .clicked()
                            {
                                action = self.navigate(true);
                            }
                            if show_navigation
                                && ui
                                    .add(icon_button("arrow_downward"))
                                    .on_hover_text("Next match (Enter or Ctrl+G)")
                                    .clicked()
                            {
                                action = self.navigate(false);
                            }
                            if ui
                                .add(icon_button("close"))
                                .on_hover_text("Close find (Escape)")
                                .clicked()
                            {
                                action = SearchAction::Close;
                            }
                        });
                    });
            });
        action
    }
}

fn searchable_line(line: &[Cell]) -> (String, Vec<(usize, usize)>) {
    let mut text = String::new();
    let mut offsets = vec![(0, 0)];
    for (column, cell) in line.iter().enumerate() {
        if cell.continuation {
            continue;
        }
        text.push_str(&cell.text);
        let width = 1 + line[column + 1..]
            .iter()
            .take_while(|cell| cell.continuation)
            .count();
        offsets.push((text.len(), column + width));
    }
    (text, offsets)
}

fn byte_column(offsets: &[(usize, usize)], byte: usize) -> usize {
    offsets
        .get(
            offsets
                .partition_point(|(offset, _)| *offset <= byte)
                .saturating_sub(1),
        )
        .map_or(0, |(_, column)| *column)
}

fn toggle(ui: &mut egui::Ui, label: &str, selected: bool, tooltip: &str) -> bool {
    let mut button = text_button(label).small().min_size(Vec2::new(32.0, 32.0));
    if selected {
        button = button
            .fill(get_global_color("primaryContainer"))
            .text_color(get_global_color("onPrimaryContainer"));
    }
    ui.add(button).on_hover_text(tooltip).clicked()
}

fn icon_button(name: &str) -> super::Button {
    text_button(
        RichText::new(egui_material3::material_symbol::material_symbol_text(name)).font(
            FontId::new(
                18.0,
                FontFamily::Name(crate::fonts::MATERIAL_SYMBOLS.into()),
            ),
        ),
    )
    .small()
    .min_size(Vec2::new(32.0, 32.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_prefill_is_short_plain_single_line_text() {
        let mut search = TerminalSearch {
            query: "preserved".into(),
            regex: true,
            ..Default::default()
        };
        search.prefill_selection(Some("selected text"));
        assert_eq!(search.query, "selected text");
        assert!(!search.regex);

        search.prefill_selection(Some("multiple\nlines"));
        assert_eq!(search.query, "selected text");
        search.prefill_selection(Some(&"x".repeat(MAX_SELECTION_QUERY_CHARS + 1)));
        assert_eq!(search.query, "selected text");
        search.prefill_selection(None);
        assert_eq!(search.query, "selected text");
    }

    #[test]
    fn plain_regex_and_case_searches_map_to_terminal_cells() {
        let mut screen = Screen::new(20, 3);
        screen.feed("Alpha界 beta\r\nalpha BETA".as_bytes());
        let mut search = TerminalSearch {
            query: "alpha".into(),
            ..Default::default()
        };
        search.rebuild(&screen);
        assert_eq!(search.matches.len(), 2);
        assert_eq!(search.matches[0].columns, 0..5);

        search.case_sensitive = true;
        search.rebuild(&screen);
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].row, 1);

        search.query = "界\\s+b.ta".into();
        search.regex = true;
        search.case_sensitive = false;
        search.rebuild(&screen);
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].columns, 5..12);

        search.query = "[".into();
        search.rebuild(&screen);
        assert!(search.error.is_some() && search.matches.is_empty());
    }
}
