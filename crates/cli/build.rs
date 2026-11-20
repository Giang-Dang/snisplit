#[path = "../../tools/icon/icon_resource.rs"]
mod icon_resource;

use std::env;
use std::path::PathBuf;

const ICON: &str = "../../assets/icon/snisplit.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=../../tools/icon/icon_resource.rs");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let res =
            PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("icon.res");
        icon_resource::write_resource_file(ICON.as_ref(), &res)
            .expect("the app icon becomes a resource file");
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}
