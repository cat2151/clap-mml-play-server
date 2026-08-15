use std::{
    ffi::OsStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Context as _, Result};

const EXIT_ON_STDIN_CLOSE_ENV: &str = "CMRT_RENDER_SERVER_EXIT_ON_STDIN_CLOSE";

pub(crate) fn install_if_requested(shutdown: Arc<AtomicBool>) -> Result<()> {
    if !guard_is_enabled(std::env::var_os(EXIT_ON_STDIN_CLOSE_ENV).as_deref()) {
        return Ok(());
    }

    std::thread::Builder::new()
        .name("render-server-parent-guard".to_string())
        .spawn(move || {
            let stdin = std::io::stdin();
            monitor_until_closed(stdin.lock(), &shutdown);
        })
        .context("failed to spawn render-server parent guard")?;
    Ok(())
}

fn guard_is_enabled(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

fn monitor_until_closed(mut input: impl std::io::Read, shutdown: &AtomicBool) {
    let mut buffer = [0_u8; 64];
    loop {
        match input.read(&mut buffer) {
            Ok(0) | Err(_) => {
                shutdown.store(true, Ordering::SeqCst);
                return;
            }
            Ok(_) => {}
        }
    }
}

#[cfg(test)]
mod tests;
