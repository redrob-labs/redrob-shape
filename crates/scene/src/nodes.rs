//! Shader node trees, the way Blender's material nodes behave: a `Material Output` node takes a
//! shader from a `Principled BSDF`, whose inputs are either constants or linked from colour and
//! value nodes (`Image Texture`, `Mix Color`, `RGB`, `Value`). The tree is the editable form; the
//! flat [`Material`] fields are what the viewport and the path tracer read, so a tree is
//! *evaluated* into them after every change ([`NodeTree::evaluate`]).

use serde::{Deserialize, Serialize};

use crate::{Material, TextureId};

/// What a socket carries. Links are only allowed between sockets of the same type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketType {
    Color,
    Float,
    Shader,
}

/// A constant for an unlinked input (or the value of an `RGB` / `Value` node).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SocketValue {
    Color([f32; 4]),
    Float(f32),
    Shader,
}

impl SocketValue {
    pub fn color(self) -> [f32; 4] {
        match self {
            SocketValue::Color(c) => c,
            SocketValue::Float(f) => [f, f, f, 1.0],
            SocketValue::Shader => [0.0, 0.0, 0.0, 1.0],
        }
    }
    pub fn float(self) -> f32 {
        match self {
            SocketValue::Float(f) => f,
            SocketValue::Color([r, g, b, _]) => (r + g + b) / 3.0,
            SocketValue::Shader => 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    MaterialOutput,
    PrincipledBsdf,
    ImageTexture { texture: Option<TextureId> },
    MixColor,
    Rgb,
    Value,
}

/// A socket's name and type, as shown in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketDef {
    pub name: &'static str,
    pub ty: SocketType,
}

macro_rules! sockets {
    ($($name:literal : $ty:ident),* $(,)?) => {{
        const S: &[SocketDef] = &[$(SocketDef { name: $name, ty: SocketType::$ty }),*];
        S
    }};
}

impl NodeKind {
    pub const ALL: [NodeKind; 6] = [
        NodeKind::MaterialOutput,
        NodeKind::PrincipledBsdf,
        NodeKind::ImageTexture { texture: None },
        NodeKind::MixColor,
        NodeKind::Rgb,
        NodeKind::Value,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            NodeKind::MaterialOutput => "Material Output",
            NodeKind::PrincipledBsdf => "Principled BSDF",
            NodeKind::ImageTexture { .. } => "Image Texture",
            NodeKind::MixColor => "Mix Color",
            NodeKind::Rgb => "RGB",
            NodeKind::Value => "Value",
        }
    }

    /// Blender's Add-menu category.
    pub fn category(&self) -> &'static str {
        match self {
            NodeKind::MaterialOutput => "Output",
            NodeKind::PrincipledBsdf => "Shader",
            NodeKind::ImageTexture { .. } => "Texture",
            NodeKind::MixColor => "Color",
            NodeKind::Rgb | NodeKind::Value => "Input",
        }
    }

    pub fn inputs(&self) -> &'static [SocketDef] {
        match self {
            NodeKind::MaterialOutput => sockets!["Surface": Shader],
            NodeKind::PrincipledBsdf => sockets![
                "Base Color": Color,
                "Metallic": Float,
                "Roughness": Float,
                "Emission Color": Color,
                "Emission Strength": Float,
            ],
            NodeKind::ImageTexture { .. } => sockets![],
            NodeKind::MixColor => sockets!["Factor": Float, "A": Color, "B": Color],
            NodeKind::Rgb | NodeKind::Value => sockets![],
        }
    }

    pub fn outputs(&self) -> &'static [SocketDef] {
        match self {
            NodeKind::MaterialOutput => sockets![],
            NodeKind::PrincipledBsdf => sockets!["BSDF": Shader],
            NodeKind::ImageTexture { .. } => sockets!["Color": Color, "Alpha": Float],
            NodeKind::MixColor => sockets!["Result": Color],
            NodeKind::Rgb => sockets!["Color": Color],
            NodeKind::Value => sockets!["Value": Float],
        }
    }

    /// Default constants: one per input, or the single stored value of RGB / Value.
    pub fn default_values(&self) -> Vec<SocketValue> {
        match self {
            NodeKind::MaterialOutput => vec![SocketValue::Shader],
            NodeKind::PrincipledBsdf => vec![
                SocketValue::Color([0.8, 0.8, 0.8, 1.0]),
                SocketValue::Float(0.0),
                SocketValue::Float(0.5),
                SocketValue::Color([1.0, 1.0, 1.0, 1.0]),
                SocketValue::Float(0.0),
            ],
            NodeKind::ImageTexture { .. } => vec![],
            NodeKind::MixColor => vec![SocketValue::Float(0.5), SocketValue::Color([0.5, 0.5, 0.5, 1.0]), SocketValue::Color([0.5, 0.5, 0.5, 1.0])],
            NodeKind::Rgb => vec![SocketValue::Color([0.5, 0.5, 0.5, 1.0])],
            NodeKind::Value => vec![SocketValue::Float(0.5)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    /// Editor position (editor units, y down).
    pub pos: [f32; 2],
    /// Constants for unlinked inputs; the stored value for RGB and Value nodes.
    pub values: Vec<SocketValue>,
}

/// One connection, output socket of `from` into input socket of `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from_node: u32,
    pub from_socket: usize,
    pub to_node: u32,
    pub to_socket: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct NodeTree {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    next_id: u32,
}

/// A colour socket's evaluated result: a constant and, when an Image Texture feeds it, the texture
/// that multiplies the constant (the viewport and tracer only support one image per material).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorResult {
    pub color: [f32; 4],
    pub texture: Option<TextureId>,
}

impl NodeTree {
    /// Blender's "Use Nodes": a Principled BSDF carrying the flat material's values, wired into a
    /// Material Output.
    pub fn from_material(m: &Material) -> Self {
        let mut t = NodeTree::default();
        let bsdf = t.add(NodeKind::PrincipledBsdf, [40.0, 40.0]);
        let out = t.add(NodeKind::MaterialOutput, [320.0, 40.0]);
        t.link(bsdf, 0, out, 0);
        t.set_from_material(m);
        if let Some(tex) = m.base_color_texture {
            let img = t.add(NodeKind::ImageTexture { texture: Some(tex) }, [-220.0, 40.0]);
            t.link(img, 0, bsdf, 0);
        }
        t
    }

    pub fn add(&mut self, kind: NodeKind, pos: [f32; 2]) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let values = kind.default_values();
        self.nodes.push(Node { id, kind, pos, values });
        id
    }

    pub fn remove(&mut self, id: u32) {
        self.nodes.retain(|n| n.id != id);
        self.links.retain(|l| l.from_node != id && l.to_node != id);
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Connect an output to an input; an input takes one link, so an older one is replaced.
    /// Refused when the socket types differ or the link would form a cycle.
    pub fn link(&mut self, from_node: u32, from_socket: usize, to_node: u32, to_socket: usize) -> bool {
        let (Some(a), Some(b)) = (self.node(from_node), self.node(to_node)) else { return false };
        let (Some(o), Some(i)) = (a.kind.outputs().get(from_socket), b.kind.inputs().get(to_socket)) else { return false };
        if o.ty != i.ty || from_node == to_node || self.reaches(to_node, from_node) {
            return false;
        }
        self.unlink_input(to_node, to_socket);
        self.links.push(Link { from_node, from_socket, to_node, to_socket });
        true
    }

    pub fn unlink_input(&mut self, to_node: u32, to_socket: usize) {
        self.links.retain(|l| !(l.to_node == to_node && l.to_socket == to_socket));
    }

    pub fn input_link(&self, to_node: u32, to_socket: usize) -> Option<Link> {
        self.links.iter().copied().find(|l| l.to_node == to_node && l.to_socket == to_socket)
    }

    /// Does following links downstream from `from` reach `target`?
    fn reaches(&self, from: u32, target: u32) -> bool {
        let mut stack = vec![from];
        let mut seen = Vec::new();
        while let Some(n) = stack.pop() {
            if n == target {
                return true;
            }
            if seen.contains(&n) {
                continue;
            }
            seen.push(n);
            stack.extend(self.links.iter().filter(|l| l.from_node == n).map(|l| l.to_node));
        }
        false
    }

    /// The Principled BSDF feeding the output, if any.
    pub fn surface_bsdf(&self) -> Option<u32> {
        let out = self.nodes.iter().find(|n| n.kind == NodeKind::MaterialOutput)?;
        let link = self.input_link(out.id, 0)?;
        let src = self.node(link.from_node)?;
        (src.kind == NodeKind::PrincipledBsdf).then_some(src.id)
    }

    /// Write the flat material's values into the BSDF's unlinked inputs (keeps the panel and the
    /// node editor in step).
    pub fn set_from_material(&mut self, m: &Material) {
        let Some(bsdf) = self.surface_bsdf() else { return };
        let emission_strength = m.emissive.iter().cloned().fold(0.0f32, f32::max);
        let emission_color = if emission_strength > 0.0 {
            [m.emissive[0] / emission_strength, m.emissive[1] / emission_strength, m.emissive[2] / emission_strength, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        let new = [
            SocketValue::Color(m.base_color),
            SocketValue::Float(m.metallic),
            SocketValue::Float(m.roughness),
            SocketValue::Color(emission_color),
            SocketValue::Float(emission_strength),
        ];
        let linked: Vec<bool> = (0..5).map(|i| self.input_link(bsdf, i).is_some()).collect();
        if let Some(node) = self.node_mut(bsdf) {
            for (i, v) in new.into_iter().enumerate() {
                if !linked[i] && i < node.values.len() {
                    node.values[i] = v;
                }
            }
        }
    }

    fn eval_color(&self, node: u32, socket: usize, depth: u32) -> ColorResult {
        let white = ColorResult { color: [1.0; 4], texture: None };
        let Some(n) = self.node(node) else { return white };
        if depth > 64 {
            return white;
        }
        match &n.kind {
            NodeKind::Rgb => ColorResult { color: n.values.first().map(|v| v.color()).unwrap_or([0.5, 0.5, 0.5, 1.0]), texture: None },
            NodeKind::Value => {
                let f = n.values.first().map(|v| v.float()).unwrap_or(0.5);
                ColorResult { color: [f, f, f, 1.0], texture: None }
            }
            NodeKind::ImageTexture { texture } => {
                if socket == 1 {
                    white
                } else {
                    ColorResult { color: [1.0; 4], texture: *texture }
                }
            }
            NodeKind::MixColor => {
                let f = self.input_float(node, 0, depth + 1).clamp(0.0, 1.0);
                let a = self.input_color(node, 1, depth + 1);
                let b = self.input_color(node, 2, depth + 1);
                let mut c = [0.0; 4];
                for (i, out) in c.iter_mut().enumerate() {
                    *out = a.color[i] * (1.0 - f) + b.color[i] * f;
                }
                // One image per material: keep the texture of the side that has one (the one
                // weighted more when both do).
                let texture = match (a.texture, b.texture) {
                    (Some(t), None) => Some(t),
                    (None, Some(t)) => Some(t),
                    (Some(ta), Some(tb)) => Some(if f < 0.5 { ta } else { tb }),
                    (None, None) => None,
                };
                ColorResult { color: c, texture }
            }
            NodeKind::PrincipledBsdf | NodeKind::MaterialOutput => white,
        }
    }

    fn input_color(&self, node: u32, socket: usize, depth: u32) -> ColorResult {
        match self.input_link(node, socket) {
            Some(l) => self.eval_color(l.from_node, l.from_socket, depth),
            None => ColorResult {
                color: self.node(node).and_then(|n| n.values.get(socket)).map(|v| v.color()).unwrap_or([0.8, 0.8, 0.8, 1.0]),
                texture: None,
            },
        }
    }

    fn input_float(&self, node: u32, socket: usize, depth: u32) -> f32 {
        match self.input_link(node, socket) {
            Some(l) => {
                let c = self.eval_color(l.from_node, l.from_socket, depth);
                let src = self.node(l.from_node);
                match src.map(|n| &n.kind) {
                    Some(NodeKind::Value) => src.and_then(|n| n.values.first()).map(|v| v.float()).unwrap_or(0.5),
                    _ => (c.color[0] + c.color[1] + c.color[2]) / 3.0,
                }
            }
            None => self.node(node).and_then(|n| n.values.get(socket)).map(|v| v.float()).unwrap_or(0.0),
        }
    }

    /// Evaluate the tree into the flat material fields. Without a BSDF on the output the surface
    /// is black, as in Blender.
    pub fn evaluate(&self, m: &mut Material) {
        let Some(bsdf) = self.surface_bsdf() else {
            m.base_color = [0.0, 0.0, 0.0, 1.0];
            m.emissive = [0.0; 3];
            m.base_color_texture = None;
            return;
        };
        let base = self.input_color(bsdf, 0, 0);
        m.base_color = base.color;
        m.base_color_texture = base.texture;
        m.metallic = self.input_float(bsdf, 1, 0).clamp(0.0, 1.0);
        m.roughness = self.input_float(bsdf, 2, 0).clamp(0.0, 1.0);
        let ec = self.input_color(bsdf, 3, 0).color;
        let es = self.input_float(bsdf, 4, 0).max(0.0);
        m.emissive = [ec[0] * es, ec[1] * es, ec[2] * es];
    }
}

impl Material {
    /// Re-evaluate the node tree (when the material has one) into the flat fields.
    pub fn apply_nodes(&mut self) {
        if let Some(tree) = self.nodes.clone() {
            tree.evaluate(self);
        }
    }

    /// Set (or clear) the Base Color image. With nodes, this edits the Image Texture node wired
    /// into the BSDF's Base Color, adding or removing one as needed; without, the flat field.
    pub fn set_image_texture(&mut self, texture: Option<TextureId>) {
        self.base_color_texture = texture;
        let Some(mut tree) = self.nodes.take() else { return };
        if let Some(bsdf) = tree.surface_bsdf() {
            let existing = tree
                .input_link(bsdf, 0)
                .map(|l| l.from_node)
                .filter(|&n| matches!(tree.node(n).map(|n| &n.kind), Some(NodeKind::ImageTexture { .. })));
            match (texture, existing) {
                (Some(t), Some(img)) => {
                    if let Some(n) = tree.node_mut(img) {
                        n.kind = NodeKind::ImageTexture { texture: Some(t) };
                    }
                }
                (Some(t), None) => {
                    let pos = tree.node(bsdf).map(|n| [n.pos[0] - 220.0, n.pos[1]]).unwrap_or([0.0; 2]);
                    let img = tree.add(NodeKind::ImageTexture { texture: Some(t) }, pos);
                    tree.link(img, 0, bsdf, 0);
                }
                (None, Some(img)) => tree.remove(img),
                (None, None) => {}
            }
        }
        self.nodes = Some(tree);
        self.apply_nodes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_nodes_round_trips_the_flat_material() {
        let m = Material { base_color: [0.2, 0.4, 0.6, 1.0], metallic: 0.7, roughness: 0.3, emissive: [2.0, 1.0, 0.0], ..Default::default() };
        let tree = NodeTree::from_material(&m);
        let mut out = Material::default();
        tree.evaluate(&mut out);
        assert_eq!(out.base_color, m.base_color);
        assert_eq!((out.metallic, out.roughness), (0.7, 0.3));
        assert!((out.emissive[0] - 2.0).abs() < 1e-5 && (out.emissive[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mix_and_image_feed_the_base_colour() {
        let mut t = NodeTree::from_material(&Material::default());
        let bsdf = t.surface_bsdf().unwrap();
        let mix = t.add(NodeKind::MixColor, [0.0; 2]);
        let red = t.add(NodeKind::Rgb, [0.0; 2]);
        t.node_mut(red).unwrap().values[0] = SocketValue::Color([1.0, 0.0, 0.0, 1.0]);
        let img = t.add(NodeKind::ImageTexture { texture: Some(TextureId(3)) }, [0.0; 2]);
        let fac = t.add(NodeKind::Value, [0.0; 2]);
        t.node_mut(fac).unwrap().values[0] = SocketValue::Float(0.25);
        assert!(t.link(red, 0, mix, 1));
        assert!(t.link(img, 0, mix, 2));
        assert!(t.link(fac, 0, mix, 0));
        assert!(t.link(mix, 0, bsdf, 0));
        assert!(!t.link(fac, 0, bsdf, 0), "float into colour is refused");
        let mut m = Material::default();
        t.evaluate(&mut m);
        assert_eq!(m.base_color_texture, Some(TextureId(3)));
        assert!((m.base_color[0] - 1.0).abs() < 1e-6 && (m.base_color[1] - 0.25).abs() < 1e-6, "{:?}", m.base_color);
    }

    #[test]
    fn links_refuse_type_mismatch_and_cycles() {
        let mut t = NodeTree::default();
        let a = t.add(NodeKind::MixColor, [0.0; 2]);
        let b = t.add(NodeKind::MixColor, [0.0; 2]);
        let v = t.add(NodeKind::Value, [0.0; 2]);
        assert!(!t.link(v, 0, a, 1), "float output into colour input");
        assert!(t.link(v, 0, a, 0));
        assert!(t.link(a, 0, b, 1));
        assert!(!t.link(b, 0, a, 2), "cycle");
        assert!(t.link(a, 0, b, 2), "replacing is fine");
        assert_eq!(t.links.len(), 3);
        t.remove(a);
        assert_eq!(t.links.len(), 0);
    }

    #[test]
    fn no_bsdf_means_black() {
        let mut t = NodeTree::default();
        t.add(NodeKind::MaterialOutput, [0.0; 2]);
        let mut m = Material::default();
        t.evaluate(&mut m);
        assert_eq!(m.base_color, [0.0, 0.0, 0.0, 1.0]);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn set_image_texture_adds_and_removes_the_node() {
        let mut m = Material { nodes: Some(NodeTree::from_material(&Material::default())), ..Default::default() };
        m.set_image_texture(Some(TextureId(1)));
        assert_eq!(m.base_color_texture, Some(TextureId(1)));
        assert_eq!(m.nodes.as_ref().unwrap().nodes.len(), 3);
        m.set_image_texture(Some(TextureId(2)));
        assert_eq!(m.nodes.as_ref().unwrap().nodes.len(), 3, "re-uses the node");
        assert_eq!(m.base_color_texture, Some(TextureId(2)));
        m.set_image_texture(None);
        assert_eq!(m.nodes.as_ref().unwrap().nodes.len(), 2);
        assert_eq!(m.base_color_texture, None);
    }
}
