use std::fs;
use std::io;
use std::path::Path;

pub const LOG_FILE: &str = "snisplit-log.txt";
const DESKTOP: &str = "Desktop";
const NOWHERE: &str = "no Desktop folder and no snisplit folder to save the file in";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Desktop,
    SnisplitFolder,
}

pub fn save(lines: &[String], home: Option<&Path>, app_folder: Option<&Path>) -> io::Result<Place> {
    let desktop = home
        .map(|home| home.join(DESKTOP))
        .filter(|desktop| desktop.is_dir());
    let (folder, place) = match (desktop, app_folder) {
        (Some(desktop), _) => (desktop, Place::Desktop),
        (None, Some(app_folder)) => {
            fs::create_dir_all(app_folder)?;
            (app_folder.to_path_buf(), Place::SnisplitFolder)
        }
        (None, None) => return Err(io::Error::new(io::ErrorKind::NotFound, NOWHERE)),
    };
    let text: String = lines.iter().map(|line| format!("{line}\r\n")).collect();
    fs::write(folder.join(LOG_FILE), text)?;
    Ok(place)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn the_file_goes_on_the_desktop_when_the_home_folder_has_one() {
        let home = TempHome::new("desktop");
        fs::create_dir(home.0.join("Desktop")).unwrap();
        let app_folder = home.0.join("AppData").join("snisplit");
        let place = save(&lines(), Some(&home.0), Some(&app_folder)).unwrap();
        assert_eq!(place, Place::Desktop);
        assert_eq!(
            fs::read_to_string(home.0.join("Desktop").join("snisplit-log.txt")).unwrap(),
            "starting snisplit\r\nstopping\r\n"
        );
        assert!(!app_folder.exists());
    }

    #[test]
    fn without_a_desktop_the_file_goes_to_the_snisplit_folder() {
        let home = TempHome::new("no-desktop");
        let app_folder = home.0.join("AppData").join("snisplit");
        let place = save(&lines(), Some(&home.0), Some(&app_folder)).unwrap();
        assert_eq!(place, Place::SnisplitFolder);
        assert_eq!(
            fs::read_to_string(app_folder.join("snisplit-log.txt")).unwrap(),
            "starting snisplit\r\nstopping\r\n"
        );
        let nowhere = save(&lines(), None, None).unwrap_err();
        assert_eq!(nowhere.kind(), io::ErrorKind::NotFound);
    }
}
