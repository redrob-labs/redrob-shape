//! Native `.rshape`: the whole [`Scene`] as JSON. Human-diffable; a binary chunk format can come
//! later behind the same `load`/`save`.

use crate::{Result, Scene};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Bumped when the on-disk layout changes incompatibly.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    rshape: u32,
    generator: String,
    scene: Scene,
}

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let file = File { rshape: VERSION, generator: format!("Redrob Shape {}", env!("CARGO_PKG_VERSION")), scene: scene.clone() };
    std::fs::write(path, serde_json::to_vec_pretty(&file)?)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Scene> {
    let file: File = serde_json::from_slice(&std::fs::read(path)?)?;
    if file.rshape != VERSION {
        return Err(crate::parse_err("rshape", 0, format!("file version {} but this build reads {}", file.rshape, VERSION)));
    }
    Ok(file.scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};

    #[test]
    fn exact_round_trip() {
        let scene = sample_scene();
        let path = scratch("sample.rshape");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back, scene, "rshape must preserve the scene exactly");
    }
}
