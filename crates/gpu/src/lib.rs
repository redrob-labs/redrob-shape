//! wgpu viewport: camera, grid, picking, shading modes.
//!
//! Stage 0 placeholder: the crate exists so the workspace layering is fixed from the first commit.
//! See ROADMAP.md for the stage that fills it in.

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-gpu";

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "shape-gpu");
    }
}
