// Embeds the icon, version info and an admin manifest into svtoggle-gui.exe only.
// The CLI stays a plain exe so `svtoggle overwatch status` doesn't pop UAC.
mod icon {
    include!("src/icon.rs");
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges>
      <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
    </requestedPrivileges></security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-env-changed=SVTOGGLE_NO_MANIFEST");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("svtoggle.ico");
    std::fs::write(&out, icon::ico(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon(out.to_str().unwrap())
        .set("FileDescription", "svtoggle")
        .set("ProductName", "svtoggle");
    // dev previews: skip the admin manifest so a look-only build opens without UAC
    if std::env::var_os("SVTOGGLE_NO_MANIFEST").is_none() {
        res.set_manifest(MANIFEST);
    }
    res.compile_for("svtoggle-gui").expect("resource compile (needs the Windows SDK's rc.exe)");
}
