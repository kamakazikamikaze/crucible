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
    back_up_files, duration_compare, retrieve_minecraft_path, App, CodeResult, GeneralError,
};

mod ui;
use ui::ui;

// region: Constants

const MIN_DEBOUNCE_MS: i64 = 750;

// endregion Constants

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
        let mut debounce: HashMap<KeyCode, DateTime<Local>> = HashMap::new();
        let start = Local::now();
        debounce.insert(KeyCode::Char('m'), start.clone());
        debounce.insert(KeyCode::Char('q'), start.clone());
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
            // Handle
            if match event::poll(std::time::Duration::from_millis(1000 / 60)) {
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
                        match key.code {
                            KeyCode::Char('q') => {
                                retval = Ok(());
                                break;
                            }
                            KeyCode::Char('m') => {
                                let debounced = is_debounced(key.code, now, &debounce);
                                debounce.insert(key.code, now.clone());
                                if !debounced {
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

    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;

    let result = run(&mut terminal, app);

    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;

    return result;
}
