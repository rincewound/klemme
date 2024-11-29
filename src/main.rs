use std::{
    fmt::Display,
    io,
    thread::{self},
    time::Duration,
    vec,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, Clear, List, ListDirection, Paragraph},
    DefaultTerminal, Frame,
};
use serial2::Settings;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

const BAUD_RATES: [u32; 8] = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

const STOP_BITS: [u8; 3] = [1, 2, 3];

const PARITY: [&str; 3] = ["None", "Odd", "Even"];

const DATABITS: [u8; 5] = [5, 6, 7, 8, 9];

const DISPLAY_MODES: [DisplayMode; 5] = [
    DisplayMode::Decimal,
    DisplayMode::Hex,
    DisplayMode::Ascii,
    DisplayMode::MixedHex,
    DisplayMode::MixedDec,
];

const CRLF_SETTINGS: [CRLFSetting; 4] = [
    CRLFSetting::None,
    CRLFSetting::CR,
    CRLFSetting::LF,
    CRLFSetting::CRLF,
];

const INPUT_MODES: [InputMode; 2] = [InputMode::Default, InputMode::Hex];

#[derive(Debug, Default, PartialEq)]
pub enum Mode {
    #[default]
    Settings,
    Interactive,
    Analyzer,
}

#[derive(Debug, Default, PartialEq)]
pub enum RxTx {
    #[default]
    Rx,
    Tx,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Decimal,
    Hex,
    Ascii,
    MixedHex,
    MixedDec,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Default,
    Hex,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CRLFSetting {
    None,
    CR,
    LF,
    CRLF,
}

enum PortThreadState {
    Stopped,
    Running(SerialContext),
}

enum SerialCommand {
    Stop,
    Start(SerialContext),
    Send(Vec<u8>),
}

#[derive(Debug)]
enum SerialStateMessage {
    DataEvent(HistoryEntry),
    ErrorEvent(String),
    Started,
    Stopped,
}

impl Display for CRLFSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CRLFSetting::None => write!(f, "None"),
            CRLFSetting::CR => write!(f, "CR"),
            CRLFSetting::LF => write!(f, "LF"),
            CRLFSetting::CRLF => write!(f, "CRLF"),
        }
    }
}

impl Display for InputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputMode::Default => write!(f, "Default"),
            InputMode::Hex => write!(f, "Hex"),
        }
    }
}

impl Display for DisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayMode::Decimal => write!(f, "Decimal"),
            DisplayMode::Hex => write!(f, "Hex"),
            DisplayMode::Ascii => write!(f, "Ascii"),
            DisplayMode::MixedHex => write!(f, "Mixed Hex"),
            DisplayMode::MixedDec => write!(f, "Mixed Decimal"),
        }
    }
}

impl Display for RxTx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RxTx::Rx => write!(f, "RX"),
            RxTx::Tx => write!(f, "TX"),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct HistoryEntry {
    rx_tx: RxTx,
    data: vec::Vec<u8>,
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let app_result = App::default().run(&mut terminal);
    ratatui::restore();
    app_result
}

#[derive(Debug)]
struct SerialContext {
    com_port: Option<serial2::SerialPort>,
}

#[derive(Debug)]
pub struct App {
    exit: bool,
    port: String,
    baud: u32,
    stopbits: u8,
    parity: String,
    databits: u8,
    mode: Mode,
    send_buffer: Vec<u8>,
    crlf: CRLFSetting,
    display_mode: DisplayMode,
    display_history: Vec<SerialStateMessage>,
    command_sender: Option<Sender<SerialCommand>>,
    state_receiver: Option<Receiver<SerialStateMessage>>,
    scroll_offset: u32,
    analyzer_cursor_pos: usize,
    input_mode: InputMode,
}

impl Default for App {
    fn default() -> Self {
        let data = App {
            port: "".to_string(),
            baud: BAUD_RATES[0],
            stopbits: STOP_BITS[0],
            parity: PARITY[0].to_string(),
            databits: DATABITS[3],
            mode: Mode::Settings,
            send_buffer: vec![],
            display_history: vec![],
            exit: false,
            command_sender: None,
            state_receiver: None,
            display_mode: DisplayMode::Hex,
            crlf: CRLFSetting::None,
            scroll_offset: 0,
            analyzer_cursor_pos: 0,
            input_mode: InputMode::Default,
        };

        data
    }
}

impl App {
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        // initial setup:
        self.rotate_port();
        self.baud = BAUD_RATES[0];
        self.stopbits = STOP_BITS[0];
        self.parity = PARITY[0].to_string();
        self.databits = DATABITS[3];

        let (stx, rtx): (Sender<SerialStateMessage>, Receiver<SerialStateMessage>) =
            mpsc::channel();
        let (tx, rx): (Sender<SerialCommand>, Receiver<SerialCommand>) = mpsc::channel();
        self.command_sender = Some(tx);
        self.state_receiver = Some(rtx);
        self.port_background_thread(rx, stx);

        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn do_settings_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),

            KeyCode::Char('p') => self.rotate_port(),
            KeyCode::Char('b') => self.rotate_baudrate(),
            KeyCode::Char('s') => self.rotate_stopbits(),
            KeyCode::Char('a') => self.rotate_parity(),
            KeyCode::Char('d') => self.rotate_databits(),
            KeyCode::Char('m') => self.rotate_display_mode(),
            KeyCode::Char('c') => self.rotate_crlf_setting(),
            KeyCode::Char(' ') => self.enter_interactive_mode(),
            KeyCode::Enter => self.enter_interactive_mode(),
            KeyCode::Backspace => self.enter_analyzer_mode(),
            _ => {}
        }
    }

    fn do_interactive_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            KeyCode::Esc => self.enter_settings(),
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
            KeyCode::F(4) => self.rotate_input_mode(),
            _ => {}
        }
    }

    fn do_analyzer_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.enter_settings(),
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::F(2) {
            self.rotate_display_mode();
        }
        if key_event.code == KeyCode::F(3) {
            self.display_history.clear();
        }

        match self.mode {
            Mode::Settings => {
                self.do_settings_mode(key_event);
                return;
            }
            Mode::Interactive => self.do_interactive_mode(key_event),
            Mode::Analyzer => self.do_analyzer_mode(key_event),
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn draw_header(&self, area: Rect, buf: &mut Frame) {
        let top = "Settings";

        let highlight_color = if self.mode == Mode::Settings {
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
            format!("ort:{};", self.port).fg(ratatui::style::Color::Gray),
            "B".fg(highlight_color),
            format!("aud:{};", self.baud).fg(ratatui::style::Color::Gray),
            "S".fg(highlight_color),
            format!("topbits:{};", self.stopbits).fg(ratatui::style::Color::Gray),
            "P".fg(ratatui::style::Color::Gray),
            "a".fg(highlight_color),
            format!("rity:{};", self.parity).fg(ratatui::style::Color::Gray),
            "D".fg(highlight_color),
            format!("atabits:{}", self.databits).fg(ratatui::style::Color::Gray),
            " Display".fg(ratatui::style::Color::Gray),
            "M".fg(highlight_color),
            format!("ode:{};", self.display_mode).fg(ratatui::style::Color::Gray),
            "C".fg(highlight_color),
            format!("RLF:{}", self.crlf).fg(ratatui::style::Color::Gray),
        ]));

        buf.render_widget(opts.block(block), area);
    }

    fn control_char_to_string(c: u8) -> String {
        let chr = match c {
            0x00 => "NUL",
            0x01 => "SOH",
            0x02 => "STX",
            0x03 => "ETX",
            0x04 => "EOT",
            0x05 => "ENQ",
            0x06 => "ACK",
            0x07 => "BEL",
            0x08 => "BS",
            0x09 => "HT",
            0x0A => "LF",
            0x0B => "VT",
            0x0C => "FF",
            0x0D => "CR",
            0x0E => "SO",
            0x0F => "SI",
            0x10 => "DLE",
            0x11 => "DC1",
            0x12 => "DC2",
            0x13 => "DC3",
            0x14 => "DC4",
            0x15 => "NAK",
            0x16 => "SYN",
            0x17 => "ETB",
            0x18 => "CAN",
            0x19 => "EM",
            0x1A => "SUB",
            0x1B => "ESC",
            0x1C => "FS",
            0x1D => "GS",
            0x1E => "RS",
            0x1F => "US",
            _ => " ",
        };
        return format!("<{}>", chr);
    }

    fn format_data(&self, data: &[u8]) -> String {
        match self.display_mode {
            DisplayMode::Hex => data.iter().map(|x| format!("{:02X} ", x)).collect(),
            DisplayMode::Ascii => {
                // replace control bytes by their name:
                let data = data
                    .iter()
                    .map(|x| {
                        if !(*x as char).is_control() {
                            return format!("{}", (*x) as char);
                        }
                        let chr = Self::control_char_to_string(*x);
                        format!("{}", chr)
                    })
                    .collect::<Vec<String>>()
                    .join("");
                data
            }
            DisplayMode::Decimal => data
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(" "),
            DisplayMode::MixedHex => {
                // all bytes, that are printable characters are printed as such, otherwise hex
                data.iter()
                    .map(|x| {
                        if (*x as char).is_ascii() && !(*x as char).is_control() {
                            return format!("{}", (*x) as char);
                        }
                        format!("{:02X}", *x)
                    })
                    .collect::<Vec<String>>()
                    .join(" ")
            }
            DisplayMode::MixedDec => data
                .iter()
                .map(|x| {
                    if (*x as char).is_ascii() && !(*x as char).is_control() {
                        return format!("{}", (*x) as char);
                    }
                    format!("{}", *x)
                })
                .collect::<Vec<String>>()
                .join(" "),
        }
    }

    fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
        let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
        let [area] = vertical.areas(area);
        let [area] = horizontal.areas(area);
        area
    }

    fn draw_rxtxbuffer(&mut self, area: Rect, buf: &mut Frame) {
        // copy state events to display history:
        if let Some(ref s) = self.state_receiver {
            while let Ok(x) = s.try_recv() {
                self.display_history.push(x);
            }
        }

        let mut line_index = 0;
        let mut analyzer_data: Vec<u8> = Vec::new();

        let items: Vec<Line> = self
            .display_history
            .iter()
            .rev()
            .skip(self.scroll_offset as usize)
            .take(10)
            .map(|x| {
                let result = match x {
                    SerialStateMessage::DataEvent(x) => {
                        let bytes = self.format_data(&x.data);

                        let mut pre_cursor = String::from(bytes.clone());
                        let mut cursor = String::from("");
                        let mut post_cursor = String::from("");
                        let mut cursor_color = ratatui::style::Color::Black;

                        if self.display_mode == DisplayMode::Hex && line_index == 0 {
                            // the cursor pos is always a multiple of 3:
                            let pos = self.analyzer_cursor_pos * 3;
                            if pos <= bytes.len() - 3 {
                                pre_cursor = String::from(&bytes[0..pos]);
                                cursor = String::from(&bytes[pos..pos + 2]);
                                post_cursor = String::from(&bytes[pos + 2..]);
                            }
                            cursor_color = ratatui::style::Color::Blue;
                            analyzer_data = x.data.to_vec();
                        }

                        let ln = Line::from(vec![
                            x.rx_tx.to_string().fg(if x.rx_tx == RxTx::Tx {
                                ratatui::style::Color::Green
                            } else {
                                ratatui::style::Color::Red
                            }),
                            format!(":").fg(ratatui::style::Color::Gray),
                            format!("{}", pre_cursor).fg(ratatui::style::Color::Gray),
                            format!("{}", cursor)
                                .fg(ratatui::style::Color::Gray)
                                .bg(cursor_color),
                            format!("{}", post_cursor).fg(ratatui::style::Color::Gray),
                        ]);
                        line_index += 1;
                        ln
                    }
                    SerialStateMessage::ErrorEvent(x) => Line::raw(x),
                    SerialStateMessage::Started => Line::raw("--- Started ---"),
                    SerialStateMessage::Stopped => Line::raw("--- Stopped ---"),
                };
                return result;
            })
            .collect();

        let list = List::new(items)
            .block(Block::bordered().title("History"))
            .style(Style::new().fg(ratatui::style::Color::Gray))
            .highlight_style(Style::new().fg(ratatui::style::Color::Red))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::BottomToTop);

        buf.render_widget(list, area);

        self.render_analyzer(area, buf, analyzer_data);
        //buf.render_widget(block, area);
    }

    /// Renders the analyzer window. This window will appear if the display mode is hex
    /// and the mode is analyzer. The window will contain the byte, u16, i16, u32, i32, f32, u64, i64, and f64
    /// values of the byte at the cursor position.
    fn render_analyzer(&mut self, area: Rect, buf: &mut Frame<'_>, analyzer_data: Vec<u8>) {
        if self.mode != Mode::Analyzer {
            return;
        }

        if self.display_mode != DisplayMode::Hex {
            return;
        }

        let mut items: Vec<String> = vec![];
        // Use the cursor position to obtain the analyzer data: 1 byte, 2 byte, 4 bytes
        let one_byte = analyzer_data[self.analyzer_cursor_pos];
        items.push(format!("binary {:08b}", one_byte));
        items.push(format!("u8: {}", one_byte));

        if (self.analyzer_cursor_pos as i32) <= (analyzer_data.len() as i32 - 2) {
            let two_bytes =
                analyzer_data[self.analyzer_cursor_pos..=self.analyzer_cursor_pos + 1].to_vec();
            let two_as_u16 = u16::from_le_bytes(two_bytes.clone().try_into().unwrap());
            let two_as_i16 = i16::from_le_bytes(two_bytes.clone().try_into().unwrap());
            items.push(format!("u16: {}", two_as_u16));
            items.push(format!("i16: {}", two_as_i16));
        }

        if (self.analyzer_cursor_pos as i32) <= (analyzer_data.len() as i32 - 4) {
            let four_bytes =
                analyzer_data[self.analyzer_cursor_pos..=self.analyzer_cursor_pos + 3].to_vec();
            let four_as_u32 = u32::from_le_bytes(four_bytes.clone().try_into().unwrap());
            let four_as_i32 = i32::from_le_bytes(four_bytes.clone().try_into().unwrap());
            let four_as_f32 = f32::from_le_bytes(four_bytes.clone().try_into().unwrap());
            items.push(format!("u32: {}", four_as_u32));
            items.push(format!("i32: {}", four_as_i32));
            items.push(format!("f32: {}", four_as_f32));
        }

        if (self.analyzer_cursor_pos as i32) <= (analyzer_data.len() as i32 - 8) {
            let eight_bytes =
                analyzer_data[self.analyzer_cursor_pos..=self.analyzer_cursor_pos + 7].to_vec();
            let eight_as_u64 = u64::from_le_bytes(eight_bytes.clone().try_into().unwrap());
            let eight_as_i64 = i64::from_le_bytes(eight_bytes.clone().try_into().unwrap());
            let eight_as_f64 = f64::from_le_bytes(eight_bytes.clone().try_into().unwrap());
            items.push(format!("u64: {}", eight_as_u64));
            items.push(format!("i64: {}", eight_as_i64));
            items.push(format!("f64: {}", eight_as_f64));
        }

        let list = List::new(items)
            .block(Block::bordered().title("Analyzer"))
            .style(Style::new().fg(ratatui::style::Color::Gray))
            .highlight_style(Style::new().fg(ratatui::style::Color::Red))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::BottomToTop);

        //let block = Block::bordered().title("Analyzer");
        let area = Self::popup_area(area, 40, 40);
        buf.render_widget(Clear, area); //this clears out the background
                                        //buf.render_widget(block, area);
        buf.render_widget(list, area);
    }

    fn draw_tx_line(&self, area: Rect, buf: &mut Frame) {
        let highlight_color = if self.mode == Mode::Interactive {
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

    pub fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());
        self.draw_header(chunks[0], frame);
        self.draw_rxtxbuffer(chunks[1], frame);
        self.draw_tx_line(chunks[2], frame);
    }

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
            for port in ports {
                if port.file_name().unwrap().to_str().unwrap() != self.port {
                    continue;
                }
                self.port = port.file_name().unwrap().to_str().unwrap().to_string();
                port_found = true;
            }
            if !port_found {
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

    fn rotate_display_mode(&mut self) {
        let mut selected_idx = DISPLAY_MODES
            .iter()
            .position(|&x| x == self.display_mode)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= DISPLAY_MODES.len();
        self.display_mode = DISPLAY_MODES[selected_idx];
    }

    fn rotate_crlf_setting(&mut self) {
        let mut selected_idx = CRLF_SETTINGS
            .iter()
            .position(|&x| x == self.crlf)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= CRLF_SETTINGS.len();
        self.crlf = CRLF_SETTINGS[selected_idx];
    }

    fn rotate_input_mode(&mut self) {
        let mut selected_idx = INPUT_MODES
            .iter()
            .position(|&x| x == self.input_mode)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= INPUT_MODES.len();
        self.input_mode = INPUT_MODES[selected_idx];
    }

    fn enter_interactive_mode(&mut self) {
        self.mode = Mode::Interactive;

        let the_port = serial2::SerialPort::open(&self.port, |mut settings: Settings| {
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
        let ctx = SerialContext { com_port: Some(p) };

        self.send_command(SerialCommand::Start(ctx));
    }

    fn enter_settings(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.mode = Mode::Settings;
    }

    fn enter_analyzer_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.mode = Mode::Analyzer;
    }

    fn two_hex_bytes_to_char(&self, b0: u8, b1: u8) -> char {
        let byte = (b0 << 4) + b1;
        char::from_u32(byte as u32).unwrap()
    }

    fn send_tx_buffer(&mut self) {
        self.apply_input_mode();
        self.apply_crlf_setting();
        self.send_command(SerialCommand::Send(self.send_buffer.clone()));
        self.send_buffer.clear();
    }

    /// Adds the necessary CRLF bytes to the send buffer according to the
    /// current CRLF setting.
    fn apply_crlf_setting(&mut self) {
        match self.crlf {
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

    /// Starts a background thread that is responsible for managing the serial port.
    /// This thread will receive commands from the main thread and act accordingly.
    /// The main thread should send commands on the `rx` channel and the background
    /// thread will send events on the `tx` channel.
    ///
    /// # Events
    ///
    /// The background thread will send the following events on the `tx` channel:
    ///
    /// - `SerialStateMessage::Started`: The background thread has successfully
    ///   opened the serial port and is ready to receive and send data.
    /// - `SerialStateMessage::Stopped`: The background thread has stopped and
    ///   closed the serial port.
    /// - `SerialStateMessage::DataEvent(HistoryEntry)`: The background thread
    ///   has received data from the serial port and is sending it back to the
    ///   main thread.
    /// - `SerialStateMessage::ErrorEvent(String)`: The background thread has
    ///   encountered an error while writing to the serial port.
    fn port_background_thread(&self, rx: Receiver<SerialCommand>, tx: Sender<SerialStateMessage>) {
        thread::spawn(move || {
            let mut state = PortThreadState::Stopped;
            loop {
                let mut data_to_send: Vec<u8> = vec![];
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SerialCommand::Send(data) => {
                            data_to_send = data;
                        }
                        SerialCommand::Stop => {
                            let _ = tx.send(SerialStateMessage::Stopped);
                            state = PortThreadState::Stopped;
                        }
                        SerialCommand::Start(ctx) => {
                            let _ = tx.send(SerialStateMessage::Started);
                            state = PortThreadState::Running(ctx);
                        }
                    }

                    match state {
                        PortThreadState::Stopped => {}
                        PortThreadState::Running(ref ctx) => {
                            if let Some(p) = &ctx.com_port {
                                if data_to_send.len() != 0 {
                                    if let Ok(_) = p.write(&data_to_send) {
                                        let entry = HistoryEntry {
                                            rx_tx: RxTx::Tx,
                                            data: data_to_send.clone(),
                                        };
                                        tx.send(SerialStateMessage::DataEvent(entry)).unwrap();
                                    } else {
                                        tx.send(SerialStateMessage::ErrorEvent(
                                            "Failed to write to port".to_string(),
                                        ))
                                        .unwrap();
                                    }
                                }
                                // receive data:
                                let mut buffer: [u8; 256] = [0u8; 256];
                                if let Ok(data) = p.read(&mut buffer) {
                                    let received_bytes = buffer[0..data].to_vec();

                                    let entry = HistoryEntry {
                                        rx_tx: RxTx::Rx,
                                        data: received_bytes,
                                    };
                                    tx.send(SerialStateMessage::DataEvent(entry)).unwrap();
                                    data_to_send.clear();
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    fn send_command(&self, cmd: SerialCommand) {
        if let Some(p) = &self.command_sender {
            p.send(cmd).unwrap();
        }
    }

    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn cursor_left(&mut self) {
        self.analyzer_cursor_pos = self.analyzer_cursor_pos.saturating_sub(1);
    }

    fn cursor_right(&mut self) {
        self.analyzer_cursor_pos += 1;
    }
}
