use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{block::Title, Block, Borders, Paragraph},
    Frame,
};

use crate::app::{get_backups_sorted, App, TITLE};

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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());
    let body = Block::bordered()
        .title(
            Title::from((TITLE).bold().style(Style::default().fg(Color::White)))
                .alignment(Alignment::Center),
        )
        .border_set(border::DOUBLE)
        .border_style(Style::default().fg(Color::Green));
    frame.render_widget(body, chunks[0]);

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

    let last_backup_footer =
        Paragraph::new(Line::from(last_backup_text)).block(Block::default().borders(Borders::ALL));

    let next_backup_footer =
        Paragraph::new(Line::from(next_backup_text)).block(Block::default().borders(Borders::ALL));

    let footer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    frame.render_widget(last_backup_footer, footer_chunks[0]);
    frame.render_widget(next_backup_footer, footer_chunks[1]);
}
