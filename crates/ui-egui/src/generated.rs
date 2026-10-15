//! Text, metaball and image-plane objects in the editor: Shift+A entries, their Data tab panels
//! (Blender's Font and Metaball tabs), regeneration of the cached mesh after every edit, and
//! Convert to Mesh for the generated ones.

use egui::RichText;
use shape_math::Vec3;
use shape_scene::{MeshData, MetaballData, MetaballElement, ObjectData, ObjectId, TextData};

use crate::ShapeApp;

impl ShapeApp {
    /// Shift+A > Text: Blender's default body "Text", one unit high, flat on the ground.
    pub fn add_text(&mut self) -> ObjectId {
        let text = TextData::default();
        let mesh = shape_ops::text::text_mesh(&text.body, text.size, text.extrude);
        let at = self.scene.cursor;
        let n = self.scene.object_count();
        let name = if n == 0 { "Text".to_owned() } else { format!("Text.{n:03}") };
        let mut id = ObjectId(0);
        self.edit("Add Text", |scene| {
            let mid = scene.add_mesh(MeshData::new(name.clone(), mesh));
            let mut o = shape_scene::Object::new(name, ObjectData::Text { text, mesh: mid });
            o.transform = shape_math::Transform::from_translation(at);
            id = scene.add_object(o);
            scene.selection.set(id);
        });
        id
    }

    /// Shift+A > Metaball > Ball: one element of radius 2 (Blender's default).
    pub fn add_metaball(&mut self) -> ObjectId {
        let meta = MetaballData::default();
        let at = self.scene.cursor;
        let n = self.scene.object_count();
        let name = if n == 0 { "Mball".to_owned() } else { format!("Mball.{n:03}") };
        let mut id = ObjectId(0);
        self.edit("Add Metaball", |scene| {
            let mid = scene.add_mesh(MeshData::new(name.clone(), shape_geom::primitives::cube(0.0)));
            let mut o = shape_scene::Object::new(name, ObjectData::Metaball { meta, mesh: mid });
            o.transform = shape_math::Transform::from_translation(at);
            id = scene.add_object(o);
            scene.selection.set(id);
            shape_ops::regenerate_generated(scene, id);
        });
        id
    }

    /// Add > Image as Plane: a quad with the image's aspect, a material carrying the texture and
    /// planar UVs, as the Blender add-on does.
    pub fn add_image_plane(&mut self, path: &std::path::Path) {
        let tex = match shape_scene::Texture::load(path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("Image failed: {e}");
                return;
            }
        };
        let aspect = if tex.height > 0 { tex.width as f32 / tex.height as f32 } else { 1.0 };
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Image".into());
        let at = self.scene.cursor;
        let (w, h) = if aspect >= 1.0 { (2.0, 2.0 / aspect) } else { (2.0 * aspect, 2.0) };
        let mesh = shape_geom::primitives::plane(1.0, 1, 1);
        self.edit("Add Image as Plane", |scene| {
            let tid = scene.add_texture(tex);
            let mat =
                scene.add_material(shape_scene::Material { name: name.clone(), base_color_texture: Some(tid), roughness: 1.0, ..Default::default() });
            let id = scene.add_mesh_object(name, mesh, Some(mat));
            if let Some(o) = scene.object_mut(id) {
                o.transform = shape_math::Transform { translation: at, rotation: shape_math::Quat::IDENTITY, scale: Vec3::new(w, 1.0, h) };
            }
            if let Some((mid, m)) = scene.object_mesh(id) {
                let uvs = shape_ops::uv_planar(m, 1);
                if let Some(d) = scene.mesh_mut(mid) {
                    d.uvs = Some(uvs);
                }
            }
            scene.selection.set(id);
        });
        self.viewport.options.shading = shape_gpu::ShadingMode::MaterialPreview;
    }

    /// Convert a Text or Metaball object into plain mesh data (Object > Convert > Mesh).
    pub fn convert_generated_to_mesh(&mut self) -> bool {
        let Some(id) = self.scene.selection.active else { return false };
        let mesh_id = match self.scene.object(id).map(|o| &o.data) {
            Some(ObjectData::Text { mesh, .. } | ObjectData::Metaball { mesh, .. }) => *mesh,
            _ => return false,
        };
        self.edit("Convert to Mesh", |scene| {
            if let Some(o) = scene.object_mut(id) {
                o.data = ObjectData::Mesh { mesh: mesh_id, material: None, modifiers: Vec::new() };
            }
        });
        true
    }

    pub(crate) fn text_data_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
        let Some(ObjectData::Text { text: original, .. }) = self.scene.object(id).map(|o| o.data.clone()) else { return };
        let mut t = original.clone();
        egui::CollapsingHeader::new("Font").default_open(true).show(ui, |ui| {
            ui.label("Body");
            ui.text_edit_multiline(&mut t.body);
            ui.add(egui::DragValue::new(&mut t.size).speed(0.02).range(0.01..=100.0).prefix("Size "));
            ui.label(RichText::new("Ubuntu Light (bundled, UFL)").weak());
        });
        egui::CollapsingHeader::new("Geometry").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut t.extrude).speed(0.005).range(0.0..=10.0).prefix("Extrude "));
        });
        if ui.button("Convert to Mesh").clicked() {
            self.convert_generated_to_mesh();
            return;
        }
        if t != original {
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(ObjectData::Text { text, .. }) = self.scene.object_mut(id).map(|o| &mut o.data) {
                *text = t;
            }
            shape_ops::regenerate_generated(&mut self.scene, id);
        }
    }

    pub(crate) fn metaball_data_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
        let Some(ObjectData::Metaball { meta: original, .. }) = self.scene.object(id).map(|o| o.data.clone()) else { return };
        let mut m = original.clone();
        egui::CollapsingHeader::new("Metaball").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut m.resolution).range(4..=96).prefix("Resolution "));
            ui.add(egui::DragValue::new(&mut m.threshold).speed(0.01).range(0.01..=5.0).prefix("Threshold "));
        });
        egui::CollapsingHeader::new("Elements").default_open(true).show(ui, |ui| {
            let mut remove = None;
            for (i, e) in m.elements.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Ball {i}"));
                    ui.add(egui::DragValue::new(&mut e.radius).speed(0.02).range(0.05..=50.0).prefix("R "));
                    ui.add(egui::DragValue::new(&mut e.stiffness).speed(0.05).range(0.1..=10.0).prefix("Stiffness "));
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.add(egui::DragValue::new(&mut e.position[axis]).speed(0.02).prefix(format!("{label} ")));
                    }
                });
            }
            if let Some(i) = remove
                && m.elements.len() > 1
            {
                m.elements.remove(i);
            }
            if ui.button("Add Ball").clicked() {
                let last = m.elements.last().copied().unwrap_or(MetaballElement { position: Vec3::ZERO, radius: 2.0, stiffness: 2.0 });
                m.elements.push(MetaballElement { position: last.position + Vec3::X * last.radius, ..last });
            }
        });
        if ui.button("Convert to Mesh").clicked() {
            self.convert_generated_to_mesh();
            return;
        }
        if m != original {
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(ObjectData::Metaball { meta, .. }) = self.scene.object_mut(id).map(|o| &mut o.data) {
                *meta = m;
            }
            shape_ops::regenerate_generated(&mut self.scene, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_metaball_objects_show_generated_meshes_and_convert() {
        let mut app = ShapeApp::default();
        let t = app.add_text();
        let text_faces = shape_ops::evaluated_mesh(&app.scene, t).unwrap().face_count();
        assert!(text_faces > 10, "'Text' has glyph faces: {text_faces}");
        if let Some(ObjectData::Text { text, .. }) = app.scene.object_mut(t).map(|o| &mut o.data) {
            text.body = "Hi".into();
            text.extrude = 0.1;
        }
        shape_ops::regenerate_generated(&mut app.scene, t);
        let solid = shape_ops::evaluated_mesh(&app.scene, t).unwrap();
        assert!(solid.is_closed());
        let m = app.add_metaball();
        let blob = shape_ops::evaluated_mesh(&app.scene, m).unwrap();
        assert!(blob.face_count() > 100 && blob.is_closed(), "blob {}", blob.face_count());
        app.scene.selection.set(m);
        assert!(app.convert_generated_to_mesh());
        assert!(app.scene.object_mesh(m).is_some());
        assert_eq!(app.history.undo_name(), Some("Convert to Mesh"));
    }
}
