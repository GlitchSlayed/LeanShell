use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../assets/wisp.ico");
    println!("cargo:rerun-if-env-changed=LEANSHELL_EMBED_DIR");

    // Optionally embed release executables (leanshell.exe, leanshell-settings.exe) into payload.zip
    // when packaged with LEANSHELL_EMBED_DIR set so leanshell-setup.exe is a single self-contained installer.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
    let payload_zip = out_dir.join("payload.zip");
    if let Some(dir) = std::env::var_os("LEANSHELL_EMBED_DIR").filter(|s| !s.is_empty()) {
        let dir = PathBuf::from(dir);
        let file = std::fs::File::create(&payload_zip).expect("create payload.zip");
        let mut writer = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for name in ["leanshell.exe", "leanshell-settings.exe", "LICENSE", "README.md"] {
            let src = dir.join(name);
            if src.is_file() {
                println!("cargo:rerun-if-changed={}", src.display());
                let bytes = std::fs::read(&src).expect("read embedded asset");
                writer.start_file(name, opts).expect("start zip entry");
                writer.write_all(&bytes).expect("write zip entry");
            }
        }
        writer.finish().expect("finish payload.zip");
    } else {
        std::fs::write(&payload_zip, []).expect("write empty payload placeholder");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        let nightly = std::env::var_os("CARGO_FEATURE_NIGHTLY").is_some();
        res.set_icon_with_id("../assets/wisp.ico", "1");
        res.set(
            "FileDescription",
            &format!("LeanShell Setup{}", if nightly { " (Nightly)" } else { "" }),
        );
        res.set("ProductName", if nightly { "LeanShell Nightly" } else { "LeanShell" });
        res.set(
            "LegalCopyright",
            "Copyright (c) 2026 Sumir Seth, LeanShell Contributors. MIT License.",
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {e}");
        }
    }
}
