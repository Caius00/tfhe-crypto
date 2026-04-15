use std::io;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod crypto;
mod ui;

use app::{App, BgMsg};

fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (bg_tx, bg_rx) = mpsc::channel::<BgMsg>();
    let mut app = App::new(bg_tx);
    app.start_key_generation();

    let result = run_loop(&mut terminal, &mut app, &bg_rx);

    // Restore terminal in all cases
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    bg_rx: &mpsc::Receiver<BgMsg>,
) -> anyhow::Result<()> {
    loop {
        // Drain all pending background messages
        while let Ok(msg) = bg_rx.try_recv() {
            app.handle_bg(msg);
        }

        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if app.on_key(key) {
                    return Ok(());
                }
            }
        }
    }
}
