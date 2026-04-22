mod connection;
mod input;
mod language;
mod repl;
mod ui;

use clap::Parser;
use color_eyre::Result;
use repl::Repl;
use std::process::{Child, Command};
use std::time::Duration;

const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:7777";
const DEFAULT_SERVER_BIN: &str = "../sakura/_build/default/bin/server.exe";

/// Sakura REPL — interactive client for the Sakura relational engine.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Server address: a Unix socket path (e.g. /tmp/sakura.sock)
    /// or a TCP host:port (e.g. 127.0.0.1:7777).
    #[arg(short, long, default_value = DEFAULT_SERVER_ADDR)]
    address: String,
}

fn start_server() -> Option<Child> {
    let bin = std::env::var("SAKURA_SERVER").unwrap_or_else(|_| DEFAULT_SERVER_BIN.to_string());
    Command::new(&bin).spawn().ok()
}

fn connect_with_retry(
    addr: &str,
    attempts: u32,
    delay: Duration,
) -> Option<connection::Connection> {
    for _ in 0..attempts {
        if let Ok(conn) = connection::Connection::connect(addr) {
            return Some(conn);
        }
        std::thread::sleep(delay);
    }
    None
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    let server_addr = cli.address;

    // Install panic hook to restore terminal on panic
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        ratatui::restore();
        default_panic(panic_info);
    }));

    // If the server is already running, connect directly.
    // Otherwise spawn it and retry (only for TCP; Unix sockets
    // are assumed to be managed externally).
    let (conn, server_child) = if let Ok(conn) = connection::Connection::connect(&server_addr) {
        (Some(conn), None)
    } else if !connection::is_unix_socket(&server_addr) {
        let child = start_server();
        let conn = connect_with_retry(&server_addr, 10, Duration::from_millis(100));
        (conn, child)
    } else {
        (None, None)
    };

    let terminal = ratatui::init();
    let result = Repl::new(conn, server_addr).run(terminal);
    ratatui::restore();

    if let Some(mut child) = server_child {
        child.kill().ok();
    }

    result
}
