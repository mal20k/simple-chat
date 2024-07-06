use std::io::{stdout, Stdout};
use std::sync::Mutex;

use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    prelude::{Constraint, Layout},
    style::{Style, Stylize},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, Paragraph},
    Frame, Terminal,
};
use regex::Regex;
use tui_input::{backend::crossterm::EventHandler, Input};

use simple_chat::client::Connection;

static REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

type Tui = Terminal<CrosstermBackend<Stdout>>;

enum ClientCommand<A> {
    Connect(String, A),
    Send(String),
    Leave,
}

impl core::str::FromStr for ClientCommand<String> {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let input_re = REGEX.get_or_init(|| {
            Regex::new(
                r#"(?x)
                ^(leave)\s*$ |
                ^(connect)\s+(.*)@(.*)$ |
                ^(send)\s+(.*)
                "#,
            )
            .unwrap()
        });

        let captures = input_re.captures(s).map(|captures| {
            captures
                .iter()
                .skip(1)
                .flatten()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
        });

        let ret = match captures.as_deref() {
            Some(["leave"]) => Ok(Self::Leave),
            Some(["connect", nick, addr]) => Ok(Self::Connect(nick.to_string(), addr.to_string())),
            Some(["send", message]) => Ok(Self::Send(message.to_string())),
            _ => {
                anyhow::bail!("invalid command")
            }
        };

        ret
    }
}

#[derive(Debug, Default)]
struct Messages {
    messages: Mutex<Vec<(String, String)>>,
}

impl Messages {
    fn push(&mut self, nick: &str, msg: &str) {
        self.messages
            .lock()
            .unwrap()
            .push((nick.to_string(), msg.to_string()))
    }
}

#[derive(Debug, Default)]
pub struct App {
    input: Input,
    messages: Messages,
    connection: Option<Connection>,
    exit: bool,
}

impl App {
    fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| ui(frame, self))?;
            if event::poll(std::time::Duration::from_millis(16))? {
                self.handle_events()?;
            }
        }

        Ok(())
    }

    fn handle_events(&mut self) -> Result<()> {
        if let event::Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter => {
                    let input = self.input.value().to_string();
                    match input.parse::<ClientCommand<String>>() {
                        Ok(ClientCommand::Leave) => self.exit(),
                        Ok(ClientCommand::Connect(nick, addr)) => {
                            if self.connection.is_none() {
                                self.messages
                                    .push("INFO", &format!("Connecting to {addr} as {nick}"));
                                if let Ok(connection) = Connection::connect(&nick, addr) {
                                    self.connection = Some(connection);
                                } else {
                                    // TODO: Display the specific error
                                    self.messages.push("ERROR", "Failed to connect to server");
                                }
                            } else {
                                self.messages.push("ERROR", "Already connected to server");
                            }
                        }
                        Ok(ClientCommand::Send(message)) => {
                            if let Some(connection) = &self.connection {
                                self.messages.push(&connection.nick, &message);
                                connection.send(&message).unwrap();
                            } else {
                                self.messages.push("ERROR", "Not connected to a server");
                            }
                        }
                        _ => {
                            self.messages.push("ERROR", "Invalid command");
                        }
                    }
                    self.input.reset();
                }
                _ => {
                    self.input.handle_event(&event::Event::Key(key));
                }
            }
        }

        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

fn ui(f: &mut Frame, app: &App) {
    let vertical = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ]);
    let [message_area, input_area, help_area] = vertical.areas(f.size());

    let header = Text::from(Line::from(vec![
        "Type ".into(),
        "connect <nick>@<ip>:<port>".bold().green(),
        " to connect to a server, type ".into(),
        "leave".bold().red(),
        " to exit.".into(),
    ]))
    .patch_style(Style::default());
    let help_message = Paragraph::new(header);
    f.render_widget(help_message, help_area);

    let width = input_area.width.max(3) - 3;
    let scroll = app.input.visual_scroll(width as usize);
    let input_block = Block::bordered().title(" Input ").border_set(border::THICK);
    let input = Paragraph::new(app.input.value())
        .scroll((0, scroll as u16))
        .block(input_block);
    f.render_widget(input, input_area);

    f.set_cursor(
        input_area.x + ((app.input.visual_cursor()).max(scroll) - scroll) as u16 + 1,
        input_area.y + 1,
    );

    let message_block = Block::bordered()
        .title(" Messages ")
        .border_set(border::THICK);
    let messages: Vec<ListItem> = app
        .messages
        .messages
        .lock()
        .unwrap()
        .iter()
        .map(|(n, m)| {
            let content = vec![Line::from(Span::raw(format!("{n}: {m}")))];
            ListItem::new(content)
        })
        .collect();
    let messages = List::new(messages).block(message_block);
    f.render_widget(messages, message_area);
}

fn main() -> Result<()> {
    let mut terminal = init()?;
    terminal.clear()?;

    let mut app = App::default();
    let res = app.run(&mut terminal);

    restore()?;

    res
}

fn init() -> Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    Terminal::new(CrosstermBackend::new(stdout())).map_err(anyhow::Error::from)
}

fn restore() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
