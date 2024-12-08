use std::sync::mpsc::Sender;

use crossterm::event::KeyCode;
use ratatui::{style::{Style, Stylize}, symbols::border, text::Line, widgets::{Block, Paragraph}};

use crate::{mode::ApplicationMode, portthread::SerialCommand, CRLFSetting, InputMode, INPUT_MODES};


#[derive(Debug)]
pub struct InteractiveMode
{
    active: bool,
    send_buffer: Vec<u8>,
    input_mode: InputMode,
    command_sender: Sender<SerialCommand>,
    crlf_setting: CRLFSetting
}

impl ApplicationMode for InteractiveMode
{
    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) {        
        match key_event.code {
            KeyCode::Char(x) => {
                if self.input_mode == InputMode::Hex {
                    if x.is_ascii_hexdigit() || x == ' ' {
                        self.send_buffer.push(x as u8);
                    }
                } else {
                    self.send_buffer.push(x as u8)
                }
            }
            KeyCode::Backspace => {
                self.send_buffer.pop();
            }
            KeyCode::Enter => self.send_tx_buffer(),            
            _ => {}
        }
    }

    fn set_active_inactive(&mut self, active: bool) {
        self.active = active;
    }

    fn render(&self, area: ratatui::prelude::Rect, buf: &mut ratatui::Frame) {
        let highlight_color = if self.active {
            ratatui::style::Color::Red
        } else {
            ratatui::style::Color::Gray
        };

        let top = "TX Line";
        let block = Block::bordered()
            .title(Line::from(top).left_aligned())
            .border_set(border::THICK)
            .border_style(Style::default().fg(highlight_color));

        let pg = Paragraph::new(Line::from(vec![
            "TX".fg(ratatui::style::Color::LightGreen),
            format!("({}):", self.input_mode).fg(ratatui::style::Color::Gray),
            format!("{}", String::from_utf8_lossy(&self.send_buffer))
                .fg(ratatui::style::Color::Gray),
        ]));

        buf.render_widget(pg.block(block), area);
    }
}

impl InteractiveMode
{

    pub fn new(command_sender : Sender<SerialCommand>) -> Self
    {
        Self
        {
            active: false,
            send_buffer: Vec::new(),
            input_mode: InputMode::Default,
            command_sender,
            crlf_setting: CRLFSetting::None
        }
    }

    pub fn set_crlf(&mut self, crlf_setting: CRLFSetting) {
        self.crlf_setting = crlf_setting;
    }

    /// Converts two hexadecimal bytes into a single `char`.
    ///
    /// # Arguments
    ///
    /// * `b0` - The high-order 4 bits represented as a `u8`.
    /// * `b1` - The low-order 4 bits represented as a `u8`.
    ///
    /// # Returns
    ///
    /// A `char` representation of the combined byte formed by shifting `b0` left by 4 bits
    /// and adding `b1`.
    ///
    /// # Panics
    ///
    /// This function will panic if the resulting byte does not correspond to a valid
    /// Unicode code point.
    fn two_hex_bytes_to_char(&self, b0: u8, b1: u8) -> char {
        let byte = (b0 << 4) + b1;
        char::from_u32(byte as u32).unwrap()
    }

/// if hex input, convert data to bytes by aggregating 2 hex chars into one byte
fn apply_input_mode(&mut self) {
    // if hex input, convert data to bytes by aggregating 2 hex chars into one byte
    if self.input_mode == InputMode::Hex {
        let mut new_buffer: Vec<u8> = vec![];
        let mut idx: usize = 0;
        while idx < self.send_buffer.len() - 1 {
            if self.send_buffer[idx] as char == ' ' {
                idx += 1;
                continue;
            }
            let b0 = (self.send_buffer[idx] as char).to_digit(16).unwrap() as u8;
            let b1 = (self.send_buffer[idx + 1] as char).to_digit(16).unwrap() as u8;
            new_buffer.push(self.two_hex_bytes_to_char(b0, b1) as u8);
            idx += 2;
        }
        self.send_buffer = new_buffer;
    }
}

    /// Adds the necessary CRLF bytes to the send buffer according to the
    /// current CRLF setting.
    fn apply_crlf_setting(&mut self) {
        match self.crlf_setting {
            CRLFSetting::CRLF => {
                self.send_buffer.push(b'\r');
                self.send_buffer.push(b'\n');
            }
            CRLFSetting::LF => {
                self.send_buffer.push(b'\n');
            }
            CRLFSetting::CR => {
                self.send_buffer.push(b'\r');
            }
            _ => {}
        }
    }

        /// Sends the contents of the `send_buffer` to the serial port.
    /// The contents of `send_buffer` are processed according to the current
    /// `input_mode` and `crlf` settings before being sent.
    fn send_tx_buffer(&mut self) {
        self.apply_input_mode();
        self.apply_crlf_setting();
        self.send_command(SerialCommand::Send(self.send_buffer.clone()));
        self.send_buffer.clear();
    }

        /// Rotates the input mode through the list of available input modes. The input modes
    /// are cycled in order, so if the input mode is Default, it will be set to Hex, and
    /// vice versa.
    pub fn rotate_input_mode(&mut self) {
        //println!("Here!");
        let mut selected_idx = INPUT_MODES
            .iter()
            .position(|&x| x == self.input_mode)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= INPUT_MODES.len();
        self.input_mode = INPUT_MODES[selected_idx];
    }

    fn send_command(&self, cmd: SerialCommand) {
       self.command_sender.send(cmd).unwrap();
    }
}