use std::{
    fmt::Display,
    io,
    thread::{self},
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use mode::{ApplicationMode, Mode};
use portthread::{SerialCommand, SerialStateMessage};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    DefaultTerminal, Frame,
};

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

mod analyzer_mode;
mod interactive_mode;
mod mode;
mod portthread;
mod serialtypes;
mod settings_mode;

const DISPLAY_MODES: [DisplayMode; 5] = [
    DisplayMode::Decimal,
    DisplayMode::Hex,
    DisplayMode::Ascii,
    DisplayMode::MixedHex,
    DisplayMode::MixedDec,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Decimal,
    Hex,
    Ascii,
    MixedHex,
    MixedDec,
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

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let app_result = App::default().run(&mut terminal);
    ratatui::restore();
    app_result
}

#[derive(Debug)]
pub struct App {
    exit: bool,
    mode: Mode,
    command_sender: Sender<SerialCommand>,
    state_receiver: Receiver<SerialStateMessage>,
    settingsmode: settings_mode::SettingsMode,
    analyzermode: analyzer_mode::AnalyzerMode,
    interactivemode: interactive_mode::InteractiveMode,
}

impl Default for App {
    fn default() -> Self {
        let (stx, rtx): (Sender<SerialStateMessage>, Receiver<SerialStateMessage>) =
            mpsc::channel();
        let (tx, rx): (Sender<SerialCommand>, Receiver<SerialCommand>) = mpsc::channel();
        portthread::port_background_thread(rx, stx);

        let data = App {
            mode: Mode::Normal,
            exit: false,
            command_sender: tx.clone(),
            state_receiver: rtx,
            settingsmode: settings_mode::SettingsMode::new(),
            analyzermode: analyzer_mode::AnalyzerMode::new(),
            interactivemode: interactive_mode::InteractiveMode::new(tx),
        };

        data
    }
}

impl App {
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
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

    fn enable_mode(&mut self, mode: Mode) {
        self.settingsmode.set_active_inactive(false);
        self.analyzermode.set_active_inactive(false);
        self.interactivemode.set_active_inactive(false);

        match mode {
            Mode::Settings => self.settingsmode.set_active_inactive(true),
            Mode::Interactive => self.interactivemode.set_active_inactive(true),
            Mode::Analyzer => self.analyzermode.set_active_inactive(true),
            _ => {}
        }

        self.mode = mode;
    }

    fn do_settings_mode(&mut self, key_event: KeyEvent) {
        self.settingsmode.handle_key_event(key_event);
        match key_event.code {
            KeyCode::Up => self.analyzermode.scroll_up(),
            KeyCode::Down => self.analyzermode.scroll_down(),
            KeyCode::Enter => self.enter_interactive_mode(),
            _ => {}
        }
    }

    fn do_interactive_mode(&mut self, key_event: KeyEvent) {
        self.interactivemode.handle_key_event(key_event);
        match key_event.code {
            KeyCode::Up => self.analyzermode.scroll_up(),
            KeyCode::Down => self.analyzermode.scroll_down(),
            _ => {}
        }
    }

    fn do_analyzer_mode(&mut self, key_event: KeyEvent) {
        self.analyzermode.handle_key_event(key_event);
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
        if key_event.code == KeyCode::F(2) {
            self.settingsmode.rotate_display_mode();
        }

        if key_event.code == KeyCode::F(10) {
            self.analyzermode.clear_history();
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

    fn draw_settings(&self, area: Rect, buf: &mut Frame) {
        self.settingsmode.render(area, buf);
    }

    fn draw_rxtxbuffer(&mut self, area: Rect, buf: &mut Frame) {
        self.analyzermode
            .update_data(&self.state_receiver, self.settingsmode.get_display_mode());
        self.analyzermode.render(area, buf);
    }

    fn draw_tx_line(&self, area: Rect, buf: &mut Frame) {
        self.interactivemode.render(area, buf);
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
        self.draw_settings(chunks[0], frame);
        self.draw_rxtxbuffer(chunks[1], frame);
        self.draw_tx_line(chunks[2], frame);
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
        let ctx = self.settingsmode.create_serial_context();
        self.send_command(SerialCommand::Start(ctx));
        self.enable_mode(mode::Mode::Interactive);
    }

    /// Exits the current mode and enters the settings mode, which is a mode where the user can adjust
    /// the port, baud rate, stop bits, parity, and data bits of the serial connection.
    fn enter_settings_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.enable_mode(mode::Mode::Settings);
    }

    /// Enters the analyzer mode, which is a special interactive mode that renders the hexadecimal,
    /// signed and unsigned 8, 16, 32, and 64 bit, as well as floating point 32 and 64 bit values of the byte
    /// at the cursor position.
    fn enter_analyzer_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.enable_mode(mode::Mode::Analyzer);
    }

    fn enter_normal_mode(&mut self) {
        self.send_command(SerialCommand::Stop);
        self.enable_mode(mode::Mode::Normal);
    }

    /// Sends a serial command to the background thread via the command sender channel.
    ///
    /// # Arguments
    ///
    /// * `cmd` - A `SerialCommand` variant to be sent to the background thread. This could
    ///   be a command to start, stop, or send data through the serial port.
    fn send_command(&self, cmd: SerialCommand) {
        self.command_sender.send(cmd).unwrap();
    }
}
