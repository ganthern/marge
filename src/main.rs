use std::{fmt::format, io::Stdout, process::Termination};

use anyhow::anyhow;
use clap::Parser;
pub mod events;
mod git;
pub mod merge_candidate;
use git::{ActivePane, AppState};
use log::*;

use crate::{
    events::{AppEvent, EventPump},
    git::Marge,
};
use crossterm::event::{KeyCode, KeyEvent};
use merge_candidate::{MergeCandidate, MergeCandidateNew};
use tui_logger::{TuiLoggerWidget, TuiWidgetEvent, TuiWidgetState};

use ratatui::{backend::Backend, prelude::*, terminal::CompletedFrame, widgets::*, *};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about)]
#[command(
    help_template = "{about-section} \n {usage-heading} \n\t {usage} \n\n {all-args} \n\n {name} v{version} ({author})"
)]
/// marge helps you merge your PRs
///
/// will get the PRs for the current git repositories' github page,
/// then ask for a desired order to merge them in. after that, each branch will in turn be
///
/// * checked out
///
/// * rebased onto its predecessor
///
/// * validated with the command passed to marge
///
/// * force-pushed to github
///
/// if any step fails, marge will pause and notify so you can fix your stuff
/// before telling her to continue.
pub struct AppArgs {
    #[arg(long, short, default_value = "main")]
    /// the branch to rebase the PR chain onto
    branch: String,
    #[arg(long, short, default_value = ".token")]
    /// file to read the github API token from
    token: String,
    #[arg(long, short, default_value = "origin")]
    /// name of the remote to pull the PRs from. not required to be overridden if there's only
    /// one remote not named origin
    remote: String,
    #[arg(default_value = "true")]
    /// the sh command line marge should run to validate each rebased branch
    cmd: String,
}

#[derive(Debug)]
pub struct AppConfig {
    args: AppArgs,
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<Screen> {
    let mut marge = Marge::try_init().await?;
    let mut screen: Screen = Screen::try_new()?;
    let mut event_pump = EventPump::new(tokio::time::Duration::from_millis(150));

    let pulls = marge.get_pulls().await?;
    let candidates = pulls
        .into_iter()
        .map(|p| MergeCandidate::<MergeCandidateNew>::new(p));

    for candidate in candidates.into_iter() {
        info!("{:?}", candidate.pull.title)
    }

    loop {
        marge.last_event = if let Some(e) = event_pump.next().await {
            e
        } else {
            break;
        };

        marge.try_transition().await?;

        if let AppEvent::Error(e) = marge.last_event {
            info!("recvd error: {:#?}", e);
            return Err(anyhow!(e));
        }

        if let AppEvent::Signal = marge.last_event {
            break;
        }

        screen.draw(|f| draw_frame(f, &mut marge))?;
    }
    Ok(screen)
}

fn draw_frame<B: Backend>(t: &mut Frame<B>, marge: &mut Marge) -> () {
    let size = t.size();

    let main_block = Block::default().borders(Borders::NONE);
    let main_area = main_block.inner(size);
    t.render_widget(main_block, size);

    let constraints = vec![
        Constraint::Length(3), // title line
        Constraint::Min(10),   // content
    ];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(main_area);

    render_title(t, marge, chunks[0]);
    render_content(t, marge, chunks[1]);
}

fn render_title<B: Backend>(t: &mut Frame<B>, marge: &mut Marge, rect: Rect) -> () {
    let title_block = Block::default().borders(Borders::ALL);
    let title_area = title_block.inner(rect);

    let title = Paragraph::new(format!(
        "Merging {}/{} ({}) into {}",
        marge.remote.owner, marge.remote.repo, marge.remote.name, marge.branch
    ));
    t.render_widget(title, title_area);
    t.render_widget(title_block, rect);
}

fn render_content<B: Backend>(t: &mut Frame<B>, marge: &mut Marge, rect: Rect) -> () {
    let constraints = vec![
        Constraint::Percentage(50), // lists
        Constraint::Percentage(50), // log
    ];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(rect);

    if let AppEvent::Input(KeyEvent {
        code: KeyCode::Left,
        ..
    })
    | AppEvent::Input(KeyEvent {
        code: KeyCode::Right,
        ..
    }) = marge.last_event
    {
        marge.active_pane = if marge.active_pane == ActivePane::List {
            ActivePane::Log
        } else {
            ActivePane::List
        }
    }

    render_app(t, marge, chunks[0]);
    render_log(t, marge, chunks[1]);
}

fn render_app<B: Backend>(t: &mut Frame<B>, marge: &mut Marge, rect: Rect) -> () {
    let style = if marge.active_pane != ActivePane::List {
        Style::new().fg(Color::DarkGray)
    } else {
        Style::new()
    };

    let lists_block = Block::default()
        .title("App")
        .border_style(style)
        .style(style)
        .borders(Borders::ALL);
    let lists_area = lists_block.inner(rect);

    let content = match marge.app_state.as_ref() {
        &AppState::Failed => "failed",
        &AppState::CheckingRepo(_) => "checking repo...",
        &AppState::GettingPulls => "gettin pulls",
        &AppState::WaitingForCleanRepo => "cleanup repo plx...",
        _ => "<empty>"
    };
    let lists = Paragraph::new(content);
    t.render_widget(lists, lists_area);
    t.render_widget(lists_block, rect);
}

fn render_log<B: Backend>(t: &mut Frame<B>, marge: &mut Marge, rect: Rect) -> () {
    let style = if marge.active_pane != ActivePane::Log {
        let e = TuiWidgetEvent::EscapeKey;
        marge.log_state.transition(&e);
        Style::new().fg(Color::DarkGray)
    } else {
        let maybe_event = match marge.last_event {
            AppEvent::Input(KeyEvent {
                code: KeyCode::Up, ..
            }) => Some(TuiWidgetEvent::PrevPageKey),
            AppEvent::Input(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => Some(TuiWidgetEvent::NextPageKey),
            AppEvent::Input(KeyEvent {
                code: KeyCode::Char(' '),
                ..
            }) => Some(TuiWidgetEvent::EscapeKey),
            // fixme remove
            AppEvent::Input(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) => {
                info!("{}", c);
                None
            }
            _ => None,
        };

        if let Some(e) = maybe_event {
            info!("{:?}", e);
            marge.log_state.transition(&e);
        }

        Style::new()
    };

    let tui_w: TuiLoggerWidget = TuiLoggerWidget::default()
        .block(
            Block::default()
                .title("Logs")
                .border_style(style)
                .title_style(style)
                .style(style)
                .borders(Borders::ALL),
        )
        .output_separator(' ')
        .output_timestamp(Some("%H:%M".to_string()))
        .output_level(None)
        .output_target(false)
        .output_file(false)
        .output_line(false)
        .state(&marge.log_state);

    t.render_widget(tui_w, rect);
}

struct Screen(Terminal<CrosstermBackend<Stdout>>);

impl Screen {
    pub fn try_new() -> anyhow::Result<Self> {
        tui_logger::init_logger(LevelFilter::Trace).unwrap();
        tui_logger::set_default_level(LevelFilter::Trace);

        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;

        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Screen(terminal))
    }

    pub fn draw<F>(&mut self, f: F) -> Result<CompletedFrame<'_>, std::io::Error>
    where
        F: FnOnce(&mut Frame<CrosstermBackend<Stdout>>),
    {
        self.0.draw(f)
    }
}

impl Termination for Screen {
    fn report(mut self) -> std::process::ExitCode {
        use crossterm::{
            execute,
            terminal::{disable_raw_mode, LeaveAlternateScreen},
        };
        use std::process::ExitCode;

        if let Err(e) = execute!(self.0.backend_mut(), LeaveAlternateScreen) {
            eprintln!("{:?}", e);
            ExitCode::FAILURE
        } else if let Err(e) = disable_raw_mode() {
            eprintln!("{:?}", e);
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }
}
