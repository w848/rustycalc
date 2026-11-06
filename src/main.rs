use std::io::{self, stdout};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use calc::{Action, App, Op};

mod calc {
    /// Four-function calculator. Each operation runs when the next one is
    /// entered, so `2 + 3 × 4` is 20.
    #[derive(Debug)]
    pub struct App {
        value: String,
        stored: Option<f64>,
        pending: Option<Op>,
        last_op: Option<Op>,
        last_right: Option<f64>,
        /// The next digit replaces the display instead of appending.
        fresh: bool,
        /// The display is a real right-hand entry, so the pending operation
        /// should run before a new operator replaces it.
        apply_on_op: bool,
        message: Option<&'static str>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Op {
        Add,
        Sub,
        Mul,
        Div,
    }

    impl Op {
        pub fn glyph(self) -> &'static str {
            match self {
                Op::Add => "+",
                Op::Sub => "−",
                Op::Mul => "×",
                Op::Div => "÷",
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        Digit(char),
        Dot,
        Op(Op),
        Equals,
        Clear,
        Backspace,
        Percent,
        Negate,
    }

    impl App {
        pub fn new() -> Self {
            Self {
                value: "0".to_string(),
                stored: None,
                pending: None,
                last_op: None,
                last_right: None,
                fresh: true,
                apply_on_op: false,
                message: None,
            }
        }

        pub fn expression(&self) -> String {
            if let Some(message) = self.message {
                return message.to_string();
            }
            let (Some(left), Some(op)) = (self.stored, self.pending) else {
                return String::new();
            };
            if self.fresh && !self.apply_on_op {
                format!("{} {}", pretty(&format_num(left)), op.glyph())
            } else {
                format!(
                    "{} {} {}",
                    pretty(&format_num(left)),
                    op.glyph(),
                    pretty(&self.value)
                )
            }
        }

        pub fn is_error(&self) -> bool {
            self.message.is_some()
        }

        /// Number shown on the display, with thousands separators.
        pub fn pretty_display(&self) -> String {
            if self.message.is_some() {
                self.value.clone()
            } else {
                pretty(&self.value)
            }
        }

        pub fn press(&mut self, action: Action) {
            match action {
                Action::Digit(digit) => self.push_digit(digit),
                Action::Dot => self.push_dot(),
                Action::Op(op) => self.push_op(op),
                Action::Equals => self.equals(),
                Action::Clear => self.clear(),
                Action::Backspace => self.backspace(),
                Action::Percent => self.percent(),
                Action::Negate => self.negate(),
            }
        }

        fn clear(&mut self) {
            *self = Self::new();
        }

        fn push_digit(&mut self, digit: char) {
            if self.message.is_some() {
                self.clear();
            }
            if self.fresh {
                self.value.clear();
                self.fresh = false;
                self.last_op = None;
                self.last_right = None;
            }
            if self.value == "0" {
                self.value.clear();
            } else if self.value == "-0" {
                self.value = "-".to_string();
            }
            if self.value.len() >= 18 {
                return;
            }
            self.value.push(digit);
            self.apply_on_op = true;
        }

        fn push_dot(&mut self) {
            if self.message.is_some() {
                self.clear();
            }
            if self.fresh {
                self.value = "0".to_string();
                self.fresh = false;
                self.last_op = None;
                self.last_right = None;
            }
            if !self.value.contains('.') && self.value.len() < 18 {
                self.value.push('.');
            }
            self.apply_on_op = true;
        }

        fn push_op(&mut self, op: Op) {
            if self.message.is_some() {
                return;
            }
            if self.pending.is_some() && self.apply_on_op {
                let right = self.parse();
                let left = self.stored.unwrap_or(right);
                let pending = self.pending.unwrap();
                if !self.finish(left, right, pending) {
                    return;
                }
            } else if self.pending.is_none() {
                self.stored = Some(self.parse());
            }
            self.pending = Some(op);
            self.fresh = true;
            self.apply_on_op = false;
            self.last_op = None;
            self.last_right = None;
        }

        fn equals(&mut self) {
            if self.message.is_some() {
                return;
            }
            if let Some(op) = self.pending {
                let right = self.parse();
                let left = self.stored.unwrap_or(right);
                self.last_op = Some(op);
                self.last_right = Some(right);
                if !self.finish(left, right, op) {
                    return;
                }
                self.pending = None;
            } else if let (Some(op), Some(right)) = (self.last_op, self.last_right) {
                let left = self.parse();
                if !self.finish(left, right, op) {
                    return;
                }
            }
            self.fresh = true;
            self.apply_on_op = false;
        }

        fn backspace(&mut self) {
            if self.message.is_some() {
                self.clear();
                return;
            }
            if self.fresh {
                return;
            }
            self.value.pop();
            if self.value.is_empty() || self.value == "-" {
                self.value = "0".to_string();
            }
        }

        fn percent(&mut self) {
            if self.message.is_some() {
                return;
            }
            let current = self.parse();
            let result = if self.pending.is_some() && self.apply_on_op {
                self.stored.unwrap_or(current) * current / 100.0
            } else {
                current / 100.0
            };
            if !result.is_finite() {
                self.fail("Overflow");
                return;
            }
            self.value = format_num(result);
            self.fresh = true;
            self.apply_on_op = true;
        }

        fn negate(&mut self) {
            if self.message.is_some() {
                return;
            }
            let number = self.parse();
            if number == 0.0 {
                return;
            }
            let number = -number;
            self.value = format_num(number);
            if self.fresh && !self.apply_on_op {
                self.stored = Some(number);
            }
        }

        fn finish(&mut self, left: f64, right: f64, op: Op) -> bool {
            match compute(left, right, op) {
                Ok(number) => {
                    self.value = format_num(number);
                    self.stored = Some(number);
                    self.message = None;
                    true
                }
                Err(message) => {
                    self.fail(message);
                    false
                }
            }
        }

        fn fail(&mut self, message: &'static str) {
            self.value = "Error".to_string();
            self.stored = None;
            self.pending = None;
            self.last_op = None;
            self.last_right = None;
            self.fresh = true;
            self.apply_on_op = false;
            self.message = Some(message);
        }

        fn parse(&self) -> f64 {
            self.value.parse().unwrap_or(0.0)
        }
    }

    fn compute(left: f64, right: f64, op: Op) -> Result<f64, &'static str> {
        let result = match op {
            Op::Add => left + right,
            Op::Sub => left - right,
            Op::Mul => left * right,
            Op::Div if right == 0.0 => return Err("Can't divide by zero"),
            Op::Div => left / right,
        };
        if result.is_finite() {
            Ok(result)
        } else {
            Err("Overflow")
        }
    }

    fn format_num(number: f64) -> String {
        if !number.is_finite() {
            return "Error".to_string();
        }
        if number == 0.0 {
            return "0".to_string();
        }
        let absolute = number.abs();
        if absolute >= 1e12 || absolute < 1e-9 {
            return format!("{number:.6e}");
        }
        let text = format!("{number:.10}");
        let text = text.trim_end_matches('0').trim_end_matches('.').to_string();
        if text == "-0" { "0".to_string() } else { text }
    }

    pub fn pretty(raw: &str) -> String {
        let (sign, rest) = match raw.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", raw),
        };
        let (int, frac) = match rest.split_once('.') {
            Some((int, frac)) => (int, Some(frac)),
            None => (rest, None),
        };
        if int.is_empty() || !int.bytes().all(|byte| byte.is_ascii_digit()) {
            return raw.to_string();
        }
        let mut grouped = String::new();
        for (index, ch) in int.chars().rev().enumerate() {
            if index > 0 && index % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        let grouped: String = grouped.chars().rev().collect();
        match frac {
            Some(frac) => format!("{sign}{grouped}.{frac}"),
            None => format!("{sign}{grouped}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn press(keys: &str) -> App {
            let mut app = App::new();
            for ch in keys.chars() {
                let action = match ch {
                    '0'..='9' => Action::Digit(ch),
                    '.' => Action::Dot,
                    '+' => Action::Op(Op::Add),
                    '-' => Action::Op(Op::Sub),
                    '*' => Action::Op(Op::Mul),
                    '/' => Action::Op(Op::Div),
                    '=' => Action::Equals,
                    'c' => Action::Clear,
                    'b' => Action::Backspace,
                    '%' => Action::Percent,
                    'n' => Action::Negate,
                    _ => panic!("unknown test key {ch}"),
                };
                app.press(action);
            }
            app
        }

        #[test]
        fn adds_and_chains_left_to_right() {
            assert_eq!(press("2+3=").value, "5");
            assert_eq!(press("2+3*4=").value, "20");
            assert_eq!(press("2+3+").value, "5");
            assert_eq!(press("2+3+").expression(), "5 +");
        }

        #[test]
        fn repeats_the_last_operation() {
            assert_eq!(press("5+2==").value, "9");
        }

        #[test]
        fn divides_and_rejects_zero() {
            assert_eq!(press("8/2=").value, "4");
            assert_eq!(press("1/0=").value, "Error");
            assert_eq!(press("1/0=").expression(), "Can't divide by zero");
            assert_eq!(press("1/0=7").value, "7");
        }

        #[test]
        fn keeps_tenths_stable() {
            assert_eq!(press("0.1+0.2=").value, "0.3");
            assert_eq!(press(".5+.5=").value, "1");
        }

        #[test]
        fn percent_negate_backspace_and_clear() {
            assert_eq!(press("50%").value, "0.5");
            assert_eq!(press("200+10%=").value, "220");
            assert_eq!(press("12n").value, "-12");
            assert_eq!(press("12b").value, "1");
            assert_eq!(press("12+b3=").value, "15");
            assert_eq!(press("99c").value, "0");
        }

        #[test]
        fn continues_from_a_result() {
            let mut app = press("3*3=");
            app.press(Action::Op(Op::Add));
            app.press(Action::Digit('1'));
            app.press(Action::Equals);
            assert_eq!(app.value, "10");
        }

        #[test]
        fn groups_thousands() {
            assert_eq!(pretty("1000000"), "1,000,000");
            assert_eq!(pretty("-1234.5"), "-1,234.5");
            assert_eq!(pretty("0."), "0.");
        }
    }
}

const KEY_COUNT: usize = 20;

struct Key {
    label: &'static str,
    action: Action,
    tone: Tone,
}

#[derive(Clone, Copy)]
enum Tone {
    Number,
    Operator,
    Equals,
    Clear,
    Function,
}

const KEYS: [Key; KEY_COUNT] = [
    Key {
        label: "C",
        action: Action::Clear,
        tone: Tone::Clear,
    },
    Key {
        label: "⌫",
        action: Action::Backspace,
        tone: Tone::Function,
    },
    Key {
        label: "%",
        action: Action::Percent,
        tone: Tone::Function,
    },
    Key {
        label: "÷",
        action: Action::Op(Op::Div),
        tone: Tone::Operator,
    },
    Key {
        label: "7",
        action: Action::Digit('7'),
        tone: Tone::Number,
    },
    Key {
        label: "8",
        action: Action::Digit('8'),
        tone: Tone::Number,
    },
    Key {
        label: "9",
        action: Action::Digit('9'),
        tone: Tone::Number,
    },
    Key {
        label: "×",
        action: Action::Op(Op::Mul),
        tone: Tone::Operator,
    },
    Key {
        label: "4",
        action: Action::Digit('4'),
        tone: Tone::Number,
    },
    Key {
        label: "5",
        action: Action::Digit('5'),
        tone: Tone::Number,
    },
    Key {
        label: "6",
        action: Action::Digit('6'),
        tone: Tone::Number,
    },
    Key {
        label: "−",
        action: Action::Op(Op::Sub),
        tone: Tone::Operator,
    },
    Key {
        label: "1",
        action: Action::Digit('1'),
        tone: Tone::Number,
    },
    Key {
        label: "2",
        action: Action::Digit('2'),
        tone: Tone::Number,
    },
    Key {
        label: "3",
        action: Action::Digit('3'),
        tone: Tone::Number,
    },
    Key {
        label: "+",
        action: Action::Op(Op::Add),
        tone: Tone::Operator,
    },
    Key {
        label: "±",
        action: Action::Negate,
        tone: Tone::Function,
    },
    Key {
        label: "0",
        action: Action::Digit('0'),
        tone: Tone::Number,
    },
    Key {
        label: ".",
        action: Action::Dot,
        tone: Tone::Number,
    },
    Key {
        label: "=",
        action: Action::Equals,
        tone: Tone::Equals,
    },
];

#[derive(Default)]
struct Pointer {
    hot: Option<usize>,
    down: Option<usize>,
}

// Catppuccin Mocha, matching ~/.config/yazi/flavors/catppuccin-mocha.yazi.
const APP_BG: Color = Color::Rgb(30, 30, 46);
const SHADOW: Color = Color::Rgb(17, 17, 27);
const CARD_BG: Color = Color::Rgb(49, 50, 68);
const CARD_BORDER: Color = Color::Rgb(127, 132, 156);
const CREAM: Color = Color::Rgb(205, 214, 244);
const BLUE: Color = Color::Rgb(137, 180, 250);
const HOVER: Color = Color::Rgb(245, 194, 231);
const STONE: Color = Color::Rgb(147, 153, 178);
const DISPLAY_BG: Color = Color::Rgb(24, 24, 37);
const DISPLAY_BORDER: Color = Color::Rgb(137, 180, 250);
const ERROR_FG: Color = Color::Rgb(243, 139, 168);
const BASE: Color = Color::Rgb(30, 30, 46);

fn main() -> io::Result<()> {
    ratatui::run(|terminal| {
        let _mouse = MouseGuard::enable()?;
        run(terminal)
    })
}

struct MouseGuard;

impl MouseGuard {
    fn enable() -> io::Result<Self> {
        execute!(stdout(), EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for MouseGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableMouseCapture);
    }
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
    let mut pointer = Pointer::default();
    let mut buttons = [Rect::default(); KEY_COUNT];

    loop {
        terminal.draw(|frame| {
            buttons = render(frame, &app, &pointer);
        })?;

        match event_read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match map_key(key) {
                Command::Quit => break,
                Command::Press(action) => app.press(action),
                Command::None => {}
            },
            Event::Mouse(mouse) => handle_mouse(&mut app, &mut pointer, &buttons, mouse),
            _ => {}
        }
    }
    Ok(())
}

fn event_read() -> io::Result<Event> {
    crossterm::event::read()
}

enum Command {
    Quit,
    Press(Action),
    None,
}

fn map_key(key: KeyEvent) -> Command {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c' | 'q' | 'C' | 'Q'))
    {
        return Command::Quit;
    }
    let action = match key.code {
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => return Command::Quit,
        KeyCode::Char(digit @ '0'..='9') => Action::Digit(digit),
        KeyCode::Char('.') => Action::Dot,
        KeyCode::Char('+') => Action::Op(Op::Add),
        KeyCode::Char('-' | '−') => Action::Op(Op::Sub),
        KeyCode::Char('*' | 'x' | 'X' | '×') => Action::Op(Op::Mul),
        KeyCode::Char('/' | '÷') => Action::Op(Op::Div),
        KeyCode::Char('=') | KeyCode::Enter => Action::Equals,
        KeyCode::Char('%') => Action::Percent,
        KeyCode::Char('n' | 'N') => Action::Negate,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete | KeyCode::Char('c' | 'C') => Action::Clear,
        _ => return Command::None,
    };
    Command::Press(action)
}

fn handle_mouse(
    app: &mut App,
    pointer: &mut Pointer,
    buttons: &[Rect; KEY_COUNT],
    mouse: MouseEvent,
) {
    let hit = button_at(buttons, mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Moved | MouseEventKind::Drag(_) => pointer.hot = hit,
        MouseEventKind::Down(MouseButton::Left) => {
            pointer.hot = hit;
            pointer.down = hit;
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let (Some(index), Some(under)) = (pointer.down, hit)
                && index == under
            {
                app.press(KEYS[index].action);
            }
            pointer.down = None;
            pointer.hot = hit;
        }
        _ => {}
    }
}

fn button_at(buttons: &[Rect; KEY_COUNT], column: u16, row: u16) -> Option<usize> {
    let position = Position::new(column, row);
    buttons.iter().position(|area| area.contains(position))
}

fn render(frame: &mut Frame, app: &App, pointer: &Pointer) -> [Rect; KEY_COUNT] {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::new().bg(APP_BG)), area);

    let Some(card) = card_rect(area) else {
        let message = Paragraph::new("Make the terminal at least 34 columns by 22 rows")
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(CREAM).bg(APP_BG));
        frame.render_widget(message, area);
        return [Rect::default(); KEY_COUNT];
    };

    if let Some(shadow) = shifted(card, area, 1, 1) {
        frame.render_widget(Block::default().style(Style::new().bg(SHADOW)), shadow);
    }

    let card_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CARD_BORDER))
        .style(Style::new().bg(CARD_BG).fg(CREAM))
        .title(Line::from(vec![
            Span::styled(" rusty", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
            Span::styled("calc ", Style::new().fg(CREAM)),
        ]))
        .title_bottom(
            Line::from(" type or click · q quits ")
                .style(Style::new().fg(STONE))
                .centered(),
        );
    let inner = card_block.inner(card);
    frame.render_widget(card_block, card);

    let content = inner.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    let content = if content.width >= 28 && content.height >= 18 {
        content
    } else {
        inner
    };

    let display_height = (content.height.saturating_sub(15)).clamp(3, 7);
    let [display_area, button_area] =
        Layout::vertical([Constraint::Length(display_height), Constraint::Min(0)]).areas(content);

    render_display(frame, app, display_area);
    let buttons = button_grid(button_area);
    for (index, key) in KEYS.iter().enumerate() {
        let hot = pointer.hot == Some(index);
        let pressed = pointer.down == Some(index) && hot;
        render_key(frame, buttons[index], key, hot, pressed);
    }
    buttons
}

fn render_display(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width.saturating_sub(3) as usize;
    let expression = fit(&app.expression(), width);
    let value = fit(&app.pretty_display(), width);
    let inner_rows = area.height.saturating_sub(2);
    let lines = display_lines(&expression, &value, inner_rows, app.is_error());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DISPLAY_BORDER))
        .style(Style::new().bg(DISPLAY_BG));
    let paragraph = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Right)
        .block(block);
    frame.render_widget(paragraph, area);
}

fn display_lines(expression: &str, value: &str, rows: u16, error: bool) -> Vec<Line<'static>> {
    let value_style = if error {
        Style::new()
            .fg(ERROR_FG)
            .bg(DISPLAY_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(CREAM)
            .bg(DISPLAY_BG)
            .add_modifier(Modifier::BOLD)
    };
    let expression_style = if error {
        Style::new().fg(ERROR_FG).bg(DISPLAY_BG)
    } else {
        Style::new().fg(STONE).bg(DISPLAY_BG)
    };
    let blank = Style::new().bg(DISPLAY_BG);
    let rows = rows.max(1) as usize;
    if rows == 1 {
        return vec![Line::styled(format!("{value} "), value_style)];
    }
    let mut lines = Vec::with_capacity(rows);
    lines.push(Line::styled(format!("{expression} "), expression_style));
    for _ in 0..rows - 2 {
        lines.push(Line::styled("", blank));
    }
    lines.push(Line::styled(format!("{value} "), value_style));
    lines
}

fn render_key(frame: &mut Frame, area: Rect, key: &Key, hot: bool, pressed: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (fg, mut bg, mut border) = tone_colors(key.tone);
    if pressed {
        bg = lighten(bg);
        border = CREAM;
    } else if hot {
        border = HOVER;
    }
    let label_style = Style::new().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
    if area.height >= 3 && area.width >= 3 {
        let inner_rows = area.height.saturating_sub(2);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border).bg(bg))
            .style(Style::new().bg(bg).fg(fg));
        let paragraph = Paragraph::new(centered_label(key.label, inner_rows, label_style))
            .alignment(ratatui::layout::Alignment::Center)
            .block(block);
        frame.render_widget(paragraph, area);
    } else {
        let paragraph = Paragraph::new(key.label)
            .alignment(ratatui::layout::Alignment::Center)
            .style(label_style);
        frame.render_widget(paragraph, area);
    }
}

fn centered_label(label: &str, rows: u16, style: Style) -> Vec<Line<'static>> {
    let rows = rows.max(1) as usize;
    let top = (rows - 1) / 2;
    let mut lines = Vec::with_capacity(rows);
    for index in 0..rows {
        if index == top {
            lines.push(Line::styled(label.to_string(), style));
        } else {
            lines.push(Line::styled("", style));
        }
    }
    lines
}

fn tone_colors(tone: Tone) -> (Color, Color, Color) {
    match tone {
        Tone::Number => (CREAM, Color::Rgb(69, 71, 90), Color::Rgb(108, 112, 134)),
        Tone::Operator => (BASE, BLUE, Color::Rgb(180, 190, 254)),
        Tone::Equals => (BASE, Color::Rgb(166, 227, 161), Color::Rgb(148, 226, 213)),
        Tone::Clear => (BASE, ERROR_FG, Color::Rgb(235, 160, 172)),
        Tone::Function => (CREAM, Color::Rgb(88, 91, 112), CARD_BORDER),
    }
}

fn lighten(color: Color) -> Color {
    match color {
        Color::Rgb(red, green, blue) => Color::Rgb(
            red.saturating_add(28),
            green.saturating_add(28),
            blue.saturating_add(28),
        ),
        other => other,
    }
}

fn card_rect(area: Rect) -> Option<Rect> {
    if area.width < 34 || area.height < 22 {
        return None;
    }
    let width = area.width.min(48);
    let height = area.height.min(32);
    Some(Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    })
}

fn button_grid(area: Rect) -> [Rect; KEY_COUNT] {
    let mut rects = [Rect::default(); KEY_COUNT];
    let cols = 4u16;
    let rows = 5u16;
    if area.width < cols * 3 || area.height < rows {
        return rects;
    }
    let gap_x = u16::from(area.width >= cols * 7 + 3);
    let gap_y = u16::from(area.height >= rows * 3 + 4);
    let width = (area.width - gap_x * (cols - 1)) / cols;
    let height = (area.height - gap_y * (rows - 1)) / rows;
    if width == 0 || height == 0 {
        return rects;
    }
    let used_width = width * cols + gap_x * (cols - 1);
    let used_height = height * rows + gap_y * (rows - 1);
    let origin_x = area.x + (area.width - used_width) / 2;
    let origin_y = area.y + (area.height - used_height) / 2;
    for row in 0..rows {
        for col in 0..cols {
            let index = (row * cols + col) as usize;
            rects[index] = Rect {
                x: origin_x + col * (width + gap_x),
                y: origin_y + row * (height + gap_y),
                width,
                height,
            };
        }
    }
    rects
}

fn shifted(area: Rect, bounds: Rect, dx: u16, dy: u16) -> Option<Rect> {
    let x = area.x.saturating_add(dx);
    let y = area.y.saturating_add(dy);
    let right = bounds.x.saturating_add(bounds.width);
    let bottom = bounds.y.saturating_add(bounds.height);
    if x >= right || y >= bottom {
        return None;
    }
    let width = area.width.min(right - x);
    let height = area.height.min(bottom - y);
    (width > 0 && height > 0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if width == 0 || count <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let skip = count - (width - 1);
    let mut fitted = String::from("…");
    fitted.extend(text.chars().skip(skip));
    fitted
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn draws_a_keypad() {
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let app = App::new();
        let pointer = Pointer::default();
        let mut buttons = [Rect::default(); KEY_COUNT];
        terminal
            .draw(|frame| {
                buttons = render(frame, &app, &pointer);
            })
            .unwrap();

        let view = buffer_text(terminal.backend().buffer());
        assert!(view.contains("rusty"), "{view}");
        assert!(view.contains('='), "{view}");
        assert!(view.contains('÷'), "{view}");
        assert!(
            buttons
                .iter()
                .all(|area| area.width >= 3 && area.height >= 3)
        );
        assert!(
            buttons
                .windows(2)
                .all(|pair| { pair[0].y != pair[1].y || pair[0].x + pair[0].width <= pair[1].x })
        );
    }

    #[test]
    fn asks_for_space_when_the_terminal_is_tiny() {
        let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();
        let app = App::new();
        let pointer = Pointer::default();
        let mut buttons = [Rect::default(); KEY_COUNT];
        terminal
            .draw(|frame| {
                buttons = render(frame, &app, &pointer);
            })
            .unwrap();
        let view = buffer_text(terminal.backend().buffer());
        assert!(view.contains("34 columns"), "{view}");
        assert!(buttons.iter().all(|area| area.width == 0));
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }
}
