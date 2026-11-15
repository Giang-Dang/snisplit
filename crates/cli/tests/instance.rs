#![cfg(windows)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use snisplit::instance::{self, Claim};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("snisplit-cli-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn program_data(&self) -> PathBuf {
        self.0.join("ProgramData")
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_routed_run_while_the_lock_is_held_is_a_runtime_error_with_the_one_line_message() {
    let root = TempRoot::new("run-held");
    let held = instance::claim(Some(&root.program_data().join("snisplit")));
    assert!(matches!(held, Claim::Ours(_)), "{held:?}");
    let missing_profile = root.0.join("missing.profile");
    let output = Command::new(env!("CARGO_BIN_EXE_snisplit"))
        .args(["run", "--profile-file"])
        .arg(&missing_profile)
        .env("ProgramData", root.program_data())
        .env("APPDATA", root.0.join("AppData"))
        .output()
        .expect("run snisplit");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "snisplit: error: another snisplit is already running, or is still turning off: wait a few seconds, then try again\n"
    );
}
