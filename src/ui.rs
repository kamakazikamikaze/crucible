use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    symbols::{border, line},
    text::{Line, Span},
    widgets::{
        block::{Position, Title},
        Block, Borders, Paragraph,
    },
    Frame,
};

use crate::app::{
    get_backups_sorted, App, CurrentScreen, TIPS_BACKUPS, TIPS_CONFIRM, TIPS_MAIN, TIPS_SETTINGS,
    TIPS_TARGETS, TITLE,
};

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

/*
pub fn ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // Top block
    let title_block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default());
    let title = Paragraph::new(Text::styled(APP_NAME, Style::default().fg(Color::Green)))
        .block(title_block);

    frame.render_widget(title, chunks[0]);
}
*/

pub fn ui(frame: &mut Frame, app: &App) {
    // General Layout Management
    let vert_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());
    let term_body = Block::bordered()
        .title(
            Title::from((TITLE).bold().style(Style::default().fg(Color::White)))
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
            Title::from(
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
            CurrentScreen::ConfirmRestore => TIPS_BACKUPS,
            CurrentScreen::ConfirmRemove => TIPS_BACKUPS,
        }
        .map(|(key, rest)| {
            if key.len() == 1 {
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
    let content = Block::default().borders(Borders::NONE);

    frame.render_widget(tiptext, horiz_chunks[0]);
    frame.render_widget(content, horiz_chunks[1]);

    if app.current_screen == CurrentScreen::ConfirmRemove
        || app.current_screen == CurrentScreen::ConfirmRestore
    {
        let center = centered_rect(33, 33, vert_chunks[0]);
        let warning = Block::default()
            .borders(Borders::ALL)
            .title(
                Title::from(
                    " Are you sure? "
                        .bold()
                        .style(Style::default().fg(Color::White)),
                )
                .alignment(Alignment::Center)
                .position(Position::Top),
            )
            .title(
                Title::from(Line::from(
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
                .position(Position::Bottom),
            )
            .border_set(border::DOUBLE)
            .border_style(Style::default().fg(Color::Gray).bg(Color::Red))
            .style(Style::default().bg(Color::Red));
        let warn_text = Paragraph::new(Line::from(
            match app.current_screen {
                CurrentScreen::ConfirmRemove => "Files for this backup will be DELETED!",
                CurrentScreen::ConfirmRestore => "Files in game directory will be OVERWRITTEN!",
                _ => "",
            }
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
