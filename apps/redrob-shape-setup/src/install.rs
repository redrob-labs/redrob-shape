//! What the setup does, independent of the window: where files go, the registry entry, the
//! shortcuts, and the reverse of all that. Everything here is per-user (`%LOCALAPPDATA%`), so no
//! elevation is needed, matching the other Redrob installers.

use std::path::{Path, PathBuf};

pub const PRODUCT: &str = "Redrob Shape";
pub const PUBLISHER: &str = "Redrob";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Settings > Apps key under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
pub const UNINSTALL_KEY: &str = "RedrobShape";
pub const APP_EXE: &str = "redrob-shape.exe";
pub const UNINSTALL_EXE: &str = "redrob-shape-uninstall.exe";

pub static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));

/// Licences shipped next to the exe, as the portable zip does.
pub const DOCS: &[(&str, &str)] = &[
    ("LICENSE-MIT", include_str!("../../../LICENSE-MIT")),
    ("LICENSE-APACHE", include_str!("../../../LICENSE-APACHE")),
    ("NOTICE", include_str!("../../../NOTICE")),
];

pub fn has_payload() -> bool {
    !PAYLOAD.is_empty()
}

/// Folders the install touches.
#[derive(Clone, Debug)]
pub struct Layout {
    /// `%LOCALAPPDATA%\Redrob\Shape`
    pub app_dir: PathBuf,
    /// `%LOCALAPPDATA%\Redrob\Uninstall` (shared with other Redrob products).
    pub uninstall_dir: PathBuf,
    /// Start menu `Programs` folder.
    pub programs_dir: PathBuf,
    pub desktop_dir: PathBuf,
}

impl Layout {
    pub fn new(local_app_data: &Path, programs: &Path, desktop: &Path) -> Self {
        Self {
            app_dir: local_app_data.join("Redrob").join("Shape"),
            uninstall_dir: local_app_data.join("Redrob").join("Uninstall"),
            programs_dir: programs.to_path_buf(),
            desktop_dir: desktop.to_path_buf(),
        }
    }
    pub fn app_exe(&self) -> PathBuf {
        self.app_dir.join(APP_EXE)
    }
    pub fn uninstall_exe(&self) -> PathBuf {
        self.uninstall_dir.join(UNINSTALL_EXE)
    }
    pub fn start_menu_link(&self) -> PathBuf {
        self.programs_dir.join(format!("{PRODUCT}.lnk"))
    }
    pub fn desktop_link(&self) -> PathBuf {
        self.desktop_dir.join(format!("{PRODUCT}.lnk"))
    }
}

/// Progress a step reports to the window: 0..=1 and a short label.
pub type Report<'a> = &'a mut dyn FnMut(f32, &str);

/// Write the app, the docs and the uninstaller. `self_exe` is this setup binary, copied as the
/// uninstaller. Returns the bytes written for the Settings > Apps size.
pub fn install_files(layout: &Layout, self_exe: &Path, report: Report) -> std::io::Result<u64> {
    report(0.05, "Preparing");
    std::fs::create_dir_all(&layout.app_dir)?;
    std::fs::create_dir_all(&layout.uninstall_dir)?;
    let mut bytes = 0u64;
    report(0.15, "Installing Redrob Shape");
    // Write to a temp name first so a half-written exe is never left in place.
    let tmp = layout.app_dir.join("redrob-shape.exe.part");
    std::fs::write(&tmp, PAYLOAD)?;
    // A running (or otherwise open) exe can be neither deleted nor replaced on Windows: stop with
    // that reason instead of carrying on and leaving a half-updated install.
    let replaced = match std::fs::remove_file(layout.app_exe()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => std::fs::rename(&tmp, layout.app_exe()),
    };
    if let Err(e) = replaced {
        let _ = std::fs::remove_file(&tmp);
        return Err(std::io::Error::new(e.kind(), format!("{PRODUCT} is in use. Close it and try again. ({e})")));
    }
    bytes += PAYLOAD.len() as u64;
    report(0.7, "Installing Redrob Shape");
    for (name, text) in DOCS {
        std::fs::write(layout.app_dir.join(name), text)?;
        bytes += text.len() as u64;
    }
    report(0.8, "Registering");
    let _ = std::fs::remove_file(layout.uninstall_exe());
    bytes += std::fs::copy(self_exe, layout.uninstall_exe())?;
    Ok(bytes)
}

/// Remove what `install_files` wrote (the uninstaller itself is deleted last, by the caller).
pub fn remove_files(layout: &Layout, report: Report) -> std::io::Result<()> {
    report(0.2, "Removing Redrob Shape");
    let _ = std::fs::remove_file(layout.start_menu_link());
    let _ = std::fs::remove_file(layout.desktop_link());
    if layout.app_dir.exists() {
        std::fs::remove_dir_all(&layout.app_dir)?;
    }
    // `Redrob` is shared with other products: only drop it when nothing else lives there.
    if let Some(parent) = layout.app_dir.parent()
        && parent.read_dir().map(|mut d| d.next().is_none()).unwrap_or(false)
    {
        let _ = std::fs::remove_dir(parent);
    }
    report(0.8, "Removing Redrob Shape");
    Ok(())
}

/// The command-line that Settings > Apps runs.
pub fn uninstall_command(layout: &Layout) -> String {
    format!("\"{}\" --uninstall", layout.uninstall_exe().display())
}

/// Install size in KB for `EstimatedSize`.
pub fn estimated_size_kb(bytes: u64) -> u32 {
    (bytes / 1024).min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths_and_uninstall_command() {
        let l = Layout::new(Path::new("/lad"), Path::new("/programs"), Path::new("/desk"));
        assert_eq!(l.app_exe(), Path::new("/lad/Redrob/Shape/redrob-shape.exe"));
        assert_eq!(l.uninstall_exe(), Path::new("/lad/Redrob/Uninstall/redrob-shape-uninstall.exe"));
        assert_eq!(l.start_menu_link(), Path::new("/programs/Redrob Shape.lnk"));
        assert!(uninstall_command(&l).ends_with("redrob-shape-uninstall.exe\" --uninstall"));
        assert_eq!(estimated_size_kb(3 * 1024 * 1024), 3072);
    }

    #[test]
    fn install_and_remove_round_trip_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("shape-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let l = Layout::new(&dir.join("lad"), &dir.join("programs"), &dir.join("desk"));
        std::fs::create_dir_all(&l.programs_dir).unwrap();
        let fake_self = dir.join("setup.exe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&fake_self, b"setup").unwrap();
        let mut steps = Vec::new();
        let bytes = install_files(&l, &fake_self, &mut |p, s| steps.push((p, s.to_string()))).unwrap();
        assert!(l.app_exe().exists());
        assert!(l.uninstall_exe().exists());
        assert!(l.app_dir.join("NOTICE").exists());
        assert!(bytes >= 5);
        assert!(steps.windows(2).all(|w| w[0].0 <= w[1].0), "progress is monotonic: {steps:?}");
        std::fs::write(l.start_menu_link(), b"lnk").unwrap();
        remove_files(&l, &mut |_, _| {}).unwrap();
        assert!(!l.app_dir.exists());
        assert!(!l.start_menu_link().exists());
        assert!(l.uninstall_exe().exists(), "the uninstaller deletes itself last, elsewhere");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
