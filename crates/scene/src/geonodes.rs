//! Geometry node trees, after Blender's Geometry Nodes modifier: a `Group Input` node hands the
//! object's mesh in, a `Group Output` node takes the result, and between them geometry flows
//! through sockets of a single `Geometry` type. Node parameters (sizes, offsets, levels) are
//! fields on the node, as in Blender when the socket is unlinked. Evaluation lives in `shape-ops`
//! (`geonodes::evaluate`); this module is the editable data.

use serde::{Deserialize, Serialize};
use shape_math::Vec3;

use crate::BooleanOp;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GeoNodeKind {
    GroupInput,
    GroupOutput,
    /// Move / rotate (Euler radians) / scale the geometry.
    Transform {
        translation: Vec3,
        rotation: Vec3,
        scale: Vec3,
    },
    /// Merge two geometries into one (Blender's multi-input socket is two inputs here).
    JoinGeometry,
    /// Linear subdivision `level` times (Blender: Subdivide Mesh).
    SubdivideMesh {
        level: u32,
    },
    /// Catmull-Clark `level` times (Blender: Subdivision Surface).
    SubdivisionSurface {
        level: u32,
    },
    /// Add `offset` to every position.
    SetPosition {
        offset: Vec3,
    },
    MeshCube {
        size: Vec3,
    },
    MeshUvSphere {
        radius: f32,
        segments: u32,
        rings: u32,
    },
    MeshCylinder {
        radius: f32,
        depth: f32,
        vertices: u32,
    },
    MeshGrid {
        size_x: f32,
        size_y: f32,
        vertices_x: u32,
        vertices_y: u32,
    },
    /// Put a copy of `Instance` at every vertex of `Points`.
    InstanceOnPoints {
        scale: f32,
    },
    MeshBoolean {
        op: BooleanOp,
    },
}

impl GeoNodeKind {
    pub const ALL: [GeoNodeKind; 13] = [
        GeoNodeKind::GroupInput,
        GeoNodeKind::GroupOutput,
        GeoNodeKind::Transform { translation: Vec3::ZERO, rotation: Vec3::ZERO, scale: Vec3::ONE },
        GeoNodeKind::JoinGeometry,
        GeoNodeKind::SubdivideMesh { level: 1 },
        GeoNodeKind::SubdivisionSurface { level: 1 },
        GeoNodeKind::SetPosition { offset: Vec3::ZERO },
        GeoNodeKind::MeshCube { size: Vec3::ONE },
        GeoNodeKind::MeshUvSphere { radius: 1.0, segments: 32, rings: 16 },
        GeoNodeKind::MeshCylinder { radius: 1.0, depth: 2.0, vertices: 32 },
        GeoNodeKind::MeshGrid { size_x: 1.0, size_y: 1.0, vertices_x: 3, vertices_y: 3 },
        GeoNodeKind::InstanceOnPoints { scale: 1.0 },
        GeoNodeKind::MeshBoolean { op: BooleanOp::Difference },
    ];

    pub fn name(&self) -> &'static str {
        match self {
            GeoNodeKind::GroupInput => "Group Input",
            GeoNodeKind::GroupOutput => "Group Output",
            GeoNodeKind::Transform { .. } => "Transform Geometry",
            GeoNodeKind::JoinGeometry => "Join Geometry",
            GeoNodeKind::SubdivideMesh { .. } => "Subdivide Mesh",
            GeoNodeKind::SubdivisionSurface { .. } => "Subdivision Surface",
            GeoNodeKind::SetPosition { .. } => "Set Position",
            GeoNodeKind::MeshCube { .. } => "Cube",
            GeoNodeKind::MeshUvSphere { .. } => "UV Sphere",
            GeoNodeKind::MeshCylinder { .. } => "Cylinder",
            GeoNodeKind::MeshGrid { .. } => "Grid",
            GeoNodeKind::InstanceOnPoints { .. } => "Instance on Points",
            GeoNodeKind::MeshBoolean { .. } => "Mesh Boolean",
        }
    }

    /// Blender's Add-menu category.
    pub fn category(&self) -> &'static str {
        match self {
            GeoNodeKind::GroupInput | GeoNodeKind::GroupOutput => "Group",
            GeoNodeKind::Transform { .. } | GeoNodeKind::JoinGeometry | GeoNodeKind::SetPosition { .. } => "Geometry",
            GeoNodeKind::SubdivideMesh { .. } | GeoNodeKind::SubdivisionSurface { .. } | GeoNodeKind::MeshBoolean { .. } => "Mesh",
            GeoNodeKind::MeshCube { .. } | GeoNodeKind::MeshUvSphere { .. } | GeoNodeKind::MeshCylinder { .. } | GeoNodeKind::MeshGrid { .. } => {
                "Mesh Primitives"
            }
            GeoNodeKind::InstanceOnPoints { .. } => "Instances",
        }
    }

    /// Geometry input socket names.
    pub fn inputs(&self) -> &'static [&'static str] {
        match self {
            GeoNodeKind::GroupInput
            | GeoNodeKind::MeshCube { .. }
            | GeoNodeKind::MeshUvSphere { .. }
            | GeoNodeKind::MeshCylinder { .. }
            | GeoNodeKind::MeshGrid { .. } => &[],
            GeoNodeKind::GroupOutput
            | GeoNodeKind::Transform { .. }
            | GeoNodeKind::SubdivideMesh { .. }
            | GeoNodeKind::SubdivisionSurface { .. }
            | GeoNodeKind::SetPosition { .. } => &["Geometry"],
            GeoNodeKind::JoinGeometry => &["Geometry", "Geometry 2"],
            GeoNodeKind::InstanceOnPoints { .. } => &["Points", "Instance"],
            GeoNodeKind::MeshBoolean { .. } => &["Mesh 1", "Mesh 2"],
        }
    }

    pub fn outputs(&self) -> &'static [&'static str] {
        match self {
            GeoNodeKind::GroupOutput => &[],
            GeoNodeKind::MeshCube { .. } | GeoNodeKind::MeshUvSphere { .. } | GeoNodeKind::MeshCylinder { .. } | GeoNodeKind::MeshGrid { .. } => {
                &["Mesh"]
            }
            GeoNodeKind::InstanceOnPoints { .. } => &["Instances"],
            GeoNodeKind::MeshBoolean { .. } => &["Mesh"],
            _ => &["Geometry"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoNode {
    pub kind: GeoNodeKind,
    /// Editor position.
    pub pos: [f32; 2],
}

/// `from_node.output[from_socket]` feeds `to_node.input[to_socket]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeoLink {
    pub from_node: usize,
    pub from_socket: usize,
    pub to_node: usize,
    pub to_socket: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct GeoNodeTree {
    pub nodes: Vec<GeoNode>,
    pub links: Vec<GeoLink>,
}

impl GeoNodeTree {
    /// Blender's default tree: Group Input linked straight to Group Output.
    pub fn default_tree() -> Self {
        let mut t = Self::default();
        t.nodes.push(GeoNode { kind: GeoNodeKind::GroupInput, pos: [-200.0, 0.0] });
        t.nodes.push(GeoNode { kind: GeoNodeKind::GroupOutput, pos: [200.0, 0.0] });
        t.links.push(GeoLink { from_node: 0, from_socket: 0, to_node: 1, to_socket: 0 });
        t
    }

    pub fn add_node(&mut self, kind: GeoNodeKind, pos: [f32; 2]) -> usize {
        self.nodes.push(GeoNode { kind, pos });
        self.nodes.len() - 1
    }

    /// Remove a node and every link touching it; later node indices shift down.
    pub fn remove_node(&mut self, index: usize) {
        if index >= self.nodes.len() {
            return;
        }
        self.nodes.remove(index);
        self.links.retain(|l| l.from_node != index && l.to_node != index);
        for l in &mut self.links {
            if l.from_node > index {
                l.from_node -= 1;
            }
            if l.to_node > index {
                l.to_node -= 1;
            }
        }
    }

    /// Connect, replacing whatever fed that input before. Refused when it would form a cycle.
    pub fn link(&mut self, from_node: usize, from_socket: usize, to_node: usize, to_socket: usize) -> bool {
        if from_node == to_node || from_node >= self.nodes.len() || to_node >= self.nodes.len() {
            return false;
        }
        if self.nodes[from_node].kind.outputs().len() <= from_socket || self.nodes[to_node].kind.inputs().len() <= to_socket {
            return false;
        }
        if self.reaches(to_node, from_node) {
            return false;
        }
        self.links.retain(|l| !(l.to_node == to_node && l.to_socket == to_socket));
        self.links.push(GeoLink { from_node, from_socket, to_node, to_socket });
        true
    }

    /// Whether geometry flowing out of `from` can reach `to` through links.
    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut stack = vec![from];
        let mut seen = vec![false; self.nodes.len()];
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if n < seen.len() && !seen[n] {
                seen[n] = true;
                stack.extend(self.links.iter().filter(|l| l.from_node == n).map(|l| l.to_node));
            }
        }
        false
    }

    pub fn input_link(&self, node: usize, socket: usize) -> Option<&GeoLink> {
        self.links.iter().find(|l| l.to_node == node && l.to_socket == socket)
    }

    pub fn output_node(&self) -> Option<usize> {
        self.nodes.iter().position(|n| n.kind == GeoNodeKind::GroupOutput)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_replace_inputs_and_refuse_cycles() {
        let mut t = GeoNodeTree::default_tree();
        let cube = t.add_node(GeoNodeKind::MeshCube { size: Vec3::ONE }, [0.0, 0.0]);
        assert!(t.link(cube, 0, 1, 0), "cube replaces group input on the output");
        assert_eq!(t.links.len(), 1);
        assert_eq!(t.input_link(1, 0).unwrap().from_node, cube);
        let sub = t.add_node(GeoNodeKind::SubdivideMesh { level: 1 }, [0.0, 0.0]);
        assert!(t.link(cube, 0, sub, 0));
        assert!(t.link(sub, 0, 1, 0));
        assert!(!t.link(1, 0, sub, 0), "group output has no outputs");
        let join = t.add_node(GeoNodeKind::JoinGeometry, [0.0, 0.0]);
        assert!(t.link(sub, 0, join, 0));
        assert!(!t.link(join, 0, cube, 0), "cube has no inputs");
        assert!(!t.link(join, 0, sub, 0), "cycle refused");
        t.remove_node(cube);
        assert!(t.links.iter().all(|l| l.from_node < t.nodes.len() && l.to_node < t.nodes.len()), "links shifted");
        assert!(t.links.contains(&GeoLink { from_node: sub - 1, from_socket: 0, to_node: join - 1, to_socket: 0 }));
        assert!(t.input_link(sub - 1, 0).is_none(), "the cube's link is gone");
        assert_eq!(t.nodes[sub - 1].kind, GeoNodeKind::SubdivideMesh { level: 1 });
    }
}
