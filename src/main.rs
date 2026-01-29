mod input;
mod repl;
mod ui;

use color_eyre::Result;
use repl::Repl;

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = Repl::new().run(terminal);
    ratatui::restore();
    result
}
