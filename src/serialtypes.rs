pub const BAUD_RATES: [u32; 8] = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

pub const STOP_BITS: [u8; 3] = [1, 2, 3];

pub const PARITY: [&str; 3] = ["None", "Odd", "Even"];

pub const DATABITS: [u8; 5] = [5, 6, 7, 8, 9];

/// Convert a control character to a string representation.
    ///
    /// This function takes a byte containing a control character and returns a string
    /// representation of that character. The string representation is of the form `<X>`,
    /// where `X` is the name of the control character. For example, a byte with the value 0x00
    /// would return the string `"<NUL>"`.
    pub fn control_char_to_string(c: u8) -> String {
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