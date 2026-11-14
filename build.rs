use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    // The "aio" build embeds SimConnect.dll.
    if env::var_os("CARGO_FEATURE_EMBED_SIMCONNECT").is_some() {
        println!("cargo:rerun-if-changed=vendor/SimConnect.dll");
        if !std::path::Path::new("vendor/SimConnect.dll").is_file() {
            panic!("vendor/SimConnect.dll is missing - run scripts/fetch-simconnect.sh first");
        }
    }

    // Windows file details (Properties -> Details) and the icon.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ver = env::var("CARGO_PKG_VERSION").unwrap();
        let mut nums: Vec<u16> = ver
            .split(['.', '-'])
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect();
        nums.resize(4, 0);
        let numver = format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]);
        let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/icon.ico")
            .to_string_lossy()
            .replace('\\', "/");
        let rc = format!(
            r#"#include <winver.h>
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numver}
PRODUCTVERSION {numver}
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "yan2k"
      VALUE "FileDescription", "Web CDU for the BlueBird 757 (MSFS)"
      VALUE "FileVersion", "{ver}"
      VALUE "InternalName", "bbs757-webcdu"
      VALUE "LegalCopyright", "Copyright (c) 2026 yan2k. PolyForm Noncommercial 1.0.0"
      VALUE "OriginalFilename", "bbs757-webcdu.exe"
      VALUE "ProductName", "bbs757-webcdu"
      VALUE "ProductVersion", "{ver}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
        );
        let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.rc");
        std::fs::write(&out, rc).unwrap();
        embed_resource::compile(&out, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
