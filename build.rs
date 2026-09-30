//! Embeds the application icon (generated from the same procedural glyph as the tray icon) and the
//! application manifest (DPI awareness, Common Controls v6, Windows 10+ compatibility) into slc.exe.

#[path = "src/glyph.rs"]
#[allow(dead_code)]
mod glyph;

use std::io::Write;
use std::path::Path;

const ICON_SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64];

/// Builds a .ico with 32-bit BMP images (BGRA + AND mask).
fn ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .map(|&s| {
            let px = glyph::render_tile(s);
            let mut bmp = Vec::new();
            // BITMAPINFOHEADER (height doubled: color + mask).
            for v in [40u32, s, s * 2] {
                bmp.extend_from_slice(&v.to_le_bytes());
            }
            bmp.extend_from_slice(&1u16.to_le_bytes());
            bmp.extend_from_slice(&32u16.to_le_bytes());
            for _ in 0..6 {
                bmp.extend_from_slice(&0u32.to_le_bytes());
            }
            // Pixels bottom-up.
            for y in (0..s).rev() {
                for x in 0..s {
                    bmp.extend_from_slice(&px[(y * s + x) as usize].to_le_bytes());
                }
            }
            // AND mask: all zero (alpha channel is used), rows padded to 32 bits.
            let row = s.div_ceil(32) * 4;
            bmp.extend(std::iter::repeat_n(0u8, (row * s) as usize));
            (s, bmp)
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, data) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    out
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="MONTINA.SLC" version="1.0.0.0"/>
  <description>Screen Lighting Control</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=src/glyph.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);
    std::fs::write(out.join("slc.ico"), ico()).expect("write icon");
    std::fs::write(out.join("slc.manifest"), MANIFEST).expect("write manifest");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let parts: Vec<u32> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let v = |i: usize| parts.get(i).copied().unwrap_or(0);
    let mut rc = std::fs::File::create(out.join("slc.rc")).expect("write rc");
    write!(
        rc,
        r#"1 ICON "slc.ico"
1 24 "slc.manifest"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "MONTINA-Ops"
      VALUE "FileDescription", "Screen Lighting Control"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "slc"
      VALUE "OriginalFilename", "slc.exe"
      VALUE "ProductName", "Screen Lighting Control"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        a = v(0),
        b = v(1),
        c = v(2),
    )
    .expect("write rc");
    drop(rc);
    embed_resource::compile(out.join("slc.rc"), embed_resource::NONE)
        .manifest_required()
        .expect("embed resources");
}
