use std::env;

const MANIFEST_ARGUMENTS: [&str; 2] = [
    "/MANIFEST:EMBED",
    "/MANIFESTUAC:level='requireAdministrator' uiAccess='false'",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        for argument in MANIFEST_ARGUMENTS {
            println!("cargo:rustc-link-arg-bins={argument}");
        }
    }
}
