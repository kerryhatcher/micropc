//! Makes `memory.x` visible to the linker and passes the cortex-m-rt linker
//! script to the firmware binary only (so host-side artifacts are unaffected).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out.join("memory.x"), include_bytes!("memory.x"))
        .expect("failed to write memory.x to OUT_DIR");
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
}
