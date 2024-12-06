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
    Normal,
    Settings,
    Interactive,
    Analyzer,
}

#[derive(Debug, Default, PartialEq)]
pub enum Endianness {
    Big,
    #[default]
    Little,
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
#[derive(Debug)]
enum PortThreadState {
    Stopped,
    Running(SerialContext),
}

impl PartialEq for PortThreadState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PortThreadState::Stopped, PortThreadState::Stopped) => true,
            (PortThreadState::Running(_), PortThreadState::Running(_)) => true,
            _ => false,
        }
    }
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

impl Display for Endianness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endianness::Big => write!(f, "Big"),
            Endianness::Little => write!(f, "Little"),
        }
    }
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
    port_name: String,
    com_port: Option<serial2::SerialPort>,
}

impl PartialEq for SerialContext {
    fn eq(&self, other: &Self) -> bool {
        self.port_name == other.port_name
    }
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
    analyzer_endianness: Endianness,
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
            analyzer_endianness: Endianness::Little,
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
            // limit framerate
            thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        let evt = event::poll(Duration::from_millis(5))?;
        if evt {
            match event::read()? {
                // it's important to check that the event is a key press event as
                // crossterm also emits key release and repeat events on Windows.
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)
                }
                _ => {}
            };
        }
        Ok(())
    }

    fn do_settings_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('p') => self.rotate_port(),
            KeyCode::Char('b') => self.rotate_baudrate(),
            KeyCode::Char('s') => self.rotate_stopbits(),
            KeyCode::Char('a') => self.rotate_parity(),
            KeyCode::Char('d') => self.rotate_databits(),
            KeyCode::Char('m') => self.rotate_display_mode(),
            KeyCode::Char('c') => self.rotate_crlf_setting(),
            KeyCode::Enter => self.enter_interactive_mode(),
            _ => {}
        }
    }

    fn do_interactive_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
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
            KeyCode::F(3) => self.rotate_input_mode(),
            KeyCode::F(2) => self.rotate_display_mode(),
            _ => {}
        }
    }

    fn do_analyzer_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            KeyCode::F(2) => self.rotate_display_mode(),
            KeyCode::Char('e') => self.rotate_analyzer_endianness(),
            _ => {}
        }
    }

    fn do_normal_mode(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::Esc {
            self.exit();
        }
        if key_event.code == KeyCode::Char('s') {
            self.enter_settings_mode();
        }
        if key_event.code == KeyCode::Char('i') {
            self.enter_interactive_mode();
        }
        if key_event.code == KeyCode::Char('a') {
            self.enter_analyzer_mode();
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::Esc && self.mode != Mode::Normal {
            self.enter_normal_mode();
            return;
        }
        // if key_event.code == KeyCode::F(2) {
        //     self.rotate_display_mode();
        // }
        // if key_event.code == KeyCode::F(3) {
        //     self.rotate_input_mode();
        // }

        if key_event.code == KeyCode::F(10) {
            self.display_history.clear();
        }

        match self.mode {
            Mode::Settings => {
                self.do_settings_mode(key_event);
                return;
            }
            Mode::Interactive => self.do_interactive_mode(key_event),
            Mode::Analyzer => self.do_analyzer_mode(key_event),
            Mode::Normal => self.do_normal_mode(key_event),
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

    /// Convert a control character to a string representation.
    ///
    /// This function takes a byte containing a control character and returns a string
    /// representation of that character. The string representation is of the form `<X>`,
    /// where `X` is the name of the control character. For example, a byte with the value 0x00
    /// would return the string `"<NUL>"`.
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
        let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Start);
        let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::End);
        let [area] = vertical.areas(area);
        let [area] = horizontal.areas(area);
        area
    }

    fn draw_rxtxbuffer(&mut self, area: Rect, buf: &mut Frame) {
        let highlight_color = if self.mode == Mode::Analyzer {
            ratatui::style::Color::Red
        } else {
            ratatui::style::Color::Gray
        };
        self.update_history_with_incoming_data();
        let mut analyzer_data: Vec<u8> = Vec::new();
        let num_rows = area.height as usize;
        let items = self.build_list_items(&mut analyzer_data, num_rows);
        let list = List::new(items)
            .block(Block::bordered().title("History"))
            .style(Style::new().fg(highlight_color))
            .highlight_style(Style::new().fg(ratatui::style::Color::Red))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::BottomToTop);

        buf.render_widget(list, area);
        self.render_analyzer(area, buf, analyzer_data);
    }

    fn build_list_items(
        &mut self,
        analyzer_data: &mut Vec<u8>,
        max_num_rows: usize,
    ) -> Vec<Line<'_>> {
        let mut line_index = 0;
        let items: Vec<Line> = self
            .display_history
            .iter()
            .rev()
            .skip(self.scroll_offset as usize)
            .take(max_num_rows)
            .map(|x| {
                let result = match x {
                    SerialStateMessage::DataEvent(x) => {
                        let bytes = self.format_data(&x.data);

                        let mut pre_cursor = String::from(bytes.clone());
                        let mut cursor = String::from("");
                        let mut post_cursor = String::from("");
                        let mut highlight_string = String::from("");
                        let mut cursor_color = ratatui::style::Color::Black;
                        let mut post_cursor_color = ratatui::style::Color::Black;

                        if self.display_mode == DisplayMode::Hex
                            && line_index == 0
                            && self.mode == Mode::Analyzer
                        {
                            // the cursor pos is always a multiple of 3:
                            let pos = self.analyzer_cursor_pos * 3;
                            if pos <= bytes.len() - 3 {
                                pre_cursor = String::from(&bytes[0..pos]);
                                cursor = String::from(&bytes[pos..pos + 2]);
                                let highlight_len = if (bytes.len() - pos - 2) > 24 {
                                    24
                                } else {
                                    bytes.len() - pos - 2
                                };
                                highlight_string =
                                    String::from(&bytes[pos + 2..pos + 2 + highlight_len]);
                                post_cursor = String::from(&bytes[pos + 2 + highlight_len..]);
                            }
                            cursor_color = ratatui::style::Color::Blue;
                            post_cursor_color = ratatui::style::Color::DarkGray;
                            *analyzer_data = x.data.to_vec();
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
                            format!("{}", highlight_string)
                                .fg(ratatui::style::Color::Gray)
                                .bg(post_cursor_color),
                            format!("{}", post_cursor).fg(ratatui::style::Color::Gray),
                        ]);
                        line_index += 1;
                        ln
                    }
                    SerialStateMessage::ErrorEvent(x) => Line::raw(x),
                    SerialStateMessage::Started => {
                        Line::from(vec!["--- Started ---".fg(ratatui::style::Color::Green)])
                    }
                    SerialStateMessage::Stopped => {
                        Line::from(vec!["--- Stopped ---".fg(ratatui::style::Color::LightRed)])
                    }
                };
                return result;
            })
            .collect();
        items
    }

    fn update_history_with_incoming_data(&mut self) {
        // copy state events to display history:
        if let Some(ref s) = self.state_receiver {
            while let Ok(x) = s.try_recv() {
                self.display_history.push(x);
            }
        }
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

        if self.analyzer_cursor_pos >= analyzer_data.len() {
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
            let two_as_u16: u16;
            let two_as_i16: i16;
            if self.analyzer_endianness == Endianness::Big {
                two_as_u16 = u16::from_be_bytes(two_bytes.clone().try_into().unwrap());
                two_as_i16 = i16::from_be_bytes(two_bytes.clone().try_into().unwrap());
            } else {
                two_as_u16 = u16::from_le_bytes(two_bytes.clone().try_into().unwrap());
                two_as_i16 = i16::from_le_bytes(two_bytes.clone().try_into().unwrap());
            }
            items.push(format!("u16: {}", two_as_u16));
            items.push(format!("i16: {}", two_as_i16));
        }

        if (self.analyzer_cursor_pos as i32) <= (analyzer_data.len() as i32 - 4) {
            let four_bytes =
                analyzer_data[self.analyzer_cursor_pos..=self.analyzer_cursor_pos + 3].to_vec();
            let four_as_u32: u32;
            let four_as_i32: i32;
            let four_as_f32: f32;

            if self.analyzer_endianness == Endianness::Big {
                four_as_u32 = u32::from_be_bytes(four_bytes.clone().try_into().unwrap());
                four_as_i32 = i32::from_be_bytes(four_bytes.clone().try_into().unwrap());
                four_as_f32 = f32::from_be_bytes(four_bytes.clone().try_into().unwrap());
            } else {
                four_as_u32 = u32::from_le_bytes(four_bytes.clone().try_into().unwrap());
                four_as_i32 = i32::from_le_bytes(four_bytes.clone().try_into().unwrap());
                four_as_f32 = f32::from_le_bytes(four_bytes.clone().try_into().unwrap());
            }

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

        let headline = Line::from(vec![
            "Analyzer,".fg(ratatui::style::Color::Gray),
            format!(" {} ", self.analyzer_endianness).fg(ratatui::style::Color::Gray),
            "e".fg(ratatui::style::Color::Red),
            "ndian".fg(ratatui::style::Color::Gray),
        ]);

        let list = List::new(items)
            .block(Block::bordered().title(headline))
            .style(Style::new().fg(ratatui::style::Color::Gray))
            .highlight_style(Style::new().fg(ratatui::style::Color::Red))
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);

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

    /// Renders the application's UI. The UI is split into three rows.
    /// The first row contains the header, which displays the current settings.
    /// The second row contains the RX/TX buffer, which displays the data sent and received over the serial port.
    /// The third row contains the TX line, which is where the user can enter data to send over the serial port.
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
            }
            else {
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

    /// Rotates the CRLF setting.
    ///
    /// The following settings are available:
    /// None: No data is appended
    /// CR: A CR character is appended after each user input
    /// LF: An LF character is appended after each user input
    /// CRLF: A CR and an LF character are appended after each user input
    fn rotate_crlf_setting(&mut self) {
        let mut selected_idx = CRLF_SETTINGS
            .iter()
            .position(|&x| x == self.crlf)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= CRLF_SETTINGS.len();
        self.crlf = CRLF_SETTINGS[selected_idx];
    }

    /// Rotates the input mode through the list of available input modes. The input modes
    /// are cycled in order, so if the input mode is Default, it will be set to Hex, and
    /// vice versa.
    fn rotate_input_mode(&mut self) {
        let mut selected_idx = INPUT_MODES
            .iter()
            .position(|&x| x == self.input_mode)
            .unwrap_or(0);
        selected_idx += 1;
        selected_idx %= INPUT_MODES.len();
        self.input_mode = INPUT_MODES[selected_idx];
    }

    /// Enters the interactive mode, which establishes a connection to the serial port
    /// with the current configuration settings. The function configures the serial port
    /// settings, such as baud rate, stop bits, parity, and character size, and opens the port.
    /// It sets read and write timeouts and starts the communication by sending a Start command
    /// with the established serial context.
    ///
    /// # Panics
    ///
    /// This function will panic if there is a failure in opening the serial port or setting
    /// the read/write timeouts.
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
        p.flush().expect("Failed to flush port.");
        let _= p.discard_buffers();
        let ctx = SerialContext { com_port: Some(p), port_name: self.port.clone() };

        self.send_command(SerialCommand::Start(ctx));
    }

    /// Exits the current mode and enters the settings mode, which is a mode where the user can adjust
    /// the port, baud rate, stop bits, parity, and data bits of the serial connection.
    fn enter_settings_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.mode = Mode::Settings;
    }

    /// Enters the analyzer mode, which is a special interactive mode that renders the hexadecimal,
    /// signed and unsigned 8, 16, 32, and 64 bit, as well as floating point 32 and 64 bit values of the byte
    /// at the cursor position.
    fn enter_analyzer_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.mode = Mode::Analyzer;
    }

    fn enter_normal_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.mode = Mode::Normal;
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

    /// Sends the contents of the `send_buffer` to the serial port.
    /// The contents of `send_buffer` are processed according to the current
    /// `input_mode` and `crlf` settings before being sent.
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

    /// Returns the next command from the main thread, or `None` if there are no commands
    /// to process.
    ///
    /// # Behavior
    ///
    /// If the serial port is stopped, this function will block until a command is received.
    /// If the serial port is running, this function will non-blockingly return the next command
    /// if there is one, or `None` if there are no commands to process.
    fn receive_command(state: &PortThreadState, rx: &Receiver<SerialCommand>) -> Option<SerialCommand>
    {
        if *state == PortThreadState::Stopped {
            if let Ok(rxd) = rx.recv()
            {
                return Some(rxd)
            }
            return  None
        }else
        {
            if let Ok(rxd )= rx.try_recv()
            {
                return  Some(rxd)
            }
            return  None
        };
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

                // if the state is stopped, wait until rx receives something:
                let _cmd = Self::receive_command(&state, &rx);
                

                if let Some(cmd) = _cmd{
                    match cmd {
                        SerialCommand::Send(data) => {
                            data_to_send = data;
                        }
                        SerialCommand::Stop => {
                            if state != PortThreadState::Stopped {
                                let _ = tx.send(SerialStateMessage::Stopped);
                                state = PortThreadState::Stopped;
                            }
                        }
                        SerialCommand::Start(ctx) => {
                            if state == PortThreadState::Stopped {
                                let _ = tx.send(SerialStateMessage::Started);
                                state = PortThreadState::Running(ctx);
                            }
                        }
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
        });
    }

    /// Sends a serial command to the background thread via the command sender channel.
    ///
    /// # Arguments
    ///
    /// * `cmd` - A `SerialCommand` variant to be sent to the background thread. This could
    ///   be a command to start, stop, or send data through the serial port.
    fn send_command(&self, cmd: SerialCommand) {
        if let Some(p) = &self.command_sender {
            p.send(cmd).unwrap();
        }
    }

    /// Scroll up in the display history, moving the top line down by one.
    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    /// Scroll down in the display history, moving the top line up by one.
    fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Move the analyzer cursor one character to the left. This will move the highlighted
    /// character in the analyzer window to the left by one position. If the cursor is
    /// already at the left edge of the window, this function has no effect.
    fn cursor_left(&mut self) {
        self.analyzer_cursor_pos = self.analyzer_cursor_pos.saturating_sub(1);
    }

    /// Move the analyzer cursor one character to the right. This will move the highlighted
    /// character in the analyzer window to the right by one position. If the cursor is
    /// already at the right edge of the window, this function has no effect.
    fn cursor_right(&mut self) {
        self.analyzer_cursor_pos += 1;
    }

    fn rotate_analyzer_endianness(&mut self) {
        if self.analyzer_endianness == Endianness::Big {
            self.analyzer_endianness = Endianness::Little
        } else {
            self.analyzer_endianness = Endianness::Big
        }
    }
}
