use std::{
    collections::HashMap,
    io::stdout,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime},
};

use chrono::prelude::{DateTime, Local};
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

mod app;
use app::{
    back_up_files, duration_compare, retrieve_minecraft_path, App, CodeResult, CurrentScreen,
    EditSetting, GeneralError,
};

mod ui;
use ui::ui;

// region: Constants

// endregion Constants

fn is_debounced(
    key: KeyCode,
    timestamp: DateTime<Local>,
    tracker: &HashMap<KeyCode, DateTime<Local>>,
    duration: Duration,
) -> bool {
    match tracker.get(&key) {
        Some(last) => {
            timestamp.signed_duration_since(last).num_milliseconds() as u128 >= duration.as_millis()
        }
        None => true,
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: App) -> CodeResult<()> {
    thread::scope(|scope| {
        let install_path = retrieve_minecraft_path()?;

        // region: Backup worker

        let safe_app = Arc::new(Mutex::new(app));
        let safe_app_copy = Arc::clone(&safe_app);
        let exit_flag = Arc::new(AtomicBool::new(false));
        let exit_flag_clone = Arc::clone(&exit_flag);
        let timer_change = Arc::new(AtomicBool::new(false));
        let timer_change_clone = Arc::clone(&timer_change);
        let manual_backup = Arc::new(AtomicBool::new(false));
        let manual_backup_clone = Arc::clone(&manual_backup);
        let worker_error_flag = Arc::new(AtomicBool::new(false));
        let worker_error_flag_clone = Arc::clone(&worker_error_flag);

        let worker = scope.spawn(move || {
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
                    sleep_for = (*safe_app_copy.lock().unwrap()).configuration.frequency;
                }

                (*safe_app_copy.lock().unwrap()).next_backup =
                    SystemTime::now().checked_add(sleep_for).unwrap().into();

                thread::park_timeout(sleep_for);
                let diff = SystemTime::now()
                    .duration_since(start)
                    .unwrap_or(Duration::from_secs(0));
                if diff >= sleep_for {
                    match back_up_files(&mc_path, &safe_app_copy.lock().unwrap().configuration) {
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
                            (*safe_app_copy.lock().unwrap()).configuration.frequency,
                            None,
                        );
                    } else if manual_backup_clone.load(Ordering::Relaxed) {
                        manual_backup_clone.swap(false, Ordering::Relaxed);
                        match back_up_files(&mc_path, &safe_app_copy.lock().unwrap().configuration)
                        {
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
                            (*safe_app_copy.lock().unwrap()).configuration.frequency,
                            diff,
                            None,
                        );
                    }
                }
            }
        });

        // endregion Backup worker

        // region: Update logic

        let retval;
        let mut main_debounce: HashMap<KeyCode, DateTime<Local>> = HashMap::new();
        let mut backups_debounce: HashMap<KeyCode, DateTime<Local>> = HashMap::new();
        let mut setting = EditSetting::None;
        let mut selected: u8 = 0;

        // Menu
        loop {
            // Draw
            match terminal.draw(|frame| ui(frame, &safe_app.lock().unwrap())) {
                Ok(_) => {}
                Err(e) => {
                    retval = Err(GeneralError::Error(e.to_string()));
                    break;
                }
            }
            let start = Local::now();
            // Handle
            if match event::poll(std::time::Duration::from_millis(
                (&safe_app.lock().unwrap().next_backup.timestamp_millis()
                    - start.timestamp_millis()
                    - 1) as u64,
            )) {
                Ok(v) => v,
                Err(e) => {
                    retval = Err(GeneralError::Error(e.to_string()));
                    break;
                }
            } {
                if let event::Event::Key(key) = match event::read() {
                    Ok(v) => v,
                    Err(e) => {
                        retval = Err(GeneralError::Error(e.to_string()));
                        break;
                    }
                } {
                    let now = Local::now();
                    if key.kind == KeyEventKind::Press {
                        let mut unwrapped_app = safe_app.lock().unwrap();
                        match &unwrapped_app.current_screen {
                            CurrentScreen::Main => {
                                let debounced = is_debounced(
                                    key.code,
                                    now,
                                    &main_debounce,
                                    Duration::from_secs(2),
                                );
                                main_debounce.insert(key.code, now.clone());
                                if !debounced {
                                    continue;
                                }
                                match key.code {
                                    KeyCode::Char('q') => {
                                        retval = Ok(());
                                        break;
                                    }
                                    KeyCode::Char('m') => {
                                        manual_backup.swap(true, Ordering::Relaxed);
                                        worker.thread().unpark();
                                    }
                                    KeyCode::Char('s') => {
                                        unwrapped_app.set_view(CurrentScreen::Settings);
                                    }
                                    KeyCode::Char('b') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                    }
                                    _ => {}
                                }
                            }
                            CurrentScreen::Settings => match setting {
                                EditSetting::Path => todo!(),
                                EditSetting::Targets => todo!(),
                                EditSetting::Frequency => todo!(),
                                EditSetting::Max => todo!(),
                                EditSetting::None => match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Main);
                                    }
                                    KeyCode::Char('m') => {
                                        setting = EditSetting::Max;
                                    }
                                    KeyCode::Char('t') => {
                                        setting = EditSetting::Targets;
                                    }
                                    KeyCode::Char('f') => {
                                        setting = EditSetting::Frequency;
                                    }
                                    KeyCode::Char('p') => {
                                        setting = EditSetting::Path;
                                    }
                                    _ => {}
                                },
                            },
                            CurrentScreen::Backups => {
                                let debounced = is_debounced(
                                    key.code,
                                    now,
                                    &backups_debounce,
                                    Duration::from_secs(2),
                                );
                                backups_debounce.insert(key.code, now.clone());
                                if !debounced {
                                    continue;
                                }
                                match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Main);
                                    }
                                    KeyCode::Char('r') => {
                                        unwrapped_app.set_view(CurrentScreen::ConfirmRestore);
                                    }
                                    KeyCode::Char('d') => {
                                        unwrapped_app.set_view(CurrentScreen::ConfirmRemove);
                                    }
                                    _ => {}
                                }
                            }
                            CurrentScreen::Targets => {}
                            CurrentScreen::ConfirmRemove => {
                                match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                    }
                                    KeyCode::Char('y') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                        // TODO: Execute removal
                                    }
                                    KeyCode::Char('n') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                    }
                                    _ => {}
                                }
                            }
                            CurrentScreen::ConfirmRestore => {
                                match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                    }
                                    KeyCode::Char('y') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                        // TODO: Execute restore
                                    }
                                    KeyCode::Char('n') => {
                                        unwrapped_app.set_view(CurrentScreen::Backups);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // endregion: Update logic

        exit_flag.store(true, Ordering::Relaxed);
        worker.thread().unpark();
        match worker.join() {
            Ok(()) => {}
            Err(e) => {
                if retval.is_err() {
                    return Err(GeneralError::LoopAndBackupWorker(
                        e,
                        retval.unwrap_err().to_string(),
                    ));
                }
            }
        }

        retval
    })
}

fn main() -> CodeResult<()> {
    let mut app = App::new();
    app.load_config()?;

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;

    let result = run(&mut terminal, app);

    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;

    return result;
}
