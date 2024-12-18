use std::time::Duration;

use crossterm::event::KeyCode;
use ratatui::{
    style::{Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, Paragraph},
};

use serde::{Deserialize, Serialize};
use snafu::{prelude::*, Whatever};

use crate::{
    mode::ApplicationMode,
    portthread::{PortError, SerialContext},
    serialtypes::{BAUD_RATES, DATABITS, PARITY, STOP_BITS},
    DisplayMode, DISPLAY_MODES,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsMode {
    port: String,
    baud: u32,
    stopbits: u8,
    parity: String,
    databits: u8,
    display_mode: DisplayMode,
    #[serde(skip_serializing, default)]
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
            _ => {}
        }
        self.try_write_config_file();
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
        ]));

        buf.render_widget(opts.block(block), area);
    }

    fn set_active_inactive(&mut self, active: bool) {
        self.active = active;
    }
}

impl SettingsMode {
    fn try_load_config_file() -> Result<SettingsMode, Whatever> {
        // if a .klemme file is present in the CWD, try
        // to deserialize settings from it:
        let path = std::env::current_dir().unwrap();
        let path = path.to_str().unwrap();
        if let Ok(file) = std::fs::File::open(path.to_owned() + "/.klemme") {
            let reader = std::io::BufReader::new(file);
            let res: Result<SettingsMode, serde_json::Error> = serde_json::from_reader(reader);

            if let Ok(settings) = res {
                return Ok(settings);
            }
            return res.with_whatever_context(|_| "Failed to deserialize .klemme file");
        }
        whatever!("no .klemme file found in CWD")
    }

    fn try_write_config_file(&self) {
        // if a .klemme file is present in the CWD, try
        // to deserialize settings from it:
        let path = std::env::current_dir().unwrap();
        let path = path.to_str().unwrap();
        let file = std::fs::File::create(path.to_owned() + "/.klemme").unwrap();
        let writer = std::io::BufWriter::new(file);
        let _ = serde_json::to_writer_pretty(writer, &self);
    }

    pub fn new() -> SettingsMode {
        match SettingsMode::try_load_config_file() {
            Ok(settings) => return settings,
            Err(_) => {}
        };

        let mut res = Self {
            port: "".to_string(),
            baud: BAUD_RATES[0],
            stopbits: STOP_BITS[0],
            parity: PARITY[0].to_string(),
            databits: DATABITS[3],
            display_mode: DisplayMode::Hex,
            active: false,
        };
        res.rotate_port();
        res
    }

    pub fn get_display_mode(&self) -> DisplayMode {
        self.display_mode
    }

    pub fn create_serial_context(&self) -> Result<SerialContext, PortError> {
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

        if let Ok(mut p) = the_port {
            if let Err(_) = p.set_read_timeout(Duration::from_millis(125)) {
                return Err(PortError::BadSettings);
            }
            if let Err(_) = p.set_write_timeout(Duration::from_millis(2500)) {
                return Err(PortError::BadSettings);
            }
            if let Err(_) = p.flush() {
                return Err(PortError::FailedToFlush);
            }
            if let Err(_) = p.discard_buffers() {
                return Err(PortError::BadSettings);
            }

            return Ok(SerialContext::new(self.port.clone(), p));
        } else {
            return Err(PortError::FailedToOpen);
        }
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
