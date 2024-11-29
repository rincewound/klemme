use std::{
    fmt::Display,
    io,
    thread::{self},
    time::Duration,
    vec,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    symbols:: border,
    text::Line,
    widgets::{Block, List, ListDirection, Paragraph},
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

#[derive(Debug, Default, PartialEq)]
pub enum Mode {
    #[default]
    Settings,
    Interactive,
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
            _ => {}
        }
    }

    fn do_interactive_mode(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.enter_settings(),
            KeyCode::Char(x) => self.send_buffer.push(x as u8),
            KeyCode::Backspace => {
                self.send_buffer.pop();
            }
            KeyCode::Enter => self.send_tx_buffer(),
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::F(2) {
            self.rotate_display_mode();
        }

        match self.mode {
            Mode::Settings => {
                self.do_settings_mode(key_event);
                return;
            }
            Mode::Interactive => self.do_interactive_mode(key_event),
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

    fn format_data(&self, data: &[u8]) -> String {
        match self.display_mode {
            DisplayMode::Hex => data.iter().map(|x| format!("{:02X} ", x)).collect(),
            DisplayMode::Ascii => data.iter().map(|x| format!("{}", (*x) as char)).collect(),
            DisplayMode::Decimal => data
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(" "),
            DisplayMode::MixedHex => {
                // all bytes, that are printable characters are printed as such, otherwise hex
                data.iter()
                    .map(|x| {
                        if (*x as char).is_ascii() {
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
                    if (*x as char).is_ascii() {
                        return format!("{}", (*x) as char);
                    }
                    format!("{}", *x)
                })
                .collect::<Vec<String>>()
                .join(" "),
        }
    }

    fn draw_rxtxbuffer(&mut self, area: Rect, buf: &mut Frame) {
        // copy state events to display history:

        if let Some(ref s) = self.state_receiver {
            while let Ok(x) = s.try_recv() {
                self.display_history.push(x);
            }
        }

        let items: Vec<Line> = self
            .display_history
            .iter()
            .rev()
            .take(10)
            .map(|x| {
                let result = match x {
                    SerialStateMessage::DataEvent(x) => {
                        //let bytes = x.data.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(" ");
                        let bytes = self.format_data(&x.data);

                        let ln = Line::from(vec![
                            x.rx_tx.to_string().fg(if x.rx_tx == RxTx::Tx {
                                ratatui::style::Color::Green
                            } else {
                                ratatui::style::Color::Red
                            }),
                            format!(": {}", bytes).fg(ratatui::style::Color::Gray),
                        ]);
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
        //buf.render_widget(block, area);
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
            "TX:".fg(ratatui::style::Color::LightGreen),
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
                println!(
                    "Port selected: {}",
                    port.file_name().unwrap().to_str().unwrap()
                );
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

    fn send_tx_buffer(&mut self) {
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

        self.send_command(SerialCommand::Send(self.send_buffer.clone()));
        self.send_buffer.clear();
    }

    fn port_background_thread(&self, rx: Receiver<SerialCommand>, tx: Sender<SerialStateMessage>) {
        thread::spawn(move || {
            let mut state = PortThreadState::Stopped;
            loop {
                let mut data_to_send: Vec<u8> = vec![];
                if let Ok(cmd) = rx.recv_timeout(Duration::from_micros(1)) {
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
}
