//! The Scripting workspace's console: a Shape Script prompt with history, like Blender's Python
//! console (Enter runs, Up/Down recall, Ctrl+L clears), plus Run Script… for a file.

use egui::RichText;

use crate::{ShapeApp, theme};

pub const CONSOLE_H: f32 = 220.0;

#[derive(Clone, Debug, Default)]
pub struct Console {
    pub input: String,
    pub lines: Vec<(bool, String)>,
    pub history: Vec<String>,
    pub cursor: Option<usize>,
    pub hovered: bool,
}

impl ShapeApp {
    pub(crate) fn console_panel(&mut self, ui: &mut egui::Ui) {
        let resp =
            egui::Panel::bottom("console").default_size(CONSOLE_H).resizable(true).frame(theme::region_frame(theme::TIMELINE)).show(ui, |ui| {
                egui::Panel::top("console-header").exact_size(theme::HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(RichText::new("Console").strong());
                        ui.separator();
                        if ui.button("Run Script…").clicked()
                            && let Some(path) = rfd::FileDialog::new().add_filter("Shape Script", &["shape", "txt"]).pick_file()
                        {
                            match std::fs::read_to_string(&path) {
                                Ok(src) => {
                                    let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("script").to_string();
                                    let out = self.run_script(&src, &name);
                                    self.console.lines.push((false, format!("# run {name}")));
                                    self.console.lines.extend(out.into_iter().map(|l| (true, l)));
                                }
                                Err(e) => self.console.lines.push((true, format!("open failed: {e}"))),
                            }
                        }
                        if ui.button("Clear").clicked() {
                            self.console.lines.clear();
                        }
                        ui.label(RichText::new("Enter run · Up/Down history · `help` lists tools").color(theme::TEXT_WEAK).small());
                    });
                });
                ui.set_min_height(CONSOLE_H - theme::HEADER_H);
                let input_h = 26.0;
                egui::ScrollArea::vertical().max_height((ui.available_height() - input_h).max(40.0)).stick_to_bottom(true).show(ui, |ui| {
                    if self.console.lines.is_empty() {
                        ui.label(RichText::new("Shape Script console. Try: add_primitive kind=cube size=1  ·  help").color(theme::TEXT_WEAK));
                    }
                    for (is_output, line) in &self.console.lines {
                        let text = RichText::new(line).monospace();
                        ui.label(if *is_output { text.color(theme::TEXT) } else { text.color(theme::TEXT_WEAK) });
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(">>>").monospace().color(theme::TEXT_WEAK));
                    let edit = egui::TextEdit::singleline(&mut self.console.input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("command key=value …");
                    let r = ui.add(edit);
                    if r.has_focus() {
                        let (up, down, clear) = ui.input_mut(|i| {
                            (
                                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                                i.consume_key(egui::Modifiers::COMMAND, egui::Key::L),
                            )
                        });
                        if clear {
                            self.console.lines.clear();
                        }
                        if up && !self.console.history.is_empty() {
                            let c = self.console.cursor.map(|c| c.saturating_sub(1)).unwrap_or(self.console.history.len() - 1);
                            self.console.cursor = Some(c);
                            self.console.input = self.console.history[c].clone();
                        }
                        if down && let Some(c) = self.console.cursor {
                            if c + 1 < self.console.history.len() {
                                self.console.cursor = Some(c + 1);
                                self.console.input = self.console.history[c + 1].clone();
                            } else {
                                self.console.cursor = None;
                                self.console.input.clear();
                            }
                        }
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let cmd = std::mem::take(&mut self.console.input);
                        if !cmd.trim().is_empty() {
                            self.console.lines.push((false, format!(">>> {cmd}")));
                            let out = self.run_script(&cmd, "console");
                            self.console.lines.extend(out.into_iter().map(|l| (true, l)));
                            self.console.history.push(cmd);
                            self.console.cursor = None;
                        }
                        r.request_focus();
                    }
                });
            });
        self.console.hovered = ui.rect_contains_pointer(resp.response.rect);
    }
}
