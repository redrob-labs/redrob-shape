//! Redrob Shape Setup: a branded per-user installer and uninstaller for Windows.
//!
//! ```text
//! redrob-shape-setup.exe               install (asks first)
//! redrob-shape-setup.exe --uninstall   remove (what Settings > Apps runs)
//! redrob-shape-setup.exe --demo        the install screens on a timer, writing nothing
//! redrob-shape-setup.exe --demo-uninstall
//! ```
//!
//! The same binary is the uninstaller: the install copies it to `%LOCALAPPDATA%\Redrob\Uninstall`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod install;
#[cfg(windows)]
mod win;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    #[cfg(windows)]
    {
        let mode = if has("--uninstall") || has("--demo-uninstall") { win::Mode::Uninstall } else { win::Mode::Install };
        let demo = has("--demo") || has("--demo-uninstall");
        std::process::exit(win::run(mode, demo));
    }
    #[cfg(not(windows))]
    {
        let _ = has;
        eprintln!(
            "redrob-shape-setup builds a Windows installer; this host is not Windows. payload={} bytes, version {}",
            install::PAYLOAD.len(),
            install::VERSION
        );
        std::process::exit(if install::has_payload() { 0 } else { 1 });
    }
}
