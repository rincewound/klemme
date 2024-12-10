use std::{fmt::Display, sync::mpsc::Receiver};

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Clear, List, ListDirection},
    Frame,
};

use crate::{
    mode::ApplicationMode,
    portthread::{RxTx, SerialStateMessage},
    serialtypes::control_char_to_string,
    DisplayMode,
};

#[derive(Debug, Default, PartialEq)]
pub enum Endianness {
    Big,
    #[default]
    Little,
}
impl Display for Endianness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endianness::Big => write!(f, "Big"),
            Endianness::Little => write!(f, "Little"),
        }
    }
}

#[derive(Debug)]
pub struct AnalyzerMode {
    active: bool,
    display_history: Vec<SerialStateMessage>,
    scroll_offset: u32,
    analyzer_cursor_line: usize,
    analyzer_cursor_pos: usize,
    analyzer_endianness: Endianness,
    active_display_mode: DisplayMode,
}

impl AnalyzerMode {
    pub fn new() -> AnalyzerMode {
        AnalyzerMode {
            active: false,
            display_history: vec![],
            scroll_offset: 0,
            analyzer_cursor_line: 0,
            analyzer_cursor_pos: 0,
            analyzer_endianness: Endianness::Little,
            active_display_mode: DisplayMode::Hex,
        }
    }

    pub(crate) fn add_to_history(&mut self, arg: &str) {
        let msg = SerialStateMessage::ErrorEvent(arg.to_string());
        self.display_history.push(msg);
    }
}

impl ApplicationMode for AnalyzerMode {
    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Left => self.cursor_left(),
            KeyCode::Right => self.cursor_right(),
            KeyCode::Up => self.scroll_analyzer_cursor_up(),
            KeyCode::Down => self.scroll_analyzer_cursor_down(),
            KeyCode::PageUp => self.scroll_up(),
            KeyCode::PageDown => self.scroll_down(),
            KeyCode::Char('e') => self.rotate_analyzer_endianness(),
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
        //self.update_history_with_incoming_data();
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
}

impl AnalyzerMode {
    /// Scroll up in the display history, moving the top line down by one.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_analyzer_cursor_up(&mut self) {
        self.analyzer_cursor_line += 1;
    }

    pub fn scroll_analyzer_cursor_down(&mut self) {
        self.analyzer_cursor_line = self.analyzer_cursor_line.saturating_sub(1);
    }

    /// Scroll down in the display history, moving the top line up by one.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Move the analyzer cursor one character to the left. This will move the highlighted
    /// character in the analyzer window to the left by one position. If the cursor is
    /// already at the left edge of the window, this function has no effect.
    pub fn cursor_left(&mut self) {
        self.analyzer_cursor_pos = self.analyzer_cursor_pos.saturating_sub(1);
    }

    /// Move the analyzer cursor one character to the right. This will move the highlighted
    /// character in the analyzer window to the right by one position. If the cursor is
    /// already at the right edge of the window, this function has no effect.
    pub fn cursor_right(&mut self) {
        self.analyzer_cursor_pos += 1;
    }

    pub fn rotate_analyzer_endianness(&mut self) {
        if self.analyzer_endianness == Endianness::Big {
            self.analyzer_endianness = Endianness::Little
        } else {
            self.analyzer_endianness = Endianness::Big
        }
    }

    pub fn update_data(
        &mut self,
        data_source: &Receiver<SerialStateMessage>,
        display_mode: DisplayMode,
    ) {
        self.update_history_with_incoming_data(data_source);
        self.active_display_mode = display_mode;
    }

    pub fn clear_history(&mut self) {
        self.display_history.clear();
        self.display_history.shrink_to_fit();
    }

    fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Start);
        let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::End);
        let [area] = vertical.areas(area);
        let [area] = horizontal.areas(area);
        area
    }

    fn update_history_with_incoming_data(&mut self, data_source: &Receiver<SerialStateMessage>) {
        // copy state events to display history:
        while let Ok(x) = data_source.try_recv() {
            self.display_history.push(x);
        }
    }

    fn format_data_for_display(&self, data: &[u8]) -> String {
        match self.active_display_mode {
            DisplayMode::Hex => data.iter().map(|x| format!("{:02X} ", x)).collect(),
            DisplayMode::Ascii => {
                // replace control bytes by their name:
                let data = data
                    .iter()
                    .map(|x| {
                        if !(*x as char).is_control() {
                            return format!("{}", (*x) as char);
                        }
                        let chr = control_char_to_string(*x);
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

    fn build_list_items(&self, analyzer_data: &mut Vec<u8>, max_num_rows: usize) -> Vec<Line<'_>> {
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
                        let bytes = self.format_data_for_display(&x.data);

                        let mut pre_cursor = String::from(bytes.clone());
                        let mut cursor = String::from("");
                        let mut post_cursor = String::from("");
                        let mut highlight_string = String::from("");
                        let mut cursor_color = ratatui::style::Color::Black;
                        let mut post_cursor_color = ratatui::style::Color::Black;

                        if self.active_display_mode == DisplayMode::Hex
                            && line_index == self.analyzer_cursor_line
                            && self.active
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

    /// Renders the analyzer window. This window will appear if the display mode is hex
    /// and the mode is analyzer. The window will contain the byte, u16, i16, u32, i32, f32, u64, i64, and f64
    /// values of the byte at the cursor position.
    fn render_analyzer(&self, area: Rect, buf: &mut Frame<'_>, analyzer_data: Vec<u8>) {
        if !self.active {
            return;
        }

        if self.active_display_mode != DisplayMode::Hex {
            return;
        }

        if self.analyzer_cursor_pos >= analyzer_data.len() {
            return;
        }

        let mut items: Vec<String> = vec![];
        // Use the cursor position to obtain the analyzer data: 1 byte, 2 byte, 4 bytes
        let one_byte = analyzer_data[self.analyzer_cursor_pos];
        items.push(format!("binary (MSB first): {:08b}", one_byte));
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
}
