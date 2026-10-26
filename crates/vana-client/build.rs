//! The Windows exe's icon: assets/vana-client.rc through windres (WINDRES, else MinGW's), linked into the binaries.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/vana-client.rc");
    println!("cargo:rerun-if-changed=assets/vana-client.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(&windres).current_dir("assets").args(["vana-client.rc", "-O", "coff", "-o"]).arg(&out).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        r => println!("cargo:warning=no exe icon: {windres} failed ({r:?})"),
    }
}
