//! Windows: the exe icon, version info and manifest (Per-Monitor-V2 DPI, common controls v6 for tooltips).
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build/app.manifest");
    println!("cargo:rerun-if-changed=build/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let v = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut n: Vec<u32> = v.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    n.resize(3, 0);
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("build");
    let path = |f: &str| dir.join(f).display().to_string().replace('\\', "/");
    let commas = format!("{},{},{},0", n[0], n[1], n[2]);
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "artmasster"
      VALUE "FileDescription", "Ai Quota Overlay"
      VALUE "FileVersion", "{v}"
      VALUE "InternalName", "Ai Quota Overlay"
      VALUE "OriginalFilename", "Ai Quota Overlay.exe"
      VALUE "ProductName", "Ai Quota Overlay"
      VALUE "ProductVersion", "{v}"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = path("icon.ico"),
        manifest = path("app.manifest"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&out, rc).expect("write app.rc");
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().expect("Windows resources");
}
