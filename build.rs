use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=rc.exe not found; skipping Windows PE resource embedding");
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let (maj, min, pat, rev) = parse_pkg_version();

    let ico_path = out_dir.join("app.ico");
    let manifest_path = out_dir.join("app.manifest");
    let rc_path = out_dir.join("app.rc");
    let res_path = out_dir.join("app.res");

    fs::write(&ico_path, build_multi_size_ico(&[16, 32, 48])).expect("failed to write app.ico");
    fs::write(&manifest_path, build_manifest(maj, min, pat, rev))
        .expect("failed to write app.manifest");
    fs::write(&rc_path, build_rc_script(maj, min, pat, rev)).expect("failed to write app.rc");

    let status = Command::new(&rc_exe)
        .current_dir(&out_dir)
        .args(["/nologo", "/fo", "app.res", "app.rc"])
        .status()
        .expect("failed to execute rc.exe");

    if !status.success() {
        panic!("rc.exe failed with exit status: {}", status);
    }

    println!("cargo:rustc-link-arg-bin=ai-token-hud={}", res_path.display());
}

fn parse_pkg_version() -> (u16, u16, u16, u16) {
    let ver = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.2.1".to_string());
    let mut parts = ver
        .split(['.', '-', '+'])
        .filter_map(|s| s.parse::<u16>().ok());
    let maj = parts.next().unwrap_or(0);
    let min = parts.next().unwrap_or(2);
    let pat = parts.next().unwrap_or(1);
    (maj, min, pat, 0)
}

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let arch_dir = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };

    for root_env in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Ok(root) = env::var(root_env) else {
            continue;
        };
        let bin_dir = Path::new(&root).join(r"Windows Kits\10\bin");
        let Ok(entries) = fs::read_dir(&bin_dir) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        versions.reverse();

        for ver_dir in versions {
            let candidate = ver_dir.join(arch_dir).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn build_manifest(maj: u16, min: u16, pat: u16, rev: u16) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="{maj}.{min}.{pat}.{rev}" processorArchitecture="*" name="WuDuKangBaZi.AiTokenHud" type="win32"/>
  <description>AI Token HUD - Local AI Token and Cost Telemetry</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#
    )
}

fn build_rc_script(maj: u16, min: u16, pat: u16, rev: u16) -> String {
    let ver_str = format!("{}.{}.{}.{}", maj, min, pat, rev);
    format!(
        r#"1 ICON "app.ico"
1 24 "app.manifest"

1 VERSIONINFO
FILEVERSION {maj},{min},{pat},{rev}
PRODUCTVERSION {maj},{min},{pat},{rev}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "WuDuKangBaZi (Open Source)\0"
      VALUE "FileDescription", "AI Token HUD - Local AI Token & Cost Monitor\0"
      VALUE "FileVersion", "{ver_str}\0"
      VALUE "InternalName", "ai-token-hud\0"
      VALUE "LegalCopyright", "MIT License - github.com/WuDuKangBaZi/ai-token-hud\0"
      VALUE "OriginalFilename", "ai-token-hud.exe\0"
      VALUE "ProductName", "AI Token HUD\0"
      VALUE "ProductVersion", "{ver_str}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
    )
}

fn build_multi_size_ico(sizes: &[u32]) -> Vec<u8> {
    let mut payloads: Vec<(u32, Vec<u8>)> = Vec::with_capacity(sizes.len());
    for &sz in sizes {
        payloads.push((sz, build_ico_dib(sz)));
    }

    let mut out = Vec::new();
    // ICONDIR
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(payloads.len() as u16).to_le_bytes());

    let mut offset = (6 + 16 * payloads.len()) as u32;
    for (sz, dib) in &payloads {
        let dim = if *sz >= 256 { 0u8 } else { *sz as u8 };
        out.push(dim); // bWidth
        out.push(dim); // bHeight
        out.push(0); // bColorCount
        out.push(0); // bReserved
        out.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
        out.extend_from_slice(&32u16.to_le_bytes()); // wBitCount
        out.extend_from_slice(&(dib.len() as u32).to_le_bytes()); // dwBytesInRes
        out.extend_from_slice(&offset.to_le_bytes()); // dwImageOffset
        offset += dib.len() as u32;
    }

    for (_, dib) in payloads {
        out.extend_from_slice(&dib);
    }
    out
}

fn build_ico_dib(size: u32) -> Vec<u8> {
    let and_row_bytes = size.div_ceil(32) * 4;
    let color_bytes = size * size * 4;
    let mask_bytes = and_row_bytes * size;
    let mut buf = Vec::with_capacity((40 + color_bytes + mask_bytes) as usize);

    // BITMAPINFOHEADER (40 bytes)
    buf.extend_from_slice(&40u32.to_le_bytes()); // biSize
    buf.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    buf.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight (color + AND mask)
    buf.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    buf.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    buf.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    buf.extend_from_slice(&color_bytes.to_le_bytes()); // biSizeImage
    buf.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    buf.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    buf.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    buf.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    let center = (size as f32 - 1.0) * 0.5;
    let outer_r = size as f32 * 0.4625;
    let ring_r = outer_r * 0.77;
    let core_r = outer_r * 0.35;
    let aa = (size as f32 / 32.0).max(0.75);

    let mut and_mask = vec![0u8; mask_bytes as usize];

    // Bottom-up BGRA scanlines matching tray ring palette
    for row in 0..size {
        let y = size - 1 - row;
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= outer_r {
                let alpha = ((outer_r - dist) / aa).clamp(0.0, 1.0);
                let (r, g, b) = if dist <= core_r {
                    (96u8, 165u8, 250u8)
                } else if dist <= ring_r {
                    (15u8, 23u8, 42u8)
                } else {
                    (52u8, 211u8, 153u8)
                };
                buf.extend_from_slice(&[b, g, r, (alpha * 255.0) as u8]);
            } else {
                buf.extend_from_slice(&[0, 0, 0, 0]);
                let byte_idx = (row * and_row_bytes + (x / 8)) as usize;
                let bit_mask = 0x80u8 >> (x % 8);
                and_mask[byte_idx] |= bit_mask;
            }
        }
    }

    buf.extend_from_slice(&and_mask);
    buf
}
