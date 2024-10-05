use dirs::{config_local_dir, document_dir};
use std::{
    collections::HashMap,
    fs::{copy, create_dir_all, read_dir, remove_dir_all},
    io::{stdout, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime},
};

use chrono::{
    prelude::{DateTime, Local},
    DurationRound, TimeZone,
};
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, KeyCode, KeyEventKind},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    style::Stylize,
    widgets::Paragraph,
    Terminal,
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

const MIN_DEBOUNCE_MS: i64 = 750;

// endregion Constants

// region: Class definitions

#[derive(Serialize, Deserialize, Clone)]
struct Configuration {
    path: PathBuf,
    frequency: Duration,
    targets: Vec<(String, String)>,
    max_backups: u8,
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

// endregion Class definitions

// region: Error types

#[derive(Error, Debug)]
enum BackupError {
    #[error("cannot find installation of `{0}`")]
    NotInstalled(String),
    #[error("error opening file '`{0}`'")]
    FileError(#[from] std::io::Error),
    #[error("unable to create directory '`{0}`'")]
    TargetFolderError(String),
    #[error("std::io::error -> '`{0}`'")]
    CopyFileError(std::io::Error),
    #[error("Unable to remove directory '`{0}`'")]
    RemoveFolderError(std::io::Error),
}

type BackupResult<T> = std::result::Result<T, BackupError>;
type CodeResult<T> = std::result::Result<T, i32>;

// endregion Error types

fn retrieve_minecraft_path() -> BackupResult<PathBuf> {
    match Hive::CurrentUser.open(r"Software\Overwolf\CurseForge", Security::Read) {
        Ok(regkey) => match regkey.value("minecraft_root") {
            Ok(data) => Ok(PathBuf::from(data.to_string())),
            Err(_) => return Err(BackupError::NotInstalled(String::from("Minecraft"))),
        },
        Err(_) => return Err(BackupError::NotInstalled(String::from("CurseForge"))),
    }
}

#[test]
fn test_registry_access() {
    match retrieve_minecraft_path() {
        Ok(path) => println!("Success: {}", path.display()),
        Err(e) => println!("Failure: {}", e),
    }
}

fn get_config_path() -> CodeResult<PathBuf> {
    let mut config_path = match config_local_dir() {
        Some(path) => path,
        None => {
            println!("Cannot determine path to save runtime config to. (What are you runnnig?!)");
            return Err(-2);
        }
    }
    .join("BCG Backupper");

    match std::fs::create_dir_all(config_path.clone()) {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to create runtime config directory.");
            println!("Error: {}", e);
            return Err(-3);
        }
    }
    config_path.push("config.json");

    Ok(config_path)
}

#[test]
fn test_config_path() {
    let conf_path = get_config_path();
    assert!(conf_path.is_ok());
    assert!(conf_path
        .clone()
        .unwrap()
        .components()
        .any(|p| p == std::path::Component::Normal(std::ffi::OsStr::new("BCG Backupper"))));
    println!("{}", conf_path.unwrap().to_str().unwrap());
}

fn read_config(file: std::fs::File) -> CodeResult<Configuration> {
    match file.metadata() {
        Ok(meta) => {
            if meta.len() == 0 {
                let conf = Configuration::default();
                match to_writer_pretty(file, &conf) {
                    Ok(_) => Ok(conf),
                    Err(e) => {
                        println!("Error creating default configuration file.");
                        println!("{}", e);
                        return Err(-4);
                    }
                }
            } else {
                match from_reader(file) {
                    Ok(c) => Ok(c),
                    Err(e) => {
                        println!("Error reading configuration file.");
                        println!("{}", e);
                        return Err(-5);
                    }
                }
            }
        }
        Err(e) => {
            println!("Error retrieving metadata for config file.");
            println!("{}", e);
            return Err(-6);
        }
    }
}

fn write_config(mut file: std::fs::File, config: Configuration) -> CodeResult<()> {
    match file.seek(SeekFrom::Start(0)) {
        Ok(_) => {}
        Err(e) => {
            println!("Error while preparing to write to config file: {}", e);
            return Err(-7);
        }
    }

    match to_writer_pretty(&file, &config) {
        Ok(_) => {}
        Err(e) => {
            println!("Error writing to config file: {}", e);
            return Err(-8);
        }
    }

    let s_config = match to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            println!("Error serializing configuration.");
            println!("{}", e);
            return Err(-9);
        }
    };

    match file.set_len(s_config.len() as u64) {
        Ok(_) => {}
        Err(e) => {
            println!("Error truncating config file.");
            println!("{}", e);
            return Err(-10);
        }
    }

    Ok(())
}

#[test]
fn test_write_config() {
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
fn duration_compare(left: Duration, right: Duration, on_error: Option<f64>) -> Duration {
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
fn test_duration_compare() {
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

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
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

fn remove_old_backups(config: &Configuration) -> BackupResult<()> {
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
    if dirs.len() > config.max_backups as usize {
        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        for i in 0..(dirs.len() - config.max_backups as usize) {
            match remove_dir_all(&dirs[i].1) {
                Ok(_) => {
                    println!("Removed backup: {}", dirs[i].1.display())
                }
                Err(e) => return Err(BackupError::RemoveFolderError(e)),
            }
        }
    }
    Ok(())
}

#[test]
fn test_folder_sorting() {
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
fn test_folder_parsing() {
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

fn back_up_files(source: &PathBuf, config: &Configuration) -> BackupResult<PathBuf> {
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
fn test_back_up_files() {
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
fn test_pathbuf_join() -> std::io::Result<()> {
    let path = PathBuf::from(r"C:\TEMP\BCG");
    let new = path.join(r"A\B\C\D.txt");
    println!("{}", new.display());
    Ok(())
}

fn is_debounced(
    key: KeyCode,
    timestamp: DateTime<Local>,
    tracker: &HashMap<KeyCode, DateTime<Local>>,
) -> bool {
    match tracker.get(&key) {
        Some(last) => timestamp.signed_duration_since(last).num_milliseconds() >= MIN_DEBOUNCE_MS,
        None => false,
    }
}

fn main() -> CodeResult<()> {
    let install_path = match retrieve_minecraft_path() {
        Ok(path) => path,
        Err(e) => {
            println!("Unable to run. The following error was encountered: {}", e);
            println!("Press [ENTER] to exit.");
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            return Err(-1);
        }
    };

    println!(
        "Minecraft installed via CurseForge at: {}",
        install_path.display()
    );

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(match get_config_path() {
            Ok(p) => p,
            Err(val) => return Err(val),
        }) {
        Ok(f) => f,
        Err(e) => {
            println!("Unable to open config file!");
            println!("Error: {}", e);
            println!("Press [ENTER] to exit.");
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            return Err(-4);
        }
    };

    let config = match read_config(file) {
        Ok(c) => c,
        Err(val) => {
            println!("Press [ENTER] to exit.");
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            return Err(val);
        }
    };

    println!("{}", config);

    // region: Backup worker

    let safe_config = Arc::new(Mutex::new(config));
    let safe_config_clone = Arc::clone(&safe_config);
    let exit_flag = Arc::new(AtomicBool::new(false));
    let exit_flag_clone = Arc::clone(&exit_flag);
    let timer_change = Arc::new(AtomicBool::new(false));
    let timer_change_clone = Arc::clone(&timer_change);
    let manual_backup = Arc::new(AtomicBool::new(false));
    let manual_backup_clone = Arc::clone(&manual_backup);
    let worker_error_flag = Arc::new(AtomicBool::new(false));
    let worker_error_flag_clone = Arc::clone(&worker_error_flag);
    let worker = thread::spawn(move || {
        let mut use_diff_time = false;
        let mut diff_time = Duration::from_secs(0);
        let mc_path = install_path.clone();
        while !exit_flag_clone.load(Ordering::Relaxed) {
            let start = SystemTime::now();
            let sleep_for;
            if use_diff_time {
                sleep_for = diff_time;
                use_diff_time = false;
            } else {
                sleep_for = (*safe_config_clone.lock().unwrap()).frequency;
            }
            thread::park_timeout(sleep_for);
            let diff = SystemTime::now()
                .duration_since(start)
                .unwrap_or(Duration::from_secs(0));
            if diff >= sleep_for {
                println!("Timed backup triggered.");
                match back_up_files(&mc_path, &safe_config_clone.lock().unwrap()) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Error attempting to back up files.");
                        println!("{}", e);
                        worker_error_flag_clone.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            } else {
                if timer_change_clone.load(Ordering::Relaxed) {
                    use_diff_time = true;
                    diff_time = duration_compare(
                        diff,
                        (*safe_config_clone.lock().unwrap()).frequency,
                        None,
                    );
                } else if manual_backup_clone.load(Ordering::Relaxed) {
                    println!("Manual backup triggered.");
                    manual_backup_clone.swap(false, Ordering::Relaxed);
                    match back_up_files(&mc_path, &safe_config_clone.lock().unwrap()) {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error attempting to back up files.");
                            println!("{}", e);
                            worker_error_flag_clone.store(true, Ordering::Relaxed);
                            return;
                        }
                    }
                    use_diff_time = true;
                    diff_time = duration_compare(
                        (*safe_config_clone.lock().unwrap()).frequency,
                        diff,
                        None,
                    );
                }
            }
        }
    });

    // endregion Backup worker

    match stdout().execute(EnterAlternateScreen) {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to enter console 'alternate screen'.");
            println!("{}", e);
            return Err(-11);
        }
    }
    match enable_raw_mode() {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to enter console 'raw mode'.");
            println!("{}", e);
            return Err(-12);
        }
    }
    let mut terminal = match Terminal::new(CrosstermBackend::new(stdout())) {
        Ok(t) => t,
        Err(e) => {
            println!("Unable to acquire terminal backend.");
            println!("{}", e);
            return Err(-13);
        }
    };
    match terminal.clear() {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to clear temrinal screen");
            println!("{}", e);
            return Err(-14);
        }
    };

    let mut debounce: HashMap<KeyCode, DateTime<Local>> = HashMap::new();
    let start = Local::now();
    debounce.insert(KeyCode::Char('m'), start.clone());
    debounce.insert(KeyCode::Char('q'), start.clone());
    // Menu
    loop {
        // Draw
        match terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Paragraph::new("Press 'q' to quit").white().on_black(), area)
        }) {
            Ok(_) => {}
            Err(e) => {
                println!("Unable to draw to terminal.");
                println!("{}", e);
                break;
            }
        }
        // Handle
        if match event::poll(std::time::Duration::from_millis(1000 / 60)) {
            Ok(v) => v,
            Err(e) => {
                println!("Handling error: {}", e);
                break;
            }
        } {
            if let event::Event::Key(key) = match event::read() {
                Ok(v) => v,
                Err(e) => {
                    println!("Error reading event: {}", e);
                    break;
                }
            } {
                let now = Local::now();
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('m') => {
                            let debounced = is_debounced(key.code, now, &debounce);
                            debounce.insert(key.code, now.clone());
                            if !debounced {
                                println!("Debouncing!");
                                continue;
                            }
                            manual_backup.swap(true, Ordering::Relaxed);
                            worker.thread().unpark();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    match stdout().execute(LeaveAlternateScreen) {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to disable the 'alternate screen'.");
            println!("{}", e);
            return Err(-15);
        }
    };
    match disable_raw_mode() {
        Ok(_) => {}
        Err(e) => {
            println!("Unable to disable 'raw mode'.");
            println!("{}", e);
            return Err(-16);
        }
    };

    exit_flag.store(true, Ordering::Relaxed);
    worker.thread().unpark();
    match worker.join() {
        Ok(_) => {}
        Err(_) => println!("Error while waiting on backup worker."),
    }
    Ok(())
}
