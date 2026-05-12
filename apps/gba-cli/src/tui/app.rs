//! Application state for the planning TUI.
//!
//! Contains the [`App`] struct that holds the chat history, current input
//! buffer, scroll position, and UI state machine.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Maximum number of characters allowed in the input buffer.
const MAX_INPUT_LENGTH: usize = 4096;

/// Maximum number of messages retained in the chat history.
const MAX_MESSAGES: usize = 1000;

/// The current state of the TUI application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    /// Waiting for user input.
    Input,
    /// Waiting for the engine to respond.
    Waiting,
    /// Session is finished — user can press any key to exit.
    Done,
}

/// The role of a message in the chat history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    /// Message from the user.
    User,
    /// Response from the assistant (LLM).
    Assistant,
    /// System notification (e.g. "Finalizing plan...").
    System,
}

/// A single message in the chat history.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Who authored the message.
    pub role: MessageRole,
    /// The message text.
    pub content: String,
}

/// TUI application state for the planning session.
#[derive(Debug)]
pub struct App {
    /// Feature slug displayed in the title bar.
    slug: String,
    /// Chat history (role + content).
    messages: Vec<ChatMessage>,
    /// Current user input buffer.
    input: String,
    /// Cursor position within the input buffer (byte offset).
    cursor: usize,
    /// Current UI state.
    state: AppState,
    /// Vertical scroll offset for the chat history (lines from bottom).
    scroll_offset: usize,
}

impl App {
    /// Create a new application with the given feature slug.
    pub fn new(slug: String) -> Self {
        Self {
            slug,
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            state: AppState::Input,
            scroll_offset: 0,
        }
    }

    /// Return the feature slug.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Return the chat message history.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Return the current input buffer contents.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Return the cursor position within the input buffer.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Return the current application state.
    pub fn state(&self) -> AppState {
        self.state
    }

    /// Return the current scroll offset (lines from bottom).
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Set the application state.
    pub fn set_state(&mut self, state: AppState) {
        self.state = state;
    }

    /// Append a message to the chat history.
    ///
    /// When the history exceeds [`MAX_MESSAGES`], the oldest message
    /// is removed.
    pub fn push_message(&mut self, role: MessageRole, content: String) {
        self.messages.push(ChatMessage { role, content });
        if self.messages.len() > MAX_MESSAGES {
            self.messages.remove(0);
        }
        // Reset scroll to bottom when a new message arrives.
        self.scroll_offset = 0;
    }

    /// Take the current input, clearing the buffer.
    ///
    /// Returns the input contents and resets the cursor.
    pub fn take_input(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.input)
    }

    /// Handle a key event based on the current state.
    ///
    /// Returns `true` if the application should exit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        match self.state {
            AppState::Input => self.handle_input_key(key),
            AppState::Waiting => self.handle_waiting_key(key),
            AppState::Done => true, // any key exits
        }
    }

    /// Handle key events while in the `Input` state.
    ///
    /// Returns `true` if the application should exit (Esc pressed).
    fn handle_input_key(&mut self, key: KeyEvent) -> bool {
        // Ctrl+C always exits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.state = AppState::Done;
            return true;
        }

        match key.code {
            KeyCode::Esc => {
                self.state = AppState::Done;
                true
            }
            KeyCode::Char(c) => {
                self.on_char(c);
                false
            }
            KeyCode::Backspace => {
                self.on_backspace();
                false
            }
            KeyCode::Delete => {
                self.on_delete();
                false
            }
            KeyCode::Left => {
                self.move_cursor_left();
                false
            }
            KeyCode::Right => {
                self.move_cursor_right();
                false
            }
            KeyCode::Home => {
                self.cursor = 0;
                false
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                false
            }
            KeyCode::Up => {
                self.scroll_up(1);
                false
            }
            KeyCode::Down => {
                self.scroll_down(1);
                false
            }
            KeyCode::PageUp => {
                self.scroll_up(10);
                false
            }
            KeyCode::PageDown => {
                self.scroll_down(10);
                false
            }
            _ => false,
        }
    }

    /// Handle key events while in the `Waiting` state.
    ///
    /// Only Esc and Ctrl+C are accepted; everything else is ignored.
    fn handle_waiting_key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Esc
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            self.state = AppState::Done;
            return true;
        }
        // Allow scrolling while waiting.
        match key.code {
            KeyCode::Up => self.scroll_up(1),
            KeyCode::Down => self.scroll_down(1),
            KeyCode::PageUp => self.scroll_up(10),
            KeyCode::PageDown => self.scroll_down(10),
            _ => {}
        }
        false
    }

    /// Insert a character at the cursor position.
    pub fn on_char(&mut self, c: char) {
        if self.input.len() >= MAX_INPUT_LENGTH {
            return;
        }
        self.input.insert(self.cursor, c);
        self.cursor = self.cursor.saturating_add(c.len_utf8());
    }

    /// Delete the character before the cursor.
    pub fn on_backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Find the byte index of the previous character boundary.
        let prev = self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.input.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Delete the character at the cursor position.
    fn on_delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        // Find the next character boundary after cursor.
        let next = self.input[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor.saturating_add(idx))
            .unwrap_or(self.input.len());
        self.input.replace_range(self.cursor..next, "");
    }

    /// Move the cursor one character to the left.
    fn move_cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.cursor = prev;
    }

    /// Move the cursor one character to the right.
    fn move_cursor_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let next = self.input[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor.saturating_add(idx))
            .unwrap_or(self.input.len());
        self.cursor = next;
    }

    /// Scroll the chat history upward by the given number of lines.
    fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    /// Scroll the chat history downward (toward latest) by the given
    /// number of lines.
    fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_create_app_with_defaults() {
        let app = App::new("test-feature".to_owned());
        assert_eq!(app.slug(), "test-feature");
        assert!(app.messages().is_empty());
        assert!(app.input().is_empty());
        assert_eq!(app.cursor(), 0);
        assert_eq!(app.state(), AppState::Input);
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn test_should_push_and_retrieve_messages() {
        let mut app = App::new("test".to_owned());
        app.push_message(MessageRole::System, "Hello".to_owned());
        app.push_message(MessageRole::User, "World".to_owned());
        assert_eq!(app.messages().len(), 2);
        assert_eq!(app.messages()[0].role, MessageRole::System);
        assert_eq!(app.messages()[0].content, "Hello");
        assert_eq!(app.messages()[1].role, MessageRole::User);
    }

    #[test]
    fn test_should_evict_oldest_message_when_over_limit() {
        let mut app = App::new("test".to_owned());
        for i in 0..=MAX_MESSAGES {
            app.push_message(MessageRole::System, format!("msg-{i}"));
        }
        assert_eq!(app.messages().len(), MAX_MESSAGES);
        // First message should be "msg-1" (msg-0 was evicted)
        assert_eq!(app.messages()[0].content, "msg-1");
    }

    #[test]
    fn test_should_insert_and_take_input() {
        let mut app = App::new("test".to_owned());
        app.on_char('H');
        app.on_char('i');
        assert_eq!(app.input(), "Hi");
        assert_eq!(app.cursor(), 2);

        let taken = app.take_input();
        assert_eq!(taken, "Hi");
        assert!(app.input().is_empty());
        assert_eq!(app.cursor(), 0);
    }

    #[test]
    fn test_should_handle_backspace() {
        let mut app = App::new("test".to_owned());
        app.on_char('A');
        app.on_char('B');
        app.on_char('C');
        app.on_backspace();
        assert_eq!(app.input(), "AB");
        assert_eq!(app.cursor(), 2);
    }

    #[test]
    fn test_should_handle_backspace_at_start() {
        let mut app = App::new("test".to_owned());
        app.on_backspace(); // should not panic
        assert!(app.input().is_empty());
    }

    #[test]
    fn test_should_enforce_max_input_length() {
        let mut app = App::new("test".to_owned());
        for _ in 0..MAX_INPUT_LENGTH {
            app.on_char('x');
        }
        assert_eq!(app.input().len(), MAX_INPUT_LENGTH);
        app.on_char('y'); // should be ignored
        assert_eq!(app.input().len(), MAX_INPUT_LENGTH);
    }

    #[test]
    fn test_should_set_and_get_state() {
        let mut app = App::new("test".to_owned());
        assert_eq!(app.state(), AppState::Input);
        app.set_state(AppState::Waiting);
        assert_eq!(app.state(), AppState::Waiting);
        app.set_state(AppState::Done);
        assert_eq!(app.state(), AppState::Done);
    }

    #[test]
    fn test_should_scroll_up_and_down() {
        let mut app = App::new("test".to_owned());
        assert_eq!(app.scroll_offset(), 0);
        app.scroll_up(5);
        assert_eq!(app.scroll_offset(), 5);
        app.scroll_down(3);
        assert_eq!(app.scroll_offset(), 2);
        app.scroll_down(10); // should clamp to 0
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn test_should_reset_scroll_on_new_message() {
        let mut app = App::new("test".to_owned());
        app.scroll_up(5);
        assert_eq!(app.scroll_offset(), 5);
        app.push_message(MessageRole::System, "new".to_owned());
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn test_should_handle_esc_in_input_state() {
        let mut app = App::new("test".to_owned());
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let should_exit = app.on_key(key);
        assert!(should_exit);
        assert_eq!(app.state(), AppState::Done);
    }

    #[test]
    fn test_should_exit_on_any_key_in_done_state() {
        let mut app = App::new("test".to_owned());
        app.set_state(AppState::Done);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let should_exit = app.on_key(key);
        assert!(should_exit);
    }

    #[test]
    fn test_should_handle_ctrl_c() {
        let mut app = App::new("test".to_owned());
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let should_exit = app.on_key(key);
        assert!(should_exit);
        assert_eq!(app.state(), AppState::Done);
    }

    #[test]
    fn test_should_move_cursor_left_and_right() {
        let mut app = App::new("test".to_owned());
        app.on_char('A');
        app.on_char('B');
        app.on_char('C');
        assert_eq!(app.cursor(), 3);

        app.move_cursor_left();
        assert_eq!(app.cursor(), 2);

        app.move_cursor_left();
        assert_eq!(app.cursor(), 1);

        app.move_cursor_right();
        assert_eq!(app.cursor(), 2);
    }

    #[test]
    fn test_should_handle_cursor_at_boundaries() {
        let mut app = App::new("test".to_owned());
        app.move_cursor_left(); // at 0, should not panic
        assert_eq!(app.cursor(), 0);

        app.on_char('A');
        app.move_cursor_right(); // at end, should not move
        assert_eq!(app.cursor(), 1);
    }

    #[test]
    fn test_should_handle_delete() {
        let mut app = App::new("test".to_owned());
        app.on_char('A');
        app.on_char('B');
        app.on_char('C');
        app.move_cursor_left();
        app.move_cursor_left();
        // Cursor is at 'B'
        app.on_delete();
        assert_eq!(app.input(), "AC");
        assert_eq!(app.cursor(), 1);
    }

    #[test]
    fn test_should_handle_home_and_end() {
        let mut app = App::new("test".to_owned());
        app.on_char('A');
        app.on_char('B');
        app.on_char('C');

        let home_key = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
        app.on_key(home_key);
        assert_eq!(app.cursor(), 0);

        let end_key = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        app.on_key(end_key);
        assert_eq!(app.cursor(), 3);
    }
}
