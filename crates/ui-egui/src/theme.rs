//! Colours and dimensions of the default screen, matched against Blender's default dark theme by
//! measuring a Blender window (packaging-free: the numbers below were read off a 1280x800 capture of
//! Blender 5.3 and are documented in ROADMAP.md, stage P1).

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const TOP_BAR: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const STATUS_BAR: Color32 = TOP_BAR;
pub const HEADER: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const VIEWPORT_BG: Color32 = Color32::from_rgb(0x3f, 0x3f, 0x3f);
pub const REGION: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
pub const REGION_ROW_ALT: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
pub const PROPERTIES_BG: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const PANEL: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);
pub const TABS_COLUMN: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1d);
pub const TIMELINE: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1d);
pub const WIDGET: Color32 = Color32::from_rgb(0x54, 0x54, 0x54);
pub const WIDGET_HOVER: Color32 = Color32::from_rgb(0x65, 0x65, 0x65);
pub const ACCENT: Color32 = Color32::from_rgb(0x47, 0x72, 0xb3);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const OUTLINE: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);

/// Region sizes in logical pixels, as Blender lays its default screen out at 1280x800.
pub const TOP_BAR_H: f32 = 26.0;
pub const HEADER_H: f32 = 24.0;
pub const STATUS_H: f32 = 20.0;
pub const RIGHT_COLUMN_W: f32 = 228.0;
pub const OUTLINER_H: f32 = 208.0;
pub const PROPS_TABS_W: f32 = 28.0;
pub const TOOLBAR_W: f32 = 50.0;
pub const SIDEBAR_W: f32 = 200.0;

/// Install the theme on the context. Called once before the first frame.
pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = PROPERTIES_BG;
    v.window_fill = HEADER;
    v.faint_bg_color = REGION_ROW_ALT;
    v.extreme_bg_color = Color32::from_rgb(0x1d, 0x1d, 0x1d);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, OUTLINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = WIDGET;
    v.widgets.inactive.weak_bg_fill = WIDGET;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, OUTLINE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = WIDGET_HOVER;
    v.widgets.hovered.weak_bg_fill = WIDGET_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, OUTLINE);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.open.bg_fill = Color32::from_rgb(0x42, 0x42, 0x42);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(0x42, 0x42, 0x42);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(4);
    }
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_stroke = Stroke::new(1.0, OUTLINE);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(4.0, 3.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size.y = 20.0;
    });
}

/// Frame of a region: Blender's editors have no border, just their fill.
pub fn region_frame(fill: Color32) -> egui::Frame {
    egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(6, 3))
}
