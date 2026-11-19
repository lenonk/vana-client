// Windows release builds open no console window; main() attaches to the parent one when started from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    vana_client::entry()
}