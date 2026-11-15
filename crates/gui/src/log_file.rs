use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const LOG_FILE: &str = "snisplit-log.txt";
const DESKTOP: &str = "Desktop";
const NOWHERE: &str = "no Desktop folder and no snisplit folder to save the file in";

pub fn save(
    lines: &[String],
    home: Option<&Path>,
    app_folder: Option<&Path>,
) -> io::Result<PathBuf> {
    let desktop = home
        .map(|home| home.join(DESKTOP))
        .filter(|desktop| desktop.is_dir());
    let folder = match (desktop, app_folder) {
        (Some(desktop), _) => desktop,
        (None, Some(app_folder)) => {
            fs::create_dir_all(app_folder)?;
            app_folder.to_path_buf()
        }
        (None, None) => return Err(io::Error::new(io::ErrorKind::NotFound, NOWHERE)),
    };
    let text: String = lines.iter().map(|line| format!("{line}\r\n")).collect();
    let path = folder.join(LOG_FILE);
    fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("snisplit-gui-log-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn lines() -> Vec<String> {
        vec!["starting snisplit".to_string(), "stopping".to_string()]
    }

    #[test]
    fn the_file_goes_on_the_desktop_when_the_home_folder_has_one_and_replaces_an_older_one() {
        let home = TempHome::new("desktop");
        let expected = home.0.join("Desktop").join("snisplit-log.txt");
        fs::create_dir(home.0.join("Desktop")).unwrap();
        fs::write(
            &expected,
            "an older log that is longer than the new one\r\n",
        )
        .unwrap();
        let app_folder = home.0.join("AppData").join("snisplit");
        let written = save(&lines(), Some(&home.0), Some(&app_folder)).unwrap();
        assert_eq!(written, expected);
        assert_eq!(
            fs::read_to_string(&expected).unwrap(),
            "starting snisplit\r\nstopping\r\n"
        );
        assert!(!app_folder.exists());
    }

    #[test]
    fn without_a_desktop_the_file_goes_to_the_snisplit_folder() {
        let home = TempHome::new("no-desktop");
        let app_folder = home.0.join("AppData").join("snisplit");
        let written = save(&lines(), Some(&home.0), Some(&app_folder)).unwrap();
        assert_eq!(written, home.0.join("AppData/snisplit/snisplit-log.txt"));
        assert_eq!(
            fs::read_to_string(&written).unwrap(),
            "starting snisplit\r\nstopping\r\n"
        );
        let nowhere = save(&lines(), None, None).unwrap_err();
        assert_eq!(nowhere.kind(), io::ErrorKind::NotFound);
    }
}
