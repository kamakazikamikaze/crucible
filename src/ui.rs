use std::path::PathBuf;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols::{border, line},
    text::{Line, Span},
    widgets::{block, Block, Borders, List, ListState, Paragraph},
    Frame,
};

use crate::app::{
    get_backups_sorted, Action, App, CurrentScreen, TIPS_BACKUPS, TIPS_CONFIRM, TIPS_MAIN,
    TIPS_NUM, TIPS_PATH, TIPS_SETTINGS, TIPS_TARGETS, TITLE,
};

pub const BACKUPS_MAX_CHARS: usize = 3;
pub const BACKUPS_FREQ_CHARS: usize = 6;

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    // Cut the given rectangle into three vertical pieces
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    // Then cut the middle vertical piece into three width-wise pieces
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1] // Return the middle chunk
}

pub fn ui(
    frame: &mut Frame,
    ui_state: &mut UIState,
    app: &App,
    action: Action,
    path: &PathBuf,
    children: &Vec<PathBuf>,
) {
    // General Layout Management
    let vert_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());
    let term_body = Block::bordered()
        .title(
            block::Title::from((TITLE).bold().style(Style::default().fg(Color::White)))
                .alignment(Alignment::Center),
        )
        .border_set(border::THICK)
        .border_style(Style::default().fg(Color::Green));
    frame.render_widget(term_body, vert_chunks[0]);

    // Main Area Management
    let horiz_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Max(20), Constraint::Min(10)])
        .split(vert_chunks[0].inner(Margin::new(1, 1)));
    let tooltips = Block::default()
        .borders(Borders::ALL)
        .title(
            block::Title::from(
                " Keys "
                    .not_bold()
                    .style(Style::default().fg(Color::Rgb(225, 225, 225))),
            )
            .alignment(Alignment::Center),
        )
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(Color::Rgb(135, 135, 135)));
    let tiptext = Paragraph::new(
        match app.current_screen {
            CurrentScreen::Main => TIPS_MAIN,
            CurrentScreen::Settings => TIPS_SETTINGS,
            CurrentScreen::Backups => TIPS_BACKUPS,
            CurrentScreen::Targets => TIPS_TARGETS,
            CurrentScreen::Path => TIPS_PATH,
            CurrentScreen::Target => TIPS_PATH,
            CurrentScreen::Frequency => TIPS_NUM,
            CurrentScreen::Max => TIPS_NUM,
        }
        .map(|(key, rest)| {
            if key.len() > 0 {
                Line::from(vec![
                    Span::styled(
                        "[",
                        Style::default().fg(Color::Rgb(185, 185, 185)).not_bold(),
                    ),
                    Span::styled(key, Style::default().fg(Color::Rgb(235, 235, 235)).bold()),
                    Span::styled(
                        "]",
                        Style::default().fg(Color::Rgb(185, 185, 185)).not_bold(),
                    ),
                    Span::styled(
                        rest,
                        Style::default().fg(Color::Rgb(185, 185, 185)).not_bold(),
                    ),
                ])
                .not_bold()
                .alignment(Alignment::Left)
            } else {
                Line::styled("", Style::default())
            }
        })
        .to_vec(),
    )
    .alignment(Alignment::Left)
    .block(tooltips);
    let mainblock = match app.current_screen {
        CurrentScreen::Backups => Block::default()
            .borders(Borders::ALL)
            .title(block::Title::from(" Backups ".not_bold()).alignment(Alignment::Left)),
        CurrentScreen::Targets => Block::default().borders(Borders::ALL).title(
            block::Title::from(" Target Files and Folders ".not_bold()).alignment(Alignment::Left),
        ),
        CurrentScreen::Settings | CurrentScreen::Frequency | CurrentScreen::Max => Block::default()
            .borders(Borders::ALL)
            .title(block::Title::from(" Settings ".not_bold()).alignment(Alignment::Left)),
        CurrentScreen::Target => Block::default()
            .borders(Borders::ALL)
            .title(block::Title::from(" Choose Path ".not_bold()).alignment(Alignment::Center)),
        _ => Block::default().borders(Borders::ALL),
    };

    frame.render_widget(tiptext, horiz_chunks[0]);

    match app.current_screen {
        CurrentScreen::Backups => {
            let backups = get_backups_sorted(&app.configuration).unwrap();
            let items = backups
                .iter()
                .map(|b| b.1.file_name().unwrap().to_str().unwrap());
            let contents = List::new(items)
                .block(mainblock)
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
                .highlight_symbol(" => ")
                .repeat_highlight_symbol(true);
            frame.render_stateful_widget(contents, horiz_chunks[1], &mut ui_state.backups);
        }
        CurrentScreen::Targets => {
            let items: Vec<Span<'_>> = app
                .configuration
                .targets
                .iter()
                .map(|b| Span::raw(b))
                .collect();
            let contents = List::new(items)
                .block(mainblock)
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
                .highlight_symbol(" => ")
                .repeat_highlight_symbol(true);
            frame.render_stateful_widget(contents, horiz_chunks[1], &mut ui_state.targets)
        }
        CurrentScreen::Settings | CurrentScreen::Max | CurrentScreen::Frequency => {
            let items: Vec<Span<'_>> = app
                .configuration
                .to_ui_list()
                .iter()
                .map(|b| Span::raw(format!(" {:>12} | {}", b.0, b.1)))
                .collect();
            let contents = List::new(items).block(mainblock);
            frame.render_widget(contents, horiz_chunks[1]);
            if app.current_screen == CurrentScreen::Max
                || app.current_screen == CurrentScreen::Frequency
            {
                let center = centered_rect(33, 33, frame.area());
                let numeric = Block::default()
                    .borders(Borders::ALL)
                    .title(
                        block::Title::from(
                            " Enter Value "
                                .bold()
                                .style(Style::default().fg(Color::White)),
                        )
                        .alignment(Alignment::Center),
                    )
                    .border_set(border::DOUBLE)
                    .border_style(Style::default().fg(Color::White).bg(Color::Blue))
                    .style(Style::default().bg(Color::Blue));
                let label;
                if app.current_screen == CurrentScreen::Max {
                    label =
                        Paragraph::new(format!("\n Max Backups: {}", ui_state.num_buf.join("")))
                            .alignment(Alignment::Left)
                            .style(Style::default().fg(Color::White))
                            .block(numeric);
                    frame.set_cursor_position(Position::new(
                        center.x + ui_state.cursor as u16 + 14,
                        center.y + 2,
                    ));
                } else {
                    let hours = ui_state.num_buf[0..2].join("");
                    let minutes = ui_state.num_buf[2..4].join("");
                    let seconds = ui_state.num_buf[4..6].join("");
                    label = Paragraph::new(format!(
                        "\n Backup Interval: {} hours, {} minutes, {} seconds",
                        hours, minutes, seconds
                    ))
                    .alignment(Alignment::Left)
                    .style(Style::default().fg(Color::White))
                    .block(numeric);
                    frame.set_cursor_position(Position::new(
                        match ui_state.cursor {
                            0..2 => center.x + ui_state.cursor as u16 + 19,
                            2..4 => center.x + (ui_state.cursor % 2) as u16 + 29,
                            4.. => center.x + (ui_state.cursor % 2) as u16 + 41,
                        },
                        center.y + 2,
                    ));
                }
                frame.render_widget(label, center);
            }
        }
        CurrentScreen::Target | CurrentScreen::Path => {
            let target_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(3)])
                .split(horiz_chunks[1]);
            let target_path = Block::bordered()
                .title(
                    block::Title::from(
                        " Current Directory "
                            .bold()
                            .style(Style::default().fg(Color::White)),
                    )
                    .alignment(Alignment::Center),
                )
                .border_set(border::THICK)
                .border_style(Style::default().fg(Color::Blue));
            let target = Paragraph::new(path.to_str().unwrap()).block(target_path);
            let target_nav = Block::bordered()
                .title(
                    block::Title::from(
                        " Navigation "
                            .bold()
                            .style(Style::default().fg(Color::White)),
                    )
                    .alignment(Alignment::Center),
                )
                .border_set(border::PLAIN)
                .border_style(Style::default().fg(Color::Blue));
            let items: Vec<Span<'_>> = children
                .iter()
                .map(|b| Span::raw(b.to_str().unwrap()))
                .collect();
            let contents = List::new(items)
                .block(target_nav)
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
                .highlight_symbol(" => ")
                .repeat_highlight_symbol(true);
            frame.render_widget(target, target_chunks[0]);
            frame.render_stateful_widget(
                contents,
                target_chunks[1],
                match app.current_screen {
                    CurrentScreen::Target => &mut ui_state.target_change,
                    CurrentScreen::Path => &mut ui_state.path,
                    _ => &mut ui_state.targets,
                },
            );
        }
        _ => frame.render_widget(mainblock, horiz_chunks[1]),
    };

    if action == Action::ConfirmDelete || action == Action::ConfirmRestore {
        let center = centered_rect(33, 33, vert_chunks[0]);
        let warning = Block::default()
            .borders(Borders::ALL)
            .title(
                block::Title::from(
                    " Are you sure? "
                        .bold()
                        .style(Style::default().fg(Color::White)),
                )
                .alignment(Alignment::Center)
                .position(block::Position::Top),
            )
            .title(
                block::Title::from(Line::from(
                    TIPS_CONFIRM
                        .map(|(key, rest)| {
                            vec![
                                " [".fg(Color::Rgb(185, 185, 185)).not_bold(),
                                key.fg(Color::Rgb(235, 235, 235)).bold(),
                                "]".fg(Color::Rgb(185, 185, 185)).not_bold(),
                                rest.fg(Color::Rgb(185, 185, 185)).not_bold(),
                                " ".fg(Color::Rgb(185, 185, 185)).not_bold(),
                            ]
                        })
                        .into_iter()
                        .flatten()
                        .collect::<Vec<Span<'_>>>(),
                ))
                .alignment(Alignment::Center)
                .position(block::Position::Bottom),
            )
            .border_set(border::DOUBLE)
            .border_style(Style::default().fg(Color::Gray).bg(Color::Red))
            .style(Style::default().bg(Color::Red));
        let warn_text = Paragraph::new(Line::from(
            match action {
                Action::ConfirmDelete => "\nFiles for this backup will be DELETED!",
                Action::ConfirmRestore => "\nFiles in game directory will be OVERWRITTEN!",
                _ => "",
            }
            .bold()
            .style(Style::default().fg(Color::White)),
        ))
        .centered()
        .block(warning);

        frame.render_widget(warn_text, center);
    } else if action == Action::ConfirmNonExistent {
        let center = centered_rect(33, 33, vert_chunks[0]);
        let warning = Block::default()
            .borders(Borders::ALL)
            .title(
                block::Title::from(
                    " !!! ERROR !!! "
                        .bold()
                        .style(Style::default().fg(Color::White)),
                )
                .alignment(Alignment::Center)
                .position(block::Position::Top),
            )
            .title(
                block::Title::from(Line::from(
                    TIPS_CONFIRM
                        .map(|(key, rest)| {
                            vec![
                                " [".fg(Color::Rgb(185, 185, 185)).not_bold(),
                                key.fg(Color::Rgb(235, 235, 235)).bold(),
                                "]".fg(Color::Rgb(185, 185, 185)).not_bold(),
                                rest.fg(Color::Rgb(185, 185, 185)).not_bold(),
                                " ".fg(Color::Rgb(185, 185, 185)).not_bold(),
                            ]
                        })
                        .into_iter()
                        .flatten()
                        .collect::<Vec<Span<'_>>>(),
                ))
                .alignment(Alignment::Center)
                .position(block::Position::Bottom),
            )
            .border_set(border::DOUBLE)
            .border_style(Style::default().fg(Color::Gray).bg(Color::Red))
            .style(Style::default().bg(Color::Red));
        let warn_text = Paragraph::new(Line::from(
            "Insufficient permisssions OR path does not exist!"
                .bold()
                .style(Style::default().fg(Color::White)),
        ))
        .centered()
        .block(warning);

        frame.render_widget(warn_text, center);
    }

    // Footer Area Management
    let last_backup_text = vec![
        Span::styled("Last backup: ", Style::default().fg(Color::White).bold()),
        {
            let backups = match get_backups_sorted(&app.configuration) {
                Ok(b) => b,
                Err(_) => Vec::new(),
            };
            match backups.len() {
                0 => Span::styled(
                    "Never",
                    Style::default().fg(Color::LightRed).bold().slow_blink(),
                ),
                _ => Span::styled(
                    backups
                        .last()
                        .unwrap()
                        .0
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                    Style::default().fg(Color::LightCyan),
                ),
            }
        },
    ];
    let next_backup_text = vec![
        Span::styled("Next backup: ", Style::default().fg(Color::White).bold()),
        Span::styled(
            app.next_backup.format("%H:%M:%S").to_string(),
            Style::default().fg(Color::LightCyan),
        ),
    ];

    let last_backup_block = Block::new().borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM);
    let last_backup_footer = Paragraph::new(Line::from(last_backup_text)).block(last_backup_block);

    let next_backup_border_set = border::Set {
        top_left: line::NORMAL.horizontal_down,
        bottom_left: line::NORMAL.horizontal_up,
        // vertical_left: line::NORMAL.vertical_left,
        ..border::PLAIN
    };
    let next_backup_block = Block::new()
        .borders(Borders::ALL)
        .border_set(next_backup_border_set);
    let next_backup_footer = Paragraph::new(Line::from(next_backup_text)).block(next_backup_block);

    let footer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vert_chunks[1]);

    frame.render_widget(last_backup_footer, footer_chunks[0]);
    frame.render_widget(next_backup_footer, footer_chunks[1]);
}

pub struct UIState {
    pub backups: ListState,
    pub targets: ListState,
    pub target_change: ListState,
    pub path: ListState,
    pub cursor: usize,
    pub num_buf: Vec<String>,
}

impl UIState {
    pub fn new() -> UIState {
        UIState {
            backups: ListState::default(),
            targets: ListState::default(),
            target_change: ListState::default(),
            path: ListState::default(),
            cursor: 0,
            num_buf: Vec::with_capacity(7),
        }
    }
}
