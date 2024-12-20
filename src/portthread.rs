use std::{
    fmt::Display, sync::mpsc::{Receiver, Sender}, thread, vec
};

use chrono::{DateTime, Local};

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

#[derive(Debug, Default, PartialEq, Clone)]
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

#[derive(Debug, PartialEq, Clone)]
pub struct HistoryEntry {
    pub timestamp: DateTime<Local>,
    pub rx_tx: RxTx,
    pub data: vec::Vec<u8>,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            timestamp: Local::now(),
            rx_tx: RxTx::Rx,
            data: vec![]
        }
    }
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
        let mut last_entry = HistoryEntry::default();

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
                    send_receive(ctx, &mut last_entry, data_to_send, &tx);
                }
            }
        }
    });
}


fn send_receive(ctx: &SerialContext, last_entry: &mut HistoryEntry, data_to_send: Vec<u8>, tx: &Sender<SerialStateMessage>) {    
    if let Some(p) = &ctx.com_port {
        if data_to_send.len() != 0 {
            if let Ok(_) = p.write(&data_to_send) {
                let entry = HistoryEntry {
                    timestamp: Local::now(),
                    rx_tx: RxTx::Tx,
                    data: data_to_send,
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
            /*
                How this works:
                Since we occasionally get several reads at the same timestamp,
                we aggregate everything we receive withinin a small number of
                milliseconds into one entry.
             */        
            handle_received_bytes(last_entry, buffer[0..data].to_vec(), tx);
        }
    }
}

fn handle_received_bytes(last_entry: &mut HistoryEntry, received_data: Vec<u8>, tx: &Sender<SerialStateMessage>) {

    let mut entry = HistoryEntry {
        timestamp: Local::now(),
        rx_tx: RxTx::Rx,
        data: received_data,
    };    
    let ms = (entry.timestamp - last_entry.timestamp).num_milliseconds();

    if ms < 5
    {
        last_entry.data.append(&mut entry.data);
    }
    else {
        tx.send(SerialStateMessage::DataEvent(last_entry.clone())).unwrap();    
        last_entry.data = entry.data;
        last_entry.timestamp = Local::now();                
    } 
}

// Tests        
#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use chrono::Days;

    use super::*;

    #[test]
    fn test_handle_received_bytes() {
        let mut last_entry = HistoryEntry::default();
        let (tx, _) = mpsc::channel();
        let received_data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        handle_received_bytes(&mut last_entry, received_data, &tx);
        // simple case, no aggregation of data:
        assert_eq!(last_entry.data, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    // Test aggregation of data in case of a short period of time:
    #[test]
    fn test_handle_received_bytes_aggregation() {

        let mut last_entry = HistoryEntry::default();

        let (tx, _) = mpsc::channel();

        let received_data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        last_entry.timestamp = Local::now();
        handle_received_bytes(&mut last_entry, received_data, &tx);
        let received_data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        handle_received_bytes(&mut last_entry, received_data, &tx);
        // simple case, no aggregation of data:
        assert_eq!(last_entry.data, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    // Test, will emit message, if time threshold is exceeded:
    #[test]
    fn test_handle_received_bytes_emits_message() {
        let mut last_entry = HistoryEntry::default();
        let (tx, rx) = mpsc::channel();
        let received_data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];        
        handle_received_bytes(&mut last_entry, received_data, &tx);
        let received_data = vec![0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        last_entry.timestamp = Local::now().checked_sub_days(Days::new(1)).unwrap();
        let _ = handle_received_bytes(&mut last_entry, received_data, &tx);
        // simple case, no aggregation of data:
        assert_eq!(last_entry.data, vec![0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        let recv = rx.recv().expect("Need message here!");
        if let SerialStateMessage::DataEvent(msg) = recv {
            assert_eq!(msg.data, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        }

    }

}