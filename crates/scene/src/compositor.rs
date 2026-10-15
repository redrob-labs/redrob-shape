//! The compositor, reduced to what Blender's default use covers: a chain from `Render Layers`
//! through image nodes to `Composite`. The chain is ordered, so it is stored as a list rather
//! than a graph; each node is one image operation applied to the rendered RGBA buffer. The
//! operations themselves live in `shape-render` (`composite::apply`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CompositorNode {
    /// Gaussian blur of `size` pixels.
    Blur { size: u32 },
    /// Blender's Bright/Contrast: bright in [-100, 100], contrast in [-100, 100].
    BrightContrast { bright: f32, contrast: f32 },
    /// Bloom-style glare: pixels above `threshold` are blurred and added back scaled by `mix`.
    Glare { threshold: f32, mix: f32, size: u32 },
    /// Gamma correction.
    Gamma { gamma: f32 },
    /// Hue / saturation / value (hue is a 0..1 rotation like Blender's, 0.5 = unchanged).
    HueSaturation { hue: f32, saturation: f32, value: f32 },
    /// Mix with a flat colour by `factor`.
    MixColor { color: [f32; 3], factor: f32 },
    /// Invert RGB.
    Invert,
}

impl CompositorNode {
    pub const ALL: [CompositorNode; 7] = [
        CompositorNode::Blur { size: 4 },
        CompositorNode::BrightContrast { bright: 0.0, contrast: 0.0 },
        CompositorNode::Glare { threshold: 1.0, mix: 0.5, size: 8 },
        CompositorNode::Gamma { gamma: 1.0 },
        CompositorNode::HueSaturation { hue: 0.5, saturation: 1.0, value: 1.0 },
        CompositorNode::MixColor { color: [1.0, 1.0, 1.0], factor: 0.0 },
        CompositorNode::Invert,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            CompositorNode::Blur { .. } => "Blur",
            CompositorNode::BrightContrast { .. } => "Bright/Contrast",
            CompositorNode::Glare { .. } => "Glare",
            CompositorNode::Gamma { .. } => "Gamma",
            CompositorNode::HueSaturation { .. } => "Hue/Saturation/Value",
            CompositorNode::MixColor { .. } => "Mix Color",
            CompositorNode::Invert => "Invert Color",
        }
    }

    /// Blender's Add-menu category.
    pub fn category(&self) -> &'static str {
        match self {
            CompositorNode::Blur { .. } | CompositorNode::Glare { .. } => "Filter",
            _ => "Color",
        }
    }
}

/// `Render Layers -> nodes[0] -> ... -> Composite`. `enabled` is Blender's "Use Nodes".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CompositorTree {
    pub enabled: bool,
    pub nodes: Vec<CompositorNode>,
}
