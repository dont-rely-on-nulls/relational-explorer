mod connection;
mod input;
mod repl;
mod ui;

use color_eyre::Result;
use repl::Repl;
use std::process::{Child, Command};
use std::time::Duration;

const SERVER_ADDR: &str = "127.0.0.1:7777";
const DEFAULT_SERVER_BIN: &str =
    "../sakura/_build/default/bin/server.exe";

fn start_server() -> Option<Child> {
    let bin = std::env::var("SAKURA_SERVER")
        .unwrap_or_else(|_| DEFAULT_SERVER_BIN.to_string());
    Command::new(&bin).spawn().ok()
}

fn connect_with_retry(attempts: u32, delay: Duration) -> Option<connection::Connection> {
    for _ in 0..attempts {
        if let Ok(conn) = connection::Connection::connect(SERVER_ADDR) {
            return Some(conn);
        }
        std::thread::sleep(delay);
    }
    None
}

fn main() -> Result<()> {
    color_eyre::install()?;

    // If the server is already running, connect directly.
    // Otherwise spawn it and retry.
    let (conn, server_child) =
        if let Ok(conn) = connection::Connection::connect(SERVER_ADDR) {
            (Some(conn), None)
        } else {
            let child = start_server();
            let conn = connect_with_retry(10, Duration::from_millis(100));
            (conn, child)
        };

    let terminal = ratatui::init();
    let result = Repl::new(conn).run(terminal);
    ratatui::restore();

    if let Some(mut child) = server_child {
        child.kill().ok();
    }

    result
}
