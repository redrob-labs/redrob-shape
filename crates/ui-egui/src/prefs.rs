//! Preferences (Edit > Preferences), after Blender's window: an Interface tab (resolution scale,
//! accent colour), a Keymap tab where object-mode shortcuts can be rebound by pressing a key,
//! and an Add-ons tab listing Shape Script files in the add-ons folder. Saved as JSON under the
//! user's config directory and loaded at start.

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::RichText;
use serde::{Deserialize, Serialize};

use crate::{ShapeApp, theme};

/// Rebindable actions. Everything else in the keymap is fixed (shown greyed in the table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    ToggleToolbar,
    ToggleSidebar,
    MaximizeArea,
    Duplicate,
    Hide,
    Reveal,
    SelectNone,
    SelectInvert,
    Undo,
    Redo,
    Delete,
    DeleteConfirm,
    FrameAll,
    Render,
    RenderAnimation,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::ToggleToolbar,
        Action::ToggleSidebar,
        Action::MaximizeArea,
        Action::Duplicate,
        Action::Hide,
        Action::Reveal,
        Action::SelectNone,
        Action::SelectInvert,
        Action::Undo,
        Action::Redo,
        Action::Delete,
        Action::DeleteConfirm,
        Action::FrameAll,
        Action::Render,
        Action::RenderAnimation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Action::ToggleToolbar => "Toggle Toolbar",
            Action::ToggleSidebar => "Toggle Sidebar",
            Action::MaximizeArea => "Maximize Area",
            Action::Duplicate => "Duplicate Objects",
            Action::Hide => "Hide Selected",
            Action::Reveal => "Show Hidden Objects",
            Action::SelectNone => "Select None",
            Action::SelectInvert => "Select Invert",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Delete => "Delete",
            Action::DeleteConfirm => "Delete (confirm)",
            Action::FrameAll => "Frame All",
            Action::Render => "Render Image",
            Action::RenderAnimation => "Render Animation",
        }
    }

    /// Blender's default binding.
    pub fn default_combo(self) -> KeyCombo {
        use egui::{Key, Modifiers as M};
        match self {
            Action::ToggleToolbar => KeyCombo::new(M::NONE, Key::T),
            Action::ToggleSidebar => KeyCombo::new(M::NONE, Key::N),
            Action::MaximizeArea => KeyCombo::new(M::COMMAND, Key::Space),
            Action::Duplicate => KeyCombo::new(M::SHIFT, Key::D),
            Action::Hide => KeyCombo::new(M::NONE, Key::H),
            Action::Reveal => KeyCombo::new(M::ALT, Key::H),
            Action::SelectNone => KeyCombo::new(M::ALT, Key::A),
            Action::SelectInvert => KeyCombo::new(M::COMMAND, Key::I),
            Action::Undo => KeyCombo::new(M::COMMAND, Key::Z),
            Action::Redo => KeyCombo::new(M::COMMAND | M::SHIFT, Key::Z),
            Action::Delete => KeyCombo::new(M::NONE, Key::Delete),
            Action::DeleteConfirm => KeyCombo::new(M::NONE, Key::X),
            Action::FrameAll => KeyCombo::new(M::NONE, Key::Home),
            Action::Render => KeyCombo::new(M::NONE, Key::F12),
            Action::RenderAnimation => KeyCombo::new(M::COMMAND, Key::F12),
        }
    }
}

/// A modifier set and a key, stored by name so the JSON stays readable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl KeyCombo {
    pub fn new(m: egui::Modifiers, key: egui::Key) -> Self {
        Self { ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt, key: key.name().to_string() }
    }

    pub fn modifiers(&self) -> egui::Modifiers {
        let mut m = egui::Modifiers::NONE;
        if self.ctrl {
            m |= egui::Modifiers::COMMAND;
        }
        if self.shift {
            m |= egui::Modifiers::SHIFT;
        }
        if self.alt {
            m |= egui::Modifiers::ALT;
        }
        m
    }

    pub fn key(&self) -> Option<egui::Key> {
        egui::Key::ALL.iter().copied().find(|k| k.name() == self.key)
    }

    /// Consume the press when exactly these modifiers are held.
    pub fn pressed(&self, i: &mut egui::InputState) -> bool {
        self.key().is_some_and(|k| crate::key(i, self.modifiers(), k))
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

/// Fixed shortcuts shown in the Keymap tab for reference (owned by their editors).
pub const FIXED_KEYMAP: &[(&str, &str)] = &[
    ("Tab", "Toggle Edit Mode"),
    ("Shift+A", "Add"),
    ("G / R / S", "Move / Rotate / Scale"),
    ("X / Y / Z (during transform)", "Constrain to axis"),
    ("A", "Select All"),
    ("B", "Box Select"),
    ("Z", "Shading pie"),
    ("Alt+Z", "Toggle X-Ray"),
    ("Numpad 1 / 3 / 7 / 5 / 0", "Front / Right / Top / Ortho / Camera"),
    ("Numpad .", "Frame Selected"),
    ("Ctrl+J", "Join"),
    ("P", "Separate"),
    ("Ctrl+B", "Bevel (edit)"),
    ("E / I / K", "Extrude / Inset / Knife (edit)"),
    ("1 / 2 / 3", "Vertex / Edge / Face select (edit)"),
    ("F / Shift+F", "Brush size / strength (sculpt)"),
    ("I / Alt+I", "Insert / Delete keyframe"),
    ("Space / Left / Right", "Play / Step frame"),
    ("F3", "Operator Search"),
    ("F12 / Ctrl+F12", "Render / Render Animation"),
    ("Ctrl+Space", "Maximize Area"),
    ("Ctrl+S / Ctrl+O / Ctrl+N", "Save / Open / New"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// Blender's Resolution Scale.
    pub ui_scale: f32,
    /// Viewport background (sRGB 0..1), Blender's theme > 3D Viewport > Gradient colour.
    pub background: [f32; 3],
    /// Rebound shortcuts; actions missing here use their default.
    #[serde(default)]
    pub keymap: BTreeMap<Action, KeyCombo>,
    /// Add-ons switched on, by file name.
    #[serde(default)]
    pub enabled_addons: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { ui_scale: 1.0, background: [0x3f as f32 / 255.0; 3], keymap: BTreeMap::new(), enabled_addons: Vec::new() }
    }
}

impl Prefs {
    pub fn combo(&self, a: Action) -> KeyCombo {
        self.keymap.get(&a).cloned().unwrap_or_else(|| a.default_combo())
    }

    pub fn pressed(&self, i: &mut egui::InputState, a: Action) -> bool {
        self.combo(a).pressed(i)
    }

    pub fn config_dir() -> PathBuf {
        if let Some(d) = std::env::var_os("SHAPE_CONFIG_DIR") {
            return PathBuf::from(d);
        }
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("redrob-shape")
    }

    pub fn path() -> PathBuf {
        Self::config_dir().join("prefs.json")
    }

    pub fn addons_dir() -> PathBuf {
        Self::config_dir().join("addons")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }
}

/// One Shape Script file in the add-ons folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Addon {
    pub file: String,
    pub info: shape_agent::script::AddonInfo,
    pub source: String,
}

/// Read every `*.shape` script in the add-ons folder.
pub fn scan_addons() -> Vec<Addon> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(Prefs::addons_dir()) else { return out };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("shape") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("addon").to_string();
        let info = shape_agent::script::addon_info(&source, file.trim_end_matches(".shape"));
        out.push(Addon { file, info, source });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Transient window state.
#[derive(Clone, Debug, Default)]
pub struct PrefsWindow {
    pub open: bool,
    pub tab: PrefsTab,
    /// Waiting for a key press to rebind this action.
    pub capturing: Option<Action>,
    pub addons: Vec<Addon>,
    pub scanned: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrefsTab {
    #[default]
    Interface,
    Keymap,
    Addons,
}

impl ShapeApp {
    pub(crate) fn preferences_window(&mut self, ctx: &egui::Context) {
        if !self.prefs_window.open {
            return;
        }
        if !self.prefs_window.scanned {
            self.prefs_window.addons = scan_addons();
            self.prefs_window.scanned = true;
        }
        let before = self.prefs.clone();
        let mut open = self.prefs_window.open;
        egui::Window::new("Preferences").open(&mut open).default_size([560.0, 420.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (t, name) in [(PrefsTab::Interface, "Interface"), (PrefsTab::Keymap, "Keymap"), (PrefsTab::Addons, "Add-ons")] {
                    ui.selectable_value(&mut self.prefs_window.tab, t, name);
                }
            });
            ui.separator();
            match self.prefs_window.tab {
                PrefsTab::Interface => {
                    ui.horizontal(|ui| {
                        ui.label("Resolution Scale");
                        ui.add(egui::Slider::new(&mut self.prefs.ui_scale, 0.5..=3.0).step_by(0.05));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Viewport background");
                        ui.color_edit_button_rgb(&mut self.prefs.background);
                    });
                    ui.label(RichText::new(format!("Saved to {}", Prefs::path().display())).color(theme::TEXT_WEAK).small());
                }
                PrefsTab::Keymap => self.keymap_tab(ui),
                PrefsTab::Addons => self.addons_tab(ui),
            }
        });
        self.prefs_window.open = open;
        if self.prefs != before {
            self.apply_prefs(ctx);
            if let Err(e) = self.prefs.save() {
                self.status = format!("Preferences not saved: {e}");
            }
        }
    }

    pub(crate) fn apply_prefs(&mut self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.prefs.ui_scale.clamp(0.5, 3.0));
        let b = self.prefs.background;
        self.viewport.options.background = [b[0], b[1], b[2], 1.0];
    }

    fn keymap_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Object mode (rebindable: click, then press a key)").color(theme::TEXT_WEAK));
            if ui.button("Restore Defaults").clicked() {
                self.prefs.keymap.clear();
            }
        });
        if let Some(action) = self.prefs_window.capturing {
            ui.label(RichText::new(format!("Press a key for {}… (Esc cancels)", action.name())).strong());
            let captured = ui.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    return Some(None);
                }
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if !matches!(key, egui::Key::Escape) => Some(Some(KeyCombo::new(*modifiers, *key))),
                    _ => None,
                })
            });
            if let Some(c) = captured {
                if let Some(c) = c {
                    self.prefs.keymap.insert(action, c);
                }
                self.prefs_window.capturing = None;
            }
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("keymap-grid").striped(true).num_columns(3).show(ui, |ui| {
                for a in Action::ALL {
                    ui.label(a.name());
                    let combo = self.prefs.combo(a);
                    let rebound = self.prefs.keymap.contains_key(&a);
                    let text = if self.prefs_window.capturing == Some(a) { "…".to_string() } else { combo.label() };
                    if ui.add(egui::Button::new(RichText::new(text).color(if rebound { theme::TEXT } else { theme::TEXT_WEAK })).min_size(egui::vec2(120.0, 0.0))).clicked() {
                        self.prefs_window.capturing = Some(a);
                    }
                    if rebound {
                        if ui.small_button("↺").on_hover_text("Restore default").clicked() {
                            self.prefs.keymap.remove(&a);
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
                ui.label(RichText::new("Fixed").color(theme::TEXT_WEAK));
                ui.label("");
                ui.label("");
                ui.end_row();
                for (k, name) in FIXED_KEYMAP {
                    ui.label(RichText::new(*name).color(theme::TEXT_WEAK));
                    ui.label(RichText::new(*k).color(theme::TEXT_WEAK));
                    ui.label("");
                    ui.end_row();
                }
            });
        });
    }

    fn addons_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Folder: {}", Prefs::addons_dir().display())).color(theme::TEXT_WEAK).small());
            if ui.button("Refresh").clicked() {
                self.prefs_window.addons = scan_addons();
            }
            if ui.button("Open Folder").clicked() {
                let dir = Prefs::addons_dir();
                let _ = std::fs::create_dir_all(&dir);
                self.status = format!("Add-ons folder: {}", dir.display());
            }
        });
        ui.label(RichText::new("A Shape Script (*.shape) with `# name:`, `# description:` and `# menu:` header lines; enabled add-ons appear under Scripts.").color(theme::TEXT_WEAK).small());
        ui.separator();
        if self.prefs_window.addons.is_empty() {
            ui.label(RichText::new("No add-ons found.").color(theme::TEXT_WEAK));
        }
        let addons = self.prefs_window.addons.clone();
        for a in &addons {
            let mut on = self.prefs.enabled_addons.contains(&a.file);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, RichText::new(&a.info.name).strong()).changed() {
                    if on {
                        self.prefs.enabled_addons.push(a.file.clone());
                    } else {
                        self.prefs.enabled_addons.retain(|f| f != &a.file);
                    }
                }
                ui.label(RichText::new(&a.file).color(theme::TEXT_WEAK).small());
                if ui.small_button("Run").clicked() {
                    self.run_script(&a.source, &a.info.name);
                }
            });
            if !a.info.description.is_empty() {
                ui.indent(&a.file, |ui| ui.label(RichText::new(&a.info.description).color(theme::TEXT_WEAK).small()));
            }
        }
    }

    /// Enabled add-ons that asked for a menu entry.
    pub(crate) fn addon_menu_entries(&self) -> Vec<(String, String, String)> {
        self.prefs_window
            .addons
            .iter()
            .filter(|a| self.prefs.enabled_addons.contains(&a.file))
            .filter_map(|a| a.info.menu.clone().map(|m| (m, a.source.clone(), a.info.name.clone())))
            .collect()
    }

    /// Run Shape Script text as one undo step.
    pub(crate) fn run_script(&mut self, source: &str, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut mutated = false;
        let mut scene = self.scene.clone();
        let (lines, m) = shape_agent::script::run(&mut scene, source);
        out.extend(lines);
        mutated |= m;
        if mutated {
            let label = format!("Script: {name}");
            self.edit(&label, move |s| *s = scene);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip_and_default_combos() {
        let dir = std::env::temp_dir().join(format!("shape-prefs-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let mut p = Prefs::default();
        p.keymap.insert(Action::Duplicate, KeyCombo::new(egui::Modifiers::ALT, egui::Key::D));
        p.ui_scale = 1.5;
        p.save_to(&path).unwrap();
        let back = Prefs::load_from(&path);
        assert_eq!(back, p);
        assert_eq!(back.combo(Action::Duplicate).label(), "Alt+D");
        assert_eq!(back.combo(Action::Undo).label(), "Ctrl+Z");
        assert_eq!(back.combo(Action::Undo).key(), Some(egui::Key::Z));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
