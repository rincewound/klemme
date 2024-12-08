use std::time::Duration;

use crossterm::event::KeyCode;
use ratatui::{
    style::{Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::{
    mode::ApplicationMode,
    portthread::SerialContext,
    serialtypes::{BAUD_RATES, DATABITS, PARITY, STOP_BITS},
    CRLFSetting, DisplayMode, DISPLAY_MODES,
};

#[derive(Debug)]
pub struct SettingsMode {
    port: String,
    baud: u32,
    stopbits: u8,
    parity: String,
    databits: u8,
    crlf: CRLFSetting,
    display_mode: DisplayMode,
    active: bool,
}

impl ApplicationMode for SettingsMode {
    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('p') => self.rotate_port(),
            KeyCode::Char('b') => self.rotate_baudrate(),
            KeyCode::Char('s') => self.rotate_stopbits(),
            KeyCode::Char('a') => self.rotate_parity(),
            KeyCode::Char('d') => self.rotate_databits(),
            KeyCode::Char('m') => self.rotate_display_mode(),
            //KeyCode::Enter => app.enter_interactive_mode(),
            _ => {}
        }
    }

    fn render(&self, area: ratatui::prelude::Rect, buf: &mut ratatui::Frame) {
        let top = "Settings"; //, active mode: ".to_string() + &self.mode.to_string();

        let highlight_color = if self.active {
            ratatui::style::Color::Red
        } else {
            ratatui::style::Color::Gray
        };

        let block = Block::bordered()
            .title(Line::from(top).left_aligned())
            .border_set(border::THICK)
            .border_style(Style::default().fg(highlight_color));

        let opts = Paragraph::new(Line::from(vec![
            "P".fg(highlight_color),
            format!("ort:{} ", self.port).fg(ratatui::style::Color::Gray),
            "B".fg(highlight_color),
            format!("aud:{} ", self.baud).fg(ratatui::style::Color::Gray),
            "D".fg(highlight_color),
            format!("atabits:{} ", self.databits).fg(ratatui::style::Color::Gray),
            "P".fg(ratatui::style::Color::Gray),
            "a".fg(highlight_color),
            format!("rity:{} ", self.parity).fg(ratatui::style::Color::Gray),
            "S".fg(highlight_color),
            format!("topbits:{} ", self.stopbits).fg(ratatui::style::Color::Gray),
            "Display".fg(ratatui::style::Color::Gray),
            "M".fg(highlight_color),
            format!("ode:{} ", self.display_mode).fg(ratatui::style::Color::Gray),
            "C".fg(highlight_color),
            format!("RLF:{} ", self.crlf).fg(ratatui::style::Color::Gray),
        ]));

        buf.render_widget(opts.block(block), area);
    }

    fn set_active_inactive(&mut self, active: bool) {
        self.active = active;
    }
}

impl SettingsMode {
    pub fn new() -> SettingsMode {
        let mut res = Self {
            port: "".to_string(),
            baud: BAUD_RATES[0],
            stopbits: STOP_BITS[0],
            parity: PARITY[0].to_string(),
            databits: DATABITS[3],
            crlf: CRLFSetting::None,
            display_mode: DisplayMode::Hex,
            active: false,
        };
        res.rotate_port();
        res
    }

    pub fn get_display_mode(&self) -> DisplayMode {
        self.display_mode
    }

    pub fn create_serial_context(&self) -> SerialContext {
        let the_port = serial2::SerialPort::open(&self.port, |mut settings: serial2::Settings| {
            let _ = settings.set_baud_rate(self.baud);
            let stop_bits = match self.stopbits {
                1 => serial2::StopBits::One,
                2 => serial2::StopBits::Two,
                _ => serial2::StopBits::One,
            };

            settings.set_stop_bits(stop_bits);

            let parity = match self.parity.as_str() {
                "N" => serial2::Parity::None,
                "E" => serial2::Parity::Even,
                "O" => serial2::Parity::Odd,
                _ => serial2::Parity::None,
            };
            settings.set_parity(parity);

            let char_size = match self.databits {
                5 => serial2::CharSize::Bits5,
                6 => serial2::CharSize::Bits6,
                7 => serial2::CharSize::Bits7,
                8 => serial2::CharSize::Bits8,
                _ => serial2::CharSize::Bits8,
            };

            settings.set_char_size(char_size);

            let stop_bits = match self.stopbits {
                1 => serial2::StopBits::One,
                2 => serial2::StopBits::Two,
                _ => serial2::StopBits::One,
            };

            settings.set_stop_bits(stop_bits);
            Ok(settings)
        });

        let mut p = the_port.expect("Failed to open port with given settings.");
        p.set_read_timeout(Duration::from_millis(125))
            .expect("Failed to set read timeout.");
        p.set_write_timeout(Duration::from_millis(2500))
            .expect("Failed to set write timeout.");
        p.flush().expect("Failed to flush port.");
        let _ = p.discard_buffers();
        let ctx = SerialContext::new(self.port.clone(), p);
        ctx
    }

    /// Rotates the serial port to the next available serial port.
    ///
    /// Enumerates all available serial ports, and sets the serial port to the next available port
    /// in the list. If the current port is not found in the list, or if the list is empty, the
    /// port is set to the first port in the list.
    fn rotate_port(&mut self) {
        // enumerate comports
        let mut port_found = false;
        if let Ok(ports) = serial2::SerialPort::available_ports() {
            if ports.len() == 0 {
                self.port = "".to_string();
                return;
            }
            let first_port_name = ports
                .first()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();

            // find index of selected port:
            let mut idx = 0;
            for (i, port) in ports.iter().enumerate() {
                if port.file_name().unwrap().to_str().unwrap() != self.port {
                    continue;
                }
                port_found = true;
                idx = i;
                break;
            }

            if port_found {
                idx += 1;
                idx %= ports.len();
                self.port = ports[idx]
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
            } else {
                self.port = first_port_name.to_string();
            }
        }
    }

    fn rotate_baudrate(&mut self) {
        let mut selected_idx = BAUD_RATES.iter().position(|&x| x == self.baud).unwrap_or(0);
        selected_idx += 1;
        selected_idx %= BAUD_RATES.len();
        self.baud = BAUD_RATES[selected_idx];
    }

    fn rotate_stopbits(&mut self) {
        let mut selected_idx = STOP_BITS
            .iter()
            .position(|&x| x == self.stopbits)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= STOP_BITS.len();
        self.stopbits = STOP_BITS[selected_idx];
    }

    fn rotate_parity(&mut self) {
        let mut selected_idx = PARITY.iter().position(|&x| x == self.parity).unwrap_or(0);
        selected_idx += 1;
        selected_idx %= PARITY.len();
        self.parity = PARITY[selected_idx].to_string();
    }

    fn rotate_databits(&mut self) {
        let mut selected_idx = DATABITS
            .iter()
            .position(|&x| x == self.databits)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= DATABITS.len();
        self.databits = DATABITS[selected_idx];
    }

    pub fn rotate_display_mode(&mut self) {
        let mut selected_idx = DISPLAY_MODES
            .iter()
            .position(|&x| x == self.display_mode)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= DISPLAY_MODES.len();
        self.display_mode = DISPLAY_MODES[selected_idx];
    }
}
