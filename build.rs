fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let numeric = format!("{},0", version.replace('.', ","));
        let resource = format!(
            r#"
#include <winver.h>
1 ICON "icon.ico"
1 24 "app.manifest"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Simple Aion Stabilizer"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "aion-stabilizer"
      VALUE "OriginalFilename", "AionStabilizer.exe"
      VALUE "ProductName", "Simple Aion Stabilizer"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
        );
        let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("app.rc");
        std::fs::write(&path, resource).unwrap();
        embed_resource::compile(path, embed_resource::NONE);
    }
}
