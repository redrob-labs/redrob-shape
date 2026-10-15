//! The Properties editor body, one tab at a time (Blender's Render / Output / World / Object /
//! Modifiers / Data / Material tabs, limited to the ones with content).

use crate::ShapeApp;
use crate::layout::PropsTab;
use crate::panels::vec3_row;
use egui::RichText;
use shape_math::Vec3;
use shape_scene::{Light, LightKind, ObjectData};
use shape_undo::Snapshot;

impl ShapeApp {
    pub(crate) fn properties(&mut self, ui: &mut egui::Ui) {
        let tab = self.regions.props_tab;
        ui.label(RichText::new(tab.name()).color(crate::theme::TEXT_WEAK).size(12.0));
        match tab {
            PropsTab::Render => self.render_properties(ui),
            PropsTab::Output => self.output_properties(ui),
            PropsTab::World => self.world_properties(ui),
            PropsTab::Object | PropsTab::Modifiers | PropsTab::Data | PropsTab::Material => self.object_properties(ui, tab),
        }
    }

    fn render_properties(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Sampling").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut self.render.target_samples).range(1..=4096).prefix("Samples "));
            ui.add(egui::DragValue::new(&mut self.render.settings.max_depth).range(1..=32).prefix("Max Bounces "));
            ui.checkbox(&mut self.render.settings.denoise, "Denoise").on_hover_text("Edge-aware filter on the final image");
        });
        egui::CollapsingHeader::new("Color Management").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut self.render.settings.exposure).range(0.01..=16.0).speed(0.02).prefix("Exposure "));
        });
        if ui.button("Render Image (F12)").clicked() {
            self.start_render(ui.ctx());
        }
        if ui.button("Render Animation (Ctrl+F12)").clicked() {
            self.start_render_animation(ui.ctx());
        }
    }

    fn output_properties(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Format").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.render.width).range(16..=8192).prefix("X "));
                ui.add(egui::DragValue::new(&mut self.render.height).range(16..=8192).prefix("Y "));
            });
            ui.add(egui::DragValue::new(&mut self.render.resolution_percent).range(1..=200).prefix("Resolution ").suffix(" %"));
            ui.label(RichText::new("PNG, 8-bit RGBA, sRGB").weak());
        });
        egui::CollapsingHeader::new("Frame Range").default_open(true).show(ui, |ui| {
            let t = &mut self.scene.timeline;
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut t.start).prefix("Start ").range(0..=t.end));
                ui.add(egui::DragValue::new(&mut t.end).prefix("End ").range(t.start..=1_000_000));
            });
            ui.add(egui::DragValue::new(&mut t.fps).prefix("Frame Rate ").range(1.0..=240.0));
        });
        egui::CollapsingHeader::new("Output").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Path");
                ui.text_edit_singleline(&mut self.render.output_path);
                if ui.button("…").clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    self.render.output_path = format!("{}/frame_", dir.display());
                }
            });
            ui.label(RichText::new("Frames are written as <path>####.png").weak());
            if let Some((done, total)) = self.render.animation_progress() {
                ui.label(format!("Rendering animation: {done}/{total} frames"));
            }
        });
    }

    fn world_properties(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Viewport Display").default_open(true).show(ui, |ui| {
            ui.checkbox(&mut self.viewport.options.show_grid, "Floor grid");
            let b = self.viewport.options.background;
            let mut bg = [b[0], b[1], b[2]];
            ui.horizontal(|ui| {
                ui.label("Background");
                if ui.color_edit_button_rgb(&mut bg).changed() {
                    self.viewport.options.background = [bg[0], bg[1], bg[2], 1.0];
                }
            });
        });
        egui::CollapsingHeader::new("Surface").default_open(true).show(ui, |ui| {
            let mut sky = self.render.settings.sky.to_array();
            ui.horizontal(|ui| {
                ui.label("Sky");
                if ui.color_edit_button_rgb(&mut sky).changed() {
                    self.render.settings.sky = Vec3::from_array(sky);
                }
            });
        });
    }

    fn object_properties(&mut self, ui: &mut egui::Ui, tab: PropsTab) {
        let Some(id) = self.scene.selection.active else {
            ui.label(RichText::new("No active object").weak());
            return;
        };
        let Some(object) = self.scene.object(id).cloned() else { return };
        let mut edited = object.clone();
        match tab {
            PropsTab::Object => {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut edited.name);
                });
                ui.checkbox(&mut edited.visible, "Visible");
                egui::CollapsingHeader::new("Transform").default_open(true).show(ui, |ui| {
                    vec3_row(ui, "Location", &mut edited.transform.translation, 0.01);
                    let (rx, ry, rz) = edited.transform.rotation.to_euler(shape_math::glam_euler());
                    let mut deg = Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
                    if vec3_row(ui, "Rotation", &mut deg, 0.5) {
                        edited.transform.rotation =
                            shape_math::Quat::from_euler(shape_math::glam_euler(), deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians());
                    }
                    vec3_row(ui, "Scale", &mut edited.transform.scale, 0.01);
                });
                egui::CollapsingHeader::new("Relations").default_open(false).show(ui, |ui| {
                    let parent = object.parent.and_then(|p| self.scene.object(p)).map(|o| o.name.clone()).unwrap_or("None".into());
                    ui.label(format!("Parent: {parent}"));
                });
            }
            PropsTab::Data => {
                if let ObjectData::Mesh { mesh, .. } = &object.data
                    && let Some(data) = self.scene.mesh(*mesh)
                {
                    egui::CollapsingHeader::new("Mesh").default_open(true).show(ui, |ui| {
                        ui.label(format!(
                            "{} vertices, {} edges, {} faces",
                            data.mesh.vertex_count(),
                            data.mesh.edge_count(),
                            data.mesh.face_count()
                        ));
                        if data.uvs.is_some() {
                            ui.label(RichText::new("UV map present").weak());
                        }
                        if !self.scene.modifiers(id).is_empty()
                            && let Some(ev) = shape_ops::evaluated_mesh(&self.scene, id)
                        {
                            ui.label(RichText::new(format!("evaluated: {} vertices, {} faces", ev.vertex_count(), ev.face_count())).weak());
                        }
                    });
                    egui::CollapsingHeader::new("UV Maps").default_open(false).show(ui, |ui| {
                        if ui.button("Box Projection").clicked() {
                            self.generate_uvs("UV Box", shape_ops::uv_box);
                        }
                        if ui.button("Planar (top)").clicked() {
                            self.generate_uvs("UV Planar", |m| shape_ops::uv_planar(m, 1));
                        }
                        if ui.button("Sphere Projection").clicked() {
                            self.generate_uvs("UV Sphere", shape_ops::uv_sphere);
                        }
                    });
                }
                match &mut edited.data {
                    ObjectData::Light(light) => {
                        egui::CollapsingHeader::new("Light").default_open(true).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (kind, label) in [(LightKind::Point, "Point"), (LightKind::Sun, "Sun"), (Light::spot(0.0).kind, "Spot")] {
                                    let same = std::mem::discriminant(&light.kind) == std::mem::discriminant(&kind);
                                    if ui.radio(same, label).clicked() && !same {
                                        light.kind = kind;
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Color");
                                ui.color_edit_button_rgb(&mut light.color);
                            });
                            let label = if matches!(light.kind, LightKind::Sun) { "Strength" } else { "Power (W)" };
                            ui.add(egui::DragValue::new(&mut light.power).speed(1.0).range(0.0..=1.0e6).prefix(format!("{label} ")));
                            if let LightKind::Spot { inner_angle, outer_angle } = &mut light.kind {
                                let (mut inner, mut outer) = (inner_angle.to_degrees(), outer_angle.to_degrees());
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(&mut inner).range(1.0..=89.0).prefix("inner ° "));
                                    ui.add(egui::DragValue::new(&mut outer).range(1.0..=90.0).prefix("outer ° "));
                                });
                                *inner_angle = inner.to_radians();
                                *outer_angle = outer.max(inner + 0.5).to_radians();
                            }
                        });
                    }
                    ObjectData::Camera(cam) => {
                        egui::CollapsingHeader::new("Lens").default_open(true).show(ui, |ui| {
                            let mut fov = cam.fov_y.to_degrees();
                            ui.add(egui::DragValue::new(&mut fov).range(5.0..=170.0).prefix("Field of View ° "));
                            cam.fov_y = fov.to_radians();
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut cam.near).speed(0.01).range(0.001..=100.0).prefix("Clip Start "));
                                ui.add(egui::DragValue::new(&mut cam.far).speed(1.0).range(1.0..=1.0e5).prefix("End "));
                            });
                        });
                        egui::CollapsingHeader::new("Depth of Field").default_open(true).show(ui, |ui| {
                            ui.checkbox(&mut cam.use_dof, "Depth of Field");
                            ui.add_enabled_ui(cam.use_dof, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut cam.focus_distance)
                                        .speed(0.05)
                                        .range(0.01..=1000.0)
                                        .prefix("Focus Distance ")
                                        .suffix(" m"),
                                );
                                ui.add(egui::DragValue::new(&mut cam.fstop).speed(0.05).range(0.1..=128.0).prefix("F-Stop "));
                            });
                        });
                    }
                    _ => {}
                }
            }
            PropsTab::Material => {
                if matches!(object.data, ObjectData::Mesh { .. }) {
                    self.material_panel(ui, id);
                }
            }
            PropsTab::Modifiers => {
                if matches!(object.data, ObjectData::Mesh { .. }) {
                    self.modifier_panel(ui, id);
                }
            }
            _ => {}
        }
        if edited != object {
            self.edit_object_live(id, edited);
        }
        // One undo step per drag/typing session, recorded when pointer and keyboard let go.
        let idle = ui.input(|i| !i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input();
        if idle && let Some(before) = self.property_before.take() {
            let after = std::mem::replace(&mut self.scene, before);
            let cmd = Snapshot::new("Edit Properties", &self.scene, move |s| *s = after);
            self.history.push(Box::new(cmd), &mut self.scene);
        }
    }
}
