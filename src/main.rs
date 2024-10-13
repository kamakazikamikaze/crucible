use std::{
    collections::HashMap,
    fs::{read_dir, remove_dir_all},
    io::stdout,
    path::PathBuf,
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
    back_up_files, duration_compare, get_backups_sorted, restore_backup, retrieve_minecraft_path,
    Action, App, CodeResult, CurrentScreen, GeneralError,
};

mod ui;
use ui::{ui, UIState};

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

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut UIState,
    app: App,
) -> CodeResult<()> {
    thread::scope(|scope| {
        let install_path = retrieve_minecraft_path()?;
        let mc_path = install_path.clone();

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
        let mut action: Action = Action::None;
        let mut conf_changed = false;

        // Handling new target
        let mut new_target = install_path.clone();
        let mut child_items: Vec<PathBuf> = Vec::new();

        // Menu
        loop {
            // Draw
            match terminal.draw(|frame| {
                ui(
                    frame,
                    state,
                    &safe_app.lock().unwrap(),
                    action,
                    &new_target,
                    &child_items,
                )
            }) {
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
                        if action == Action::ConfirmDelete
                            || action == Action::ConfirmRestore
                            || action == Action::ConfirmNonExistent
                        {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Char('n') => {
                                    action = Action::None;
                                }
                                KeyCode::Char('y') => {
                                    action = match action {
                                        Action::ConfirmDelete => {
                                            match &unwrapped_app.current_screen {
                                                CurrentScreen::Backups => {
                                                    match state.backups.selected() {
                                                        Some(index) => {
                                                            remove_dir_all(
                                                                &get_backups_sorted(
                                                                    &unwrapped_app.configuration,
                                                                )
                                                                .unwrap()[index]
                                                                    .1,
                                                            )?;
                                                        }
                                                        None => {}
                                                    }
                                                    Action::None
                                                }
                                                CurrentScreen::Targets => {
                                                    match state.targets.selected() {
                                                        Some(index) => {
                                                            unwrapped_app
                                                                .configuration
                                                                .targets
                                                                .remove(index);
                                                            conf_changed = true;
                                                        }
                                                        None => {}
                                                    }
                                                    Action::None
                                                }
                                                _ => Action::None,
                                            }
                                        }
                                        Action::ConfirmRestore => match state.backups.selected() {
                                            Some(index) => {
                                                restore_backup(
                                                    &install_path,
                                                    &get_backups_sorted(
                                                        &unwrapped_app.configuration,
                                                    )
                                                    .unwrap()[index]
                                                        .1,
                                                    &unwrapped_app.configuration,
                                                )?;
                                                Action::None
                                            }
                                            None => Action::None,
                                        },
                                        Action::ConfirmNonExistent => Action::None,
                                        _ => action,
                                    }
                                }
                                _ => {}
                            }
                        } else {
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
                                CurrentScreen::Settings => {
                                    action = Action::None;
                                    match key.code {
                                        KeyCode::Char('q') => {
                                            unwrapped_app.set_view(CurrentScreen::Main);
                                        }
                                        KeyCode::Char('m') => {
                                            unwrapped_app.set_view(CurrentScreen::Max);
                                        }
                                        KeyCode::Char('t') => {
                                            unwrapped_app.set_view(CurrentScreen::Targets);
                                        }
                                        KeyCode::Char('f') => {
                                            unwrapped_app.set_view(CurrentScreen::Frequency);
                                        }
                                        KeyCode::Char('p') => {
                                            unwrapped_app.set_view(CurrentScreen::Path);
                                            new_target = unwrapped_app.configuration.path.clone();
                                            child_items = read_dir(new_target.clone())?
                                                .map(|i| i.unwrap().path())
                                                .collect();
                                            child_items.insert(0, new_target.join(".."));
                                        }
                                        _ => {}
                                    }
                                }
                                CurrentScreen::Backups => match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Main);
                                    }
                                    KeyCode::Char('r') => {
                                        action = Action::ConfirmRestore;
                                    }
                                    KeyCode::Char('d') => {
                                        action = Action::ConfirmDelete;
                                    }
                                    KeyCode::Down | KeyCode::Char('s') => {
                                        state.backups.select_next();
                                    }
                                    KeyCode::Up | KeyCode::Char('w') => {
                                        state.backups.select_previous();
                                    }
                                    KeyCode::Home => {
                                        state.backups.select_first();
                                    }
                                    KeyCode::End => {
                                        state.backups.select_last();
                                    }
                                    _ => {}
                                },
                                CurrentScreen::Path => match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Settings);
                                        state.path.select_first();
                                        new_target = install_path.clone();
                                        child_items = read_dir(new_target.clone())?
                                            .map(|i| i.unwrap().path())
                                            .collect();
                                        child_items.insert(0, new_target.join(".."));
                                    }
                                    KeyCode::Down | KeyCode::Char('s') => {
                                        state.path.select_next();
                                    }
                                    KeyCode::Up | KeyCode::Char('w') => {
                                        state.path.select_previous();
                                    }
                                    KeyCode::Char('t') => {
                                        match state.path.selected().unwrap() {
                                            0 => {
                                                unwrapped_app.configuration.path =
                                                    new_target.clone()
                                            }
                                            _ => {
                                                unwrapped_app.configuration.path = child_items
                                                    .remove(state.path.selected().unwrap())
                                            }
                                        };
                                        unwrapped_app.set_view(CurrentScreen::Settings);
                                        state.path.select_first();
                                        new_target = install_path.clone();
                                        conf_changed = true;
                                    }
                                    KeyCode::Enter => {
                                        new_target = match state.path.selected().unwrap() {
                                            0 => match new_target.parent() {
                                                Some(parent) => parent.to_path_buf(),
                                                None => new_target,
                                            },
                                            _ => child_items.remove(state.path.selected().unwrap()),
                                        };
                                        if new_target.is_file() {
                                            new_target = new_target.parent().unwrap().to_path_buf();
                                        }
                                        child_items = read_dir(new_target.clone())?
                                            .map(|i| i.unwrap().path())
                                            .collect();
                                        child_items.insert(0, new_target.join(".."));
                                        state.path.select_first();
                                    }
                                    KeyCode::Home => {
                                        state.path.select_first();
                                    }
                                    KeyCode::End => {
                                        state.path.select_last();
                                    }
                                    _ => {}
                                },
                                CurrentScreen::Target => match key.code {
                                    KeyCode::Char('q') => {
                                        unwrapped_app.set_view(CurrentScreen::Targets);
                                        state.target_change.select_first();
                                        new_target = install_path.clone();
                                    }
                                    KeyCode::Down | KeyCode::Char('s') => {
                                        state.target_change.select_next();
                                    }
                                    KeyCode::Up | KeyCode::Char('w') => {
                                        state.target_change.select_previous();
                                    }
                                    KeyCode::Char('t') => {
                                        if action == Action::Add {
                                            match state.target_change.selected().unwrap() {
                                                0 => unwrapped_app.configuration.targets.push(
                                                    String::from(
                                                        new_target
                                                            .strip_prefix(&install_path)
                                                            .unwrap()
                                                            .to_str()
                                                            .unwrap(),
                                                    ),
                                                ),
                                                _ => unwrapped_app.configuration.targets.push(
                                                    String::from(
                                                        child_items
                                                            .remove(
                                                                state
                                                                    .target_change
                                                                    .selected()
                                                                    .unwrap(),
                                                            )
                                                            .strip_prefix(&install_path)
                                                            .unwrap()
                                                            .to_str()
                                                            .unwrap(),
                                                    ),
                                                ),
                                            }
                                        } else if action == Action::Edit {
                                            unwrapped_app
                                                .configuration
                                                .targets
                                                .remove(state.targets.selected().unwrap());
                                            unwrapped_app.configuration.targets.insert(
                                                state.targets.selected().unwrap(),
                                                match state.target_change.selected().unwrap() {
                                                    0 => String::from(
                                                        new_target
                                                            .strip_prefix(&install_path)
                                                            .unwrap()
                                                            .to_str()
                                                            .unwrap(),
                                                    ),
                                                    _ => String::from(
                                                        child_items
                                                            .remove(
                                                                state
                                                                    .target_change
                                                                    .selected()
                                                                    .unwrap(),
                                                            )
                                                            .strip_prefix(&install_path)
                                                            .unwrap()
                                                            .to_str()
                                                            .unwrap(),
                                                    ),
                                                },
                                            );
                                        }
                                        unwrapped_app.set_view(CurrentScreen::Targets);
                                        state.target_change.select_first();
                                        new_target = install_path.clone();
                                        conf_changed = true;
                                    }
                                    KeyCode::Enter => {
                                        new_target = match state.target_change.selected().unwrap() {
                                            0 => {
                                                if new_target == install_path {
                                                    new_target
                                                } else {
                                                    match new_target.parent() {
                                                        Some(parent) => parent.to_path_buf(),
                                                        None => new_target,
                                                    }
                                                }
                                            }
                                            _ => child_items
                                                .remove(state.target_change.selected().unwrap()),
                                        };
                                        if new_target.is_file() {
                                            new_target = new_target.parent().unwrap().to_path_buf();
                                        }
                                        child_items = read_dir(new_target.clone())?
                                            .map(|i| i.unwrap().path())
                                            .collect();
                                        child_items.insert(0, new_target.join(".."));
                                        state.target_change.select_first();
                                    }
                                    KeyCode::Home => {
                                        state.target_change.select_first();
                                    }
                                    KeyCode::End => {
                                        state.target_change.select_last();
                                    }
                                    _ => {}
                                },
                                CurrentScreen::Targets => match key.code {
                                    KeyCode::Char('q') => {
                                        action = Action::None;
                                        unwrapped_app.set_view(CurrentScreen::Settings);
                                    }
                                    KeyCode::Char('a') => {
                                        action = Action::Add;
                                        unwrapped_app.set_view(CurrentScreen::Target);
                                        child_items = read_dir(new_target.clone())?
                                            .map(|i| i.unwrap().path())
                                            .collect();
                                        child_items.insert(0, new_target.join(".."));
                                    }
                                    KeyCode::Char('e') => {
                                        action = Action::Edit;
                                        unwrapped_app.set_view(CurrentScreen::Target);
                                        new_target = install_path.clone().join(
                                            unwrapped_app.configuration.targets
                                                [state.targets.selected().unwrap()]
                                            .clone(),
                                        );
                                        if new_target.is_file() {
                                            new_target = new_target.parent().unwrap().to_path_buf();
                                        }
                                        child_items = read_dir(new_target.clone())?
                                            .map(|i| i.unwrap().path())
                                            .collect();
                                        child_items.insert(0, new_target.join(".."));
                                    }
                                    KeyCode::Char('d') => {
                                        action = Action::ConfirmDelete;
                                    }
                                    KeyCode::Down | KeyCode::Char('s') => {
                                        state.targets.select_next();
                                    }
                                    KeyCode::Up | KeyCode::Char('w') => {
                                        state.targets.select_previous();
                                    }
                                    KeyCode::Home => {
                                        state.targets.select_first();
                                    }
                                    KeyCode::End => {
                                        state.targets.select_last();
                                    }
                                    _ => {}
                                },
                                CurrentScreen::Frequency => todo!(),
                                CurrentScreen::Max => todo!(),
                            }
                        }
                        if conf_changed {
                            conf_changed = false;
                            unwrapped_app.save_config()?;
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

    let mut state = UIState::new();
    state.backups.select_first();
    state.targets.select_first();
    state.target_change.select_first();
    state.path.select_first();

    let result = run(&mut terminal, &mut state, app);

    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;

    match &result {
        Ok(_) => {}
        Err(e) => println!("{:?}", e),
    }

    return result;
}
