use std::fmt::Display;

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

#[derive(Debug, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Settings,
    Interactive,
    Analyzer,
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Normal => write!(f, "Normal"),
            Mode::Settings => write!(f, "Settings"),
            Mode::Interactive => write!(f, "Interactive"),
            Mode::Analyzer => write!(f, "Analyzer"),
        }
    }
}

pub trait ApplicationMode {
    fn handle_key_event(&mut self, key_event: KeyEvent);
    fn set_active_inactive(&mut self, active: bool);
    fn render(&self, area: Rect, buf: &mut Frame);
}
