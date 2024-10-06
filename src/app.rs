use dirs::{config_local_dir, document_dir};

use std::{
    any::Any,
    char::from_digit,
    fs::{copy, create_dir_all, read_dir, remove_dir_all},
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{
    prelude::{DateTime, Local},
    TimeZone,
};
use registry::{Hive, Security};
use serde::{Deserialize, Serialize};
use serde_json::{
    de::from_reader,
    ser::{to_string_pretty, to_writer_pretty},
};

use thiserror::Error;

// region: Constants

const TO_COPY: [(&str, &str); 5] = [
    (r"Instances\BigChadGuys Plus (w Cobblemon)\options.txt", r""),
    (r"Instances\BigChadGuys Plus (w Cobblemon)\saves", r"saves"),
    (r"Instances\BigChadGuys Plus (w Cobblemon)\local", r"local"),
    (
        r"Instances\BigChadGuys Plus (w Cobblemon)\journeymap\data",
        r"journeymap\data",
    ),
    (
        r"Instances\BigChadGuys Plus (w Cobblemon)\journeymap\config",
        r"journeymap\config",
    ),
];

pub const TITLE: &str = " Crucible ";

pub const TIPS_MAIN: [(&str, &str); 5] = [
    ("m", "anually back up"),
    ("s", "ettings"),
    ("b", "ackups"),
    ("q", "uit"),
    ("", ""),
];
pub const TIPS_SETTINGS: [(&str, &str); 5] = [
    ("m", "ax backups"),
    ("t", "argets"),
    ("f", "requency"),
    ("p", "ath"),
    ("q", "uit"),
];
pub const TIPS_BACKUPS: [(&str, &str); 5] = [
    ("r", "estore"),
    ("d", "elete"),
    ("q", "uit"),
    ("", ""),
    ("", ""),
];
pub const TIPS_TARGETS: [(&str, &str); 5] = [
    ("a", "dd"),
    ("r", "emove"),
    ("e", "dit"),
    ("q", "uit"),
    ("", ""),
];
pub const TIPS_CONFIRM: [(&str, &str); 3] = [("y", "es"), ("n", "o"), ("q", "uit")];

// endregion: Constants

// region: Core classes

#[derive(Serialize, Deserialize, Clone)]
pub struct Configuration {
    pub path: PathBuf,
    pub frequency: Duration,
    pub targets: Vec<(String, String)>,
    pub max_backups: u8,
}

impl Default for Configuration {
    fn default() -> Configuration {
        Configuration {
            path: match document_dir() {
                Some(d) => d.join("BCG Backups"),
                None => PathBuf::from("./"),
            },
            frequency: Duration::from_secs(60 * 15),
            targets: TO_COPY
                .map(|pair| (pair.0.to_string(), pair.1.to_string()))
                .to_vec(),
            max_backups: 10,
        }
    }
}

impl std::fmt::Display for Configuration {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "target: '{}', frequency: {} seconds, max_backups: {}",
            self.path.display(),
            self.frequency.as_secs().to_string(),
            self.max_backups,
        )
    }
}

// endregion: Core classes

// region: Custom enums
#[derive(Copy, Clone, PartialEq)]
pub enum CurrentScreen {
    Main,
    Settings,
    Backups,
    Targets,
    ConfirmRestore,
    ConfirmRemove,
}

#[derive(Clone, Copy, PartialEq)]
pub enum EditSetting {
    Path,
    Targets,
    Frequency,
    Max,
    None,
}

// endregion: Custom enums

// region: Error types

#[derive(Error, Debug)]
pub enum BackupError {
    #[error("error opening file '`{0}`'")]
    FileError(#[from] std::io::Error),
    #[error("unable to create directory '`{0}`'")]
    TargetFolderError(String),
    #[error("std::io::error -> '`{0}`'")]
    CopyFileError(std::io::Error),
    #[error("unable to remove directory '`{0}`'")]
    RemoveFolderError(std::io::Error),
}

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("cannot find installation of `{0}`")]
    NotInstalled(String),
    #[error("error with file; `{0}`")]
    FileError(#[from] std::io::Error),
    #[error("`{0}`")]
    Error(String),
    #[error("Error joining worker thread after non-erroneous drawing loop: `{0:?}`")]
    JustBackupWorker(Box<dyn Any + Send>),
    #[error("Error joining worker thread after erroneous drawing loop: `{0:?}`\n\n`{1}`")]
    LoopAndBackupWorker(Box<dyn Any + Send>, String),
}

pub type BackupResult<T> = std::result::Result<T, BackupError>;
pub type CodeResult<T> = std::result::Result<T, GeneralError>;

// endregion Error types

// region: Helper functions

pub fn retrieve_minecraft_path() -> CodeResult<PathBuf> {
    match Hive::CurrentUser.open(r"Software\Overwolf\CurseForge", Security::Read) {
        Ok(regkey) => match regkey.value("minecraft_root") {
            Ok(data) => Ok(PathBuf::from(data.to_string())),
            Err(_) => return Err(GeneralError::NotInstalled(String::from("Minecraft"))),
        },
        Err(_) => return Err(GeneralError::NotInstalled(String::from("CurseForge"))),
    }
}

#[test]
pub fn test_registry_access() {
    match retrieve_minecraft_path() {
        Ok(path) => println!("Success: {}", path.display()),
        Err(e) => println!("Failure: {}", e),
    }
}

pub fn get_config_path() -> CodeResult<PathBuf> {
    let mut config_path = match config_local_dir() {
        Some(path) => path,
        None => {
            return Err(GeneralError::Error(String::from(
                "no userdir or unknown OS",
            )));
        }
    }
    .join("BCG Backupper");

    match std::fs::create_dir_all(config_path.clone()) {
        Ok(_) => {}
        Err(e) => {
            return Err(GeneralError::FileError(e));
        }
    }
    config_path.push("config.json");

    Ok(config_path)
}

#[test]
pub fn test_config_path() {
    let conf_path = get_config_path();
    assert!(conf_path.is_ok());
    assert!(conf_path
        // .clone()
        .as_ref()
        .unwrap()
        .components()
        .any(|p| p == std::path::Component::Normal(std::ffi::OsStr::new("BCG Backupper"))));
    println!("{}", &conf_path.unwrap().to_str().unwrap());
}

pub fn read_config(file: std::fs::File) -> CodeResult<Configuration> {
    match file.metadata() {
        Ok(meta) => {
            if meta.len() == 0 {
                let conf = Configuration::default();
                match to_writer_pretty(file, &conf) {
                    Ok(_) => Ok(conf),
                    Err(e) => {
                        return Err(GeneralError::Error(e.to_string()));
                    }
                }
            } else {
                match from_reader(file) {
                    Ok(c) => Ok(c),
                    Err(e) => {
                        return Err(GeneralError::Error(e.to_string()));
                    }
                }
            }
        }
        Err(e) => {
            return Err(GeneralError::Error(e.to_string()));
        }
    }
}

pub fn write_config(mut file: std::fs::File, config: Configuration) -> CodeResult<()> {
    match file.seek(SeekFrom::Start(0)) {
        Ok(_) => {}
        Err(e) => {
            return Err(GeneralError::FileError(e));
        }
    }

    match to_writer_pretty(&file, &config) {
        Ok(_) => {}
        Err(e) => {
            return Err(GeneralError::Error(e.to_string()));
        }
    }

    let s_config = match to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            return Err(GeneralError::Error(e.to_string()));
        }
    };

    match file.set_len(s_config.len() as u64) {
        Ok(_) => {}
        Err(e) => {
            return Err(GeneralError::FileError(e));
        }
    }

    Ok(())
}

#[test]
pub fn test_write_config() {
    let config = Configuration {
        path: PathBuf::from(r"C:\TEMP\BCG"),
        frequency: Duration::from_secs(60 * 15),
        targets: TO_COPY
            .map(|pair| (pair.0.to_string(), pair.1.to_string()))
            .to_vec(),
        max_backups: 10,
    };

    let filepath = match get_config_path() {
        Ok(p) => p,
        Err(e) => {
            println!("Error encountered: {}", e);
            assert!(false);
            return;
        }
    };

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(filepath)
    {
        Ok(f) => f,
        Err(e) => {
            println!("Error: {}", e);
            assert!(false);
            return;
        }
    };

    match write_config(file, config) {
        Ok(_) => {}
        Err(_) => assert!(false),
    }
}

/// Safely compare two `std::time::Duration` objects, returning a default value
/// (or 0.0 if provided default is negative).
pub fn duration_compare(left: Duration, right: Duration, on_error: Option<f64>) -> Duration {
    std::panic::set_hook(Box::new(|_info| {}));
    let result = match std::panic::catch_unwind(|| left - right) {
        Ok(s) => s,
        Err(_) => Duration::from_secs_f64(match on_error {
            Some(val) => {
                if val < 0.0 {
                    0.0
                } else {
                    val
                }
            }
            None => 0.0,
        }),
    };
    let _ = std::panic::take_hook();
    result
}

#[test]
pub fn test_duration_compare() {
    assert_eq!(
        duration_compare(Duration::from_secs(4), Duration::from_secs(2), None).as_secs(),
        2
    );
    assert_eq!(
        duration_compare(Duration::from_secs(2), Duration::from_secs(4), Some(0.0)).as_secs(),
        0
    );
    assert_eq!(
        duration_compare(Duration::from_secs(2), Duration::from_secs(4), Some(-1.0)).as_secs(),
        0
    );
    assert_eq!(
        duration_compare(Duration::from_secs(2), Duration::from_secs(4), Some(2.1)).as_secs(),
        2
    );
}

pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    create_dir_all(&dst)?;
    if src.as_ref().is_file() {
        copy(
            &src,
            dst.as_ref().join(match src.as_ref().file_name() {
                Some(v) => v,
                None => std::ffi::OsStr::new("unknown"),
            }),
        )?;
    } else {
        for entry in read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
            } else {
                copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
            }
        }
    }
    Ok(())
}

pub fn get_backups_sorted(config: &Configuration) -> BackupResult<Vec<(DateTime<Local>, PathBuf)>> {
    let mut dirs: Vec<(DateTime<Local>, PathBuf)> = std::vec::Vec::new();
    for entry in read_dir(&config.path)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            match entry.file_name().to_str() {
                Some(s) => {
                    let parts = s
                        .split(['-', ' '])
                        .map(|a| a.parse::<u32>())
                        .collect::<Vec<_>>();
                    if parts.len() != 6 {
                        continue;
                    } else if parts.iter().any(|a| a.is_err()) {
                        continue;
                    }
                    dirs.push((
                        Local
                            .with_ymd_and_hms(
                                *parts[0].as_ref().unwrap() as i32,
                                *parts[1].as_ref().unwrap(),
                                *parts[2].as_ref().unwrap(),
                                *parts[3].as_ref().unwrap(),
                                *parts[4].as_ref().unwrap(),
                                *parts[5].as_ref().unwrap(),
                            )
                            .unwrap(),
                        entry.path(),
                    ));
                }
                None => {}
            }
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(dirs)
}

pub fn remove_old_backups(config: &Configuration) -> BackupResult<()> {
    let dirs = get_backups_sorted(config)?;
    if dirs.len() > config.max_backups as usize {
        for i in 0..(dirs.len() - config.max_backups as usize) {
            match remove_dir_all(&dirs[i].1) {
                Ok(_) => {}
                Err(e) => return Err(BackupError::RemoveFolderError(e)),
            }
        }
    }
    Ok(())
}

#[test]
pub fn test_folder_sorting() {
    let mut folders = vec![
        (
            Local.with_ymd_and_hms(2024, 07, 04, 00, 00, 00).unwrap(),
            PathBuf::from(r"C:\2"),
        ),
        (
            Local.with_ymd_and_hms(2024, 04, 03, 10, 11, 12).unwrap(),
            PathBuf::from(r"C:\0"),
        ),
        (
            Local.with_ymd_and_hms(2024, 04, 03, 10, 11, 13).unwrap(),
            PathBuf::from(r"C:\1"),
        ),
        (
            Local.with_ymd_and_hms(2024, 08, 03, 10, 11, 12).unwrap(),
            PathBuf::from(r"C:\4"),
        ),
        (
            Local.with_ymd_and_hms(2024, 08, 03, 00, 00, 00).unwrap(),
            PathBuf::from(r"C:\3"),
        ),
    ];
    folders.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(folders[0].1, PathBuf::from(r"C:\0"));
    assert_eq!(folders[1].1, PathBuf::from(r"C:\1"));
    assert_eq!(folders[2].1, PathBuf::from(r"C:\2"));
    assert_eq!(folders[3].1, PathBuf::from(r"C:\3"));
    assert_eq!(folders[4].1, PathBuf::from(r"C:\4"));
}

#[test]
pub fn test_folder_parsing() {
    let parts = "2024-05-06_07-08-09".split(['-', '_']);
    let collection = parts.collect::<Vec<_>>();
    if collection.len() < 6 {
        assert!(false);
    }
    let date = Local
        .with_ymd_and_hms(
            match collection[0].parse::<i32>() {
                Ok(v) => v,
                Err(_) => 1970,
            },
            match collection[1].parse::<u32>() {
                Ok(v) => v,
                Err(_) => 1,
            },
            match collection[2].parse::<u32>() {
                Ok(v) => v,
                Err(_) => 1,
            },
            match collection[3].parse::<u32>() {
                Ok(v) => v,
                Err(_) => 0,
            },
            match collection[4].parse::<u32>() {
                Ok(v) => v,
                Err(_) => 0,
            },
            match collection[5].parse::<u32>() {
                Ok(v) => v,
                Err(_) => 0,
            },
        )
        .unwrap();
    assert_eq!(
        Local.with_ymd_and_hms(2024, 05, 06, 07, 08, 09).unwrap(),
        date
    );

    let dt: DateTime<Local> = match "2024-05-06 07:08:09.0000000 -05:00".parse() {
        Ok(d) => d,
        Err(_) => {
            assert!(false);
            Local::now()
        }
    };
    assert_eq!(
        Local.with_ymd_and_hms(2024, 05, 06, 07, 08, 09).unwrap(),
        dt
    );
}

pub fn back_up_files(source: &PathBuf, config: &Configuration) -> BackupResult<PathBuf> {
    let now = Local::now();
    let new_dir = config
        .path
        .join(now.format("%Y-%m-%d %H-%M-%S").to_string());
    for i in &config.targets {
        copy_dir_all(source.join(&i.0), new_dir.join(&i.1))?;
    }
    remove_old_backups(config)?;
    Ok(new_dir)
}

#[test]
pub fn test_back_up_files() {
    let config = Configuration {
        frequency: Duration::from_secs(5),
        path: PathBuf::from(r"C:\TEMP\backups"),
        targets: TO_COPY
            .map(|pair| (pair.0.to_string(), pair.1.to_string()))
            .to_vec(),
        max_backups: 5,
    };
    create_dir_all(r"C:\TEMP\target\example\a").unwrap();
    for _ in 0..7 {
        match back_up_files(&PathBuf::from(r"C:\TEMP\target"), &config) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                println!("Error: {}", e);
                assert!(false);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    assert_eq!(
        read_dir(config.path)
            .unwrap()
            .into_iter()
            .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .count(),
        5,
    );
}

#[test]
pub fn test_pathbuf_join() -> std::io::Result<()> {
    let path = PathBuf::from(r"C:\TEMP\BCG");
    let new = path.join(r"A\B\C\D.txt");
    println!("{}", new.display());
    Ok(())
}

// endregion: Helper functions
pub struct App {
    pub current_screen: CurrentScreen,
    pub configuration: Configuration,
    pub next_backup: DateTime<Local>,
}

impl App {
    pub fn new() -> App {
        App {
            current_screen: CurrentScreen::Main,
            configuration: Configuration::default(),
            next_backup: DateTime::from_timestamp_nanos(0).into(),
        }
    }

    pub fn load_config(&mut self) -> CodeResult<()> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(match get_config_path() {
                Ok(p) => p,
                Err(val) => return Err(val),
            })?;

        self.configuration = read_config(file)?;

        Ok(())
    }

    pub fn set_view(&mut self, view: CurrentScreen) {
        self.current_screen = view;
    }
}
