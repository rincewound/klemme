use std::{
    fmt::Display,
    sync::mpsc::{Receiver, Sender},
    thread, vec,
};

pub enum PortError {
    BadSettings,
    FailedToFlush,
    FailedToOpen,
}

#[derive(Debug)]
pub struct SerialContext {
    port_name: String,
    com_port: Option<serial2::SerialPort>,
}

impl SerialContext {
    pub fn new(port_name: String, com_port: serial2::SerialPort) -> Self {
        SerialContext {
            port_name,
            com_port: Some(com_port),
        }
    }
}

impl PartialEq for SerialContext {
    fn eq(&self, other: &Self) -> bool {
        self.port_name == other.port_name
    }
}

#[derive(Debug, Default, PartialEq)]
pub enum RxTx {
    #[default]
    Rx,
    Tx,
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
pub struct HistoryEntry {
    pub rx_tx: RxTx,
    pub data: vec::Vec<u8>,
}

#[derive(Debug)]
pub enum PortThreadState {
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

pub enum SerialCommand {
    Stop,
    Start(SerialContext),
    Send(Vec<u8>),
}

#[derive(Debug)]
pub enum SerialStateMessage {
    DataEvent(HistoryEntry),
    ErrorEvent(String),
    Started,
    Stopped,
}

/// Returns the next command from the main thread, or `None` if there are no commands
/// to process.
///
/// # Behavior
///
/// If the serial port is stopped, this function will block until a command is received.
/// If the serial port is running, this function will non-blockingly return the next command
/// if there is one, or `None` if there are no commands to process.
fn receive_command(state: &PortThreadState, rx: &Receiver<SerialCommand>) -> Option<SerialCommand> {
    if *state == PortThreadState::Stopped {
        if let Ok(rxd) = rx.recv() {
            return Some(rxd);
        }
        return None;
    } else {
        if let Ok(rxd) = rx.try_recv() {
            return Some(rxd);
        }
        return None;
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
pub fn port_background_thread(rx: Receiver<SerialCommand>, tx: Sender<SerialStateMessage>) {
    thread::spawn(move || {
        let mut state = PortThreadState::Stopped;
        loop {
            let mut data_to_send: Vec<u8> = vec![];

            // if the state is stopped, wait until rx receives something:
            let _cmd = receive_command(&state, &rx);

            if let Some(cmd) = _cmd {
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
                    send_receive(ctx, data_to_send, &tx);
                }
            }
        }
    });
}

fn send_receive(ctx: &SerialContext, mut data_to_send: Vec<u8>, tx: &Sender<SerialStateMessage>) {
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
