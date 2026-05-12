//! UI rendering for the planning TUI.
//!
//! Contains the [`draw`] function that lays out the planning interface:
//! title bar, scrollable chat history, status bar, and input area.

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::app::{App, AppState, MessageRole};

/// Role prefix for user messages.
const USER_PREFIX: &str = "You: ";

/// Role prefix for assistant messages.
const ASSISTANT_PREFIX: &str = "Assistant: ";

/// Role prefix for system messages.
const SYSTEM_PREFIX: &str = "System: ";

/// Draw the planning TUI onto the given frame.
///
/// Layout (top to bottom):
/// 1. **Title bar** — shows "GBA Plan: `<slug>`"
/// 2. **Chat area** — scrollable message history with role-colored prefixes
/// 3. **Status bar** — current application state indicator
/// 4. **Input area** — visible only in the `Input` state, shows current input buffer
pub fn draw(frame: &mut Frame, app: &App) {
    let input_height = match app.state() {
        AppState::Input => 3,
        _ => 0,
    };

    let constraints = if input_height > 0 {
        vec![
            Constraint::Length(3), // title
            Constraint::Min(5),    // chat area
            Constraint::Length(1), // status bar
            Constraint::Length(3), // input
        ]
    } else {
        vec![
            Constraint::Length(3), // title
            Constraint::Min(5),    // chat area
            Constraint::Length(1), // status bar
        ]
    };

    let chunks = Layout::vertical(constraints).split(frame.area());

    draw_title(frame, chunks[0], app);
    draw_chat(frame, chunks[1], app);
    draw_status(frame, chunks[2], app);

    if input_height > 0 {
        draw_input(frame, chunks[3], app);
    }
}

/// Render the title bar.
fn draw_title(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let title = format!(" GBA Plan: {} ", app.slug());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(block, area);
}

/// Render the scrollable chat history.
fn draw_chat(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Chat ")
        .style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = build_chat_lines(app);

    // Calculate visible height.
    let visible_height = inner.height as usize;
    let total_lines = lines.len();

    // Apply scroll: scroll_offset is from the bottom.
    let scroll_offset = app
        .scroll_offset()
        .min(total_lines.saturating_sub(visible_height));

    // We render a Paragraph with scroll. The scroll position is measured from the top.
    let scroll_from_top = total_lines
        .saturating_sub(visible_height)
        .saturating_sub(scroll_offset);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(scroll_from_top).unwrap_or(u16::MAX), 0));

    frame.render_widget(paragraph, inner);
}

/// Build all chat lines from the message history.
fn build_chat_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for msg in app.messages() {
        let (prefix, style) = match msg.role {
            MessageRole::User => (
                USER_PREFIX,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            MessageRole::Assistant => (ASSISTANT_PREFIX, Style::default().fg(Color::Green)),
            MessageRole::System => (
                SYSTEM_PREFIX,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::ITALIC),
            ),
        };

        // First line of the message gets the prefix.
        let content_lines: Vec<&str> = msg.content.lines().collect();

        if content_lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(prefix.to_owned(), style)]));
        } else {
            // First line: prefix + content.
            let first_line = format!("{prefix}{}", content_lines[0]);
            lines.push(Line::from(vec![Span::styled(first_line, style)]));

            // Continuation lines: indented to align with the content.
            let indent = " ".repeat(prefix.len());
            for content_line in content_lines.iter().skip(1) {
                let continued = format!("{indent}{content_line}");
                lines.push(Line::from(vec![Span::styled(continued, style)]));
            }
        }

        // Blank line between messages for readability.
        lines.push(Line::from(""));
    }

    lines
}

/// Render the status bar.
fn draw_status(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let (status_text, color) = match app.state() {
        AppState::Input => (
            "  [Input] Type your message, Enter to send, /done to finalize, Esc to quit",
            Color::Green,
        ),
        AppState::Waiting => ("  [Waiting] Processing... (Esc to cancel)", Color::Yellow),
        AppState::Done => ("  [Done] Press any key to exit", Color::Cyan),
    };

    let status = Paragraph::new(Line::from(vec![Span::styled(
        status_text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )]))
    .style(Style::default().bg(Color::DarkGray));

    frame.render_widget(status, area);
}

/// Render the input area.
fn draw_input(frame: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Input ")
        .style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let input_text =
        Paragraph::new(Line::from(app.input().to_owned())).style(Style::default().fg(Color::White));
    frame.render_widget(input_text, inner);

    // Position the cursor within the input area.
    // We need to compute the visual column from the byte offset.
    let visual_col = app.input().get(..app.cursor()).map_or(0, |s| {
        // Count grapheme columns — for ASCII this is just the char count.
        s.chars().count()
    });
    let col = inner
        .x
        .saturating_add(u16::try_from(visual_col).unwrap_or(u16::MAX));
    if col <= inner.right() {
        frame.set_cursor_position((col, inner.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;

    #[test]
    fn test_should_build_chat_lines_empty() {
        let app = App::new("test".to_owned());
        let lines = build_chat_lines(&app);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_should_build_chat_lines_with_messages() {
        let mut app = App::new("test".to_owned());
        app.push_message(MessageRole::User, "Hello".to_owned());
        app.push_message(MessageRole::Assistant, "Hi there".to_owned());

        let lines = build_chat_lines(&app);
        // 2 messages * (1 content line + 1 blank separator) = 4 lines
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_should_build_multiline_message_lines() {
        let mut app = App::new("test".to_owned());
        app.push_message(MessageRole::Assistant, "Line 1\nLine 2\nLine 3".to_owned());

        let lines = build_chat_lines(&app);
        // 3 content lines + 1 blank separator = 4 lines
        assert_eq!(lines.len(), 4);
    }
}
