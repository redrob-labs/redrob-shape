//! The Timeline editor, after Blender's: playback controls, frame fields, a frame ruler with the
//! active object's keyframes as diamonds, a blue playhead you can scrub, and Blender's playback
//! keys (Space, arrows, Shift+arrows, Up/Down to the next/previous key). Scrubbing and playback
//! evaluate the actions into the object transforms without touching the undo history.

use egui::{Color32, RichText, Stroke, pos2, vec2};
use shape_scene::KeyingSet;

use crate::{ShapeApp, theme};

pub const TIMELINE_H: f32 = 120.0;
const KEY_DIAMOND: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);
const KEY_SELECTED: Color32 = Color32::from_rgb(0xff, 0xa0, 0x28);
const PLAYHEAD: Color32 = Color32::from_rgb(0x47, 0x72, 0xb3);

impl ShapeApp {
    /// Jump to `frame` (clamped to the range) and evaluate the animation there.
    pub fn set_frame(&mut self, frame: i32) {
        let t = &mut self.scene.timeline;
        t.frame = frame.clamp(t.start.min(t.end), t.end.max(t.start));
        let f = t.frame as f32;
        self.scene.evaluate_animation(f);
    }

    /// Advance playback by the frame's wall time (Space).
    pub(crate) fn animation_tick(&mut self, ui: &egui::Ui) {
        if !self.scene.timeline.playing {
            return;
        }
        let dt = ui.input(|i| i.stable_dt).min(0.25);
        self.play_accum += dt * self.scene.timeline.fps;
        let steps = self.play_accum.floor() as i32;
        if steps > 0 {
            self.play_accum -= steps as f32;
            let t = &self.scene.timeline;
            let mut f = t.frame + steps;
            if f > t.end {
                f = t.start + (f - t.end - 1) % (t.end - t.start + 1).max(1);
            }
            self.set_frame(f);
        }
        ui.ctx().request_repaint();
    }

    pub fn toggle_play(&mut self) {
        self.scene.timeline.playing = !self.scene.timeline.playing;
        self.play_accum = 0.0;
        self.status = if self.scene.timeline.playing { "Playing".into() } else { "Paused".into() };
    }

    /// Blender's playback keys; live in every mode except while typing.
    pub(crate) fn playback_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() || self.popup.is_some() {
            return;
        }
        let [space, left, right, s_left, s_right, up, down, alt_i] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::Space),
                crate::key(i, egui::Modifiers::NONE, egui::Key::ArrowLeft),
                crate::key(i, egui::Modifiers::NONE, egui::Key::ArrowRight),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::ArrowLeft),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::ArrowRight),
                crate::key(i, egui::Modifiers::NONE, egui::Key::ArrowUp),
                crate::key(i, egui::Modifiers::NONE, egui::Key::ArrowDown),
                crate::key(i, egui::Modifiers::ALT, egui::Key::I),
            ]
        });
        if space {
            self.toggle_play();
        }
        let (frame, start, end) = (self.scene.timeline.frame, self.scene.timeline.start, self.scene.timeline.end);
        if left {
            self.set_frame(frame - 1);
        }
        if right {
            self.set_frame(frame + 1);
        }
        if s_left {
            self.set_frame(start);
        }
        if s_right {
            self.set_frame(end);
        }
        if up || down {
            let keys = self.scene.selection.active.map(|id| self.scene.keyed_frames(id)).unwrap_or_default();
            let target = if up {
                keys.iter().map(|k| k.round() as i32).find(|&k| k > frame)
            } else {
                keys.iter().rev().map(|k| k.round() as i32).find(|&k| k < frame)
            };
            match target {
                Some(f) => self.set_frame(f),
                None => self.status = "No more keyframes".into(),
            }
        }
        if alt_i {
            self.delete_keyframes_at_current_frame();
        }
    }

    /// `I` menu action: key the selected objects on `set` at the current frame, as one undo step.
    pub fn insert_keyframes(&mut self, set: KeyingSet) {
        let frame = self.scene.timeline.frame;
        if let Some(pose) = self.pose.clone() {
            // Pose Mode: key the selected bones instead of the object.
            let bones = match self.scene.object(pose.object).map(|o| &o.data) {
                Some(shape_scene::ObjectData::Armature(a)) => a.selected.clone(),
                _ => Vec::new(),
            };
            if bones.is_empty() {
                self.status = "No bones selected to key".into();
                return;
            }
            self.edit(&format!("Insert Keyframe ({})", set.name()), |scene| {
                scene.insert_bone_keyframes(pose.object, &bones, frame, set);
            });
            return;
        }
        let targets: Vec<_> = self.scene.selection.objects.iter().copied().collect();
        if targets.is_empty() {
            self.status = "Nothing selected to key".into();
            return;
        }
        self.edit(&format!("Insert Keyframe ({})", set.name()), |scene| {
            for id in &targets {
                scene.insert_keyframe(*id, frame, set);
            }
        });
    }

    pub fn delete_keyframes_at_current_frame(&mut self) {
        let frame = self.scene.timeline.frame;
        let targets: Vec<_> = self.scene.selection.objects.iter().copied().filter(|id| self.scene.has_key_at(*id, frame)).collect();
        if targets.is_empty() {
            self.status = "No keyframe at this frame".into();
            return;
        }
        self.edit("Delete Keyframe", |scene| {
            for id in &targets {
                scene.delete_keyframe(*id, frame);
            }
        });
    }

    pub(crate) fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("timeline").default_size(TIMELINE_H).resizable(true).frame(theme::region_frame(theme::TIMELINE)).show(ui, |ui| {
            self.timeline_header(ui);
            ui.set_min_height(TIMELINE_H - theme::HEADER_H);
            self.timeline_ruler(ui);
        });
    }

    fn timeline_header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("timeline-header").exact_size(theme::HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Timeline").strong());
                ui.separator();
                let (start, end, frame) = (self.scene.timeline.start, self.scene.timeline.end, self.scene.timeline.frame);
                let keys = self.scene.selection.active.map(|id| self.scene.keyed_frames(id)).unwrap_or_default();
                if ui.button("⏮").on_hover_text("Jump to Endpoint (Shift+Left)").clicked() {
                    self.set_frame(start);
                }
                if ui.button("◆◀").on_hover_text("Jump to Keyframe (Down)").clicked()
                    && let Some(f) = keys.iter().rev().map(|k| k.round() as i32).find(|&k| k < frame)
                {
                    self.set_frame(f);
                }
                if ui.button("◀").on_hover_text("Previous Frame (Left)").clicked() {
                    self.set_frame(frame - 1);
                }
                let playing = self.scene.timeline.playing;
                if ui.button(if playing { "⏸" } else { "▶" }).on_hover_text("Play Animation (Space)").clicked() {
                    self.toggle_play();
                }
                if ui.button("▶|").on_hover_text("Next Frame (Right)").clicked() {
                    self.set_frame(frame + 1);
                }
                if ui.button("▶◆").on_hover_text("Jump to Keyframe (Up)").clicked()
                    && let Some(f) = keys.iter().map(|k| k.round() as i32).find(|&k| k > frame)
                {
                    self.set_frame(f);
                }
                if ui.button("⏭").on_hover_text("Jump to Endpoint (Shift+Right)").clicked() {
                    self.set_frame(end);
                }
                ui.separator();
                let mut f = frame;
                if ui.add(egui::DragValue::new(&mut f).range(start..=end)).on_hover_text("Current Frame").changed() {
                    self.set_frame(f);
                }
                ui.separator();
                let t = &mut self.scene.timeline;
                ui.add(egui::DragValue::new(&mut t.start).prefix("Start ").range(0..=t.end));
                ui.add(egui::DragValue::new(&mut t.end).prefix("End ").range(t.start..=1_000_000));
                ui.add(egui::DragValue::new(&mut t.fps).prefix("FPS ").range(1.0..=240.0));
                ui.separator();
                ui.menu_button("Keying", |ui| {
                    for set in KeyingSet::ALL {
                        if ui.button(set.name()).clicked() {
                            self.insert_keyframes(set);
                        }
                    }
                });
                ui.label(RichText::new("I insert · Alt+I delete · Space play").color(theme::TEXT_WEAK).small());
            });
        });
    }

    fn timeline_ruler(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x1d, 0x1d, 0x1d));
        let t = self.scene.timeline.clone();
        let span = (t.end - t.start).max(1) as f32;
        let left = rect.left() + 8.0;
        let width = (rect.width() - 16.0).max(1.0);
        let x_of = |frame: f32| left + (frame - t.start as f32) / span * width;
        let frame_of = |x: f32| (t.start as f32 + (x - left) / width * span).round() as i32;
        // Ruler: a label every n frames so they stay ~60 px apart.
        let ruler_h = 20.0;
        painter.rect_filled(egui::Rect::from_min_size(rect.min, vec2(rect.width(), ruler_h)), 0.0, Color32::from_rgb(0x28, 0x28, 0x28));
        let step = [1, 2, 5, 10, 20, 25, 50, 100, 250, 500, 1000].into_iter().find(|&s| width / (span / s as f32) >= 60.0).unwrap_or(1000);
        let mut f = (t.start / step) * step;
        while f <= t.end {
            if f >= t.start {
                let x = x_of(f as f32);
                painter.line_segment([pos2(x, rect.top() + ruler_h), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x2a)));
                painter.text(pos2(x, rect.top() + 4.0), egui::Align2::CENTER_TOP, f.to_string(), egui::FontId::proportional(11.0), theme::TEXT_WEAK);
            }
            f += step;
        }
        // Summary row + one row per channel of the active object (Dope Sheet style).
        let row_h = 18.0;
        let mut y = rect.top() + ruler_h + row_h / 2.0;
        if let Some(id) = self.scene.selection.active {
            let keyed = self.scene.keyed_frames(id);
            let name = self.scene.object(id).map(|o| o.name.clone()).unwrap_or_default();
            painter.text(
                pos2(rect.left() + 8.0, y),
                egui::Align2::LEFT_CENTER,
                format!("Summary · {name}"),
                egui::FontId::proportional(11.0),
                theme::TEXT,
            );
            for k in &keyed {
                diamond(&painter, pos2(x_of(*k), y), 5.0, if (*k - t.frame as f32).abs() < 1e-3 { KEY_SELECTED } else { KEY_DIAMOND });
            }
            y += row_h;
            if let Some(action) = self.scene.object(id).and_then(|o| o.action.as_ref()) {
                for c in &action.curves {
                    if y > rect.bottom() - row_h / 2.0 {
                        break;
                    }
                    painter.text(
                        pos2(rect.left() + 16.0, y),
                        egui::Align2::LEFT_CENTER,
                        c.channel.name(),
                        egui::FontId::proportional(10.0),
                        theme::TEXT_WEAK,
                    );
                    for k in &c.keys {
                        diamond(
                            &painter,
                            pos2(x_of(k.frame), y),
                            4.0,
                            if (k.frame - t.frame as f32).abs() < 1e-3 { KEY_SELECTED } else { KEY_DIAMOND },
                        );
                    }
                    y += row_h;
                }
            }
        } else {
            painter.text(
                pos2(rect.left() + 8.0, y),
                egui::Align2::LEFT_CENTER,
                "No active object",
                egui::FontId::proportional(11.0),
                theme::TEXT_WEAK,
            );
        }
        // Playhead.
        let px = x_of(t.frame as f32);
        painter.line_segment([pos2(px, rect.top()), pos2(px, rect.bottom())], Stroke::new(2.0, PLAYHEAD));
        let label = egui::Rect::from_center_size(pos2(px, rect.top() + ruler_h / 2.0), vec2(30.0, 16.0));
        painter.rect_filled(label, 3.0, PLAYHEAD);
        painter.text(label.center(), egui::Align2::CENTER_CENTER, t.frame.to_string(), egui::FontId::proportional(11.0), Color32::WHITE);
        // Scrub: click or drag anywhere in the strip.
        if (resp.clicked() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.set_frame(frame_of(p.x));
        }
    }
}

fn diamond(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let pts = vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)];
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::new(1.0, Color32::BLACK)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn i_menu_keys_the_cube_and_scrubbing_moves_it() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.scene.selection.set(id);
        app.set_workspace(crate::layout::Workspace::Animation);
        app.insert_keyframes(KeyingSet::Location);
        app.set_frame(25);
        app.scene.object_mut(id).unwrap().transform.translation.x = 4.0;
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        harness.get_by_label("Timeline");
        harness.key_press(egui::Key::I);
        harness.run();
        harness.get_by_label("Location & Rotation");
        harness.get_by_label("Location, Rotation & Scale").click();
        harness.run();
        harness.run();
        drop(harness);
        assert_eq!(app.scene.keyed_frames(id), vec![1.0, 25.0]);
        assert_eq!(app.history.undo_name(), Some("Insert Keyframe (Location, Rotation & Scale)"));
        app.set_frame(13);
        let x = app.scene.object(id).unwrap().transform.translation.x;
        assert!((x - 2.0).abs() < 1e-3, "eased midpoint {x}");
        // Alt+I at frame 25 deletes that key.
        app.set_frame(25);
        app.delete_keyframes_at_current_frame();
        assert_eq!(app.scene.keyed_frames(id), vec![1.0]);
        // Playback wraps at the end of the range.
        app.scene.timeline.end = 10;
        app.set_frame(10);
        app.scene.timeline.playing = true;
        app.play_accum = 1.0;
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.step();
        drop(harness);
        let f = app.scene.timeline.frame;
        assert!((1..10).contains(&f), "wrapped into the range: {f}");
    }
}
