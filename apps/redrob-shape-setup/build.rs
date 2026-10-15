//! Embeds the payload (the signed `redrob-shape.exe` named by `REDROB_SHAPE_PAYLOAD`) and, on
//! Windows, the icon and version resource. Without the variable the setup is a demo-only build
//! that installs nothing (`--demo`).

use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-env-changed=REDROB_SHAPE_PAYLOAD");
    let payload = out.join("payload.bin");
    match std::env::var_os("REDROB_SHAPE_PAYLOAD") {
        Some(p) => {
            println!("cargo:rerun-if-changed={}", p.to_string_lossy());
            std::fs::copy(&p, &payload).unwrap_or_else(|e| panic!("copy payload {}: {e}", p.to_string_lossy()));
            println!("cargo:rustc-cfg=has_payload");
        }
        None => {
            std::fs::write(&payload, []).expect("write empty payload");
        }
    }
    println!("cargo:rustc-check-cfg=cfg(has_payload)");
    #[cfg(windows)]
    {
        let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/brand/icon.ico");
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon.to_string_lossy());
        res.set("ProductName", "Redrob Shape");
        res.set("FileDescription", "Redrob Shape Setup");
        res.set("CompanyName", "Redrob");
        res.set("LegalCopyright", "Copyright (c) 2026 Janghoon Lee (Redrob) and contributors");
        res.set("ProductVersion", version);
        res.set("FileVersion", version);
        // Per-user install: no elevation prompt.
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
  </windowsSettings></application>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=resource compile skipped: {e}");
        }
    }
}
