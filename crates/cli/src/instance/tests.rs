use std::path::PathBuf;

use super::*;

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("snisplit-instance-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn app(&self) -> PathBuf {
        self.0.join("snisplit")
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
#[test]
fn a_second_claim_is_refused_while_the_first_is_held_and_succeeds_once_it_is_dropped() {
    let folder = TempFolder::new("second");
    let first = claim(Some(&folder.app()));
    assert!(matches!(first, Claim::Ours(_)), "{first:?}");
    let second = claim(Some(&folder.app()));
    assert!(matches!(second, Claim::Elsewhere), "{second:?}");
    drop(first);
    let third = claim(Some(&folder.app()));
    assert!(matches!(third, Claim::Ours(_)), "{third:?}");
}

#[test]
fn a_lock_file_left_behind_by_a_killed_process_does_not_block() {
    let folder = TempFolder::new("stale");
    fs::create_dir_all(folder.app()).unwrap();
    fs::write(
        folder.app().join("instance.lock"),
        b"left by a killed process",
    )
    .unwrap();
    let claimed = claim(Some(&folder.app()));
    assert!(matches!(claimed, Claim::Ours(_)), "{claimed:?}");
}

#[test]
fn a_missing_folder_is_created_and_claimed() {
    let folder = TempFolder::new("missing");
    let claimed = claim(Some(&folder.app().join("nested")));
    assert!(matches!(claimed, Claim::Ours(_)), "{claimed:?}");
    assert!(folder.app().join("nested").join("instance.lock").is_file());
}

#[test]
fn a_folder_that_cannot_be_created_proceeds_unlocked_rather_than_held() {
    let folder = TempFolder::new("blocked");
    fs::create_dir_all(&folder.0).unwrap();
    let in_the_way = folder.0.join("snisplit");
    fs::write(&in_the_way, b"a file where the folder should be").unwrap();
    let claimed = claim(Some(&in_the_way));
    assert!(matches!(claimed, Claim::Unlocked(_)), "{claimed:?}");
}

#[test]
fn no_per_user_folder_proceeds_unlocked_and_says_why() {
    match claim(None) {
        Claim::Unlocked(error) => assert_eq!(
            error.to_string(),
            "no per-user snisplit folder: the APPDATA environment variable is not set"
        ),
        other => panic!("expected an unlocked claim, got {other:?}"),
    }
}
