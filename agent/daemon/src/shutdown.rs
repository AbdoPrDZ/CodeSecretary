//! OS shutdown signals.

use std::io;

/// Resolves when the OS asks the agent to stop. Returns the signal name.
///
/// - Unix: SIGINT, SIGTERM, SIGHUP.
/// - Windows: Ctrl+C, Ctrl+Break, console close, logoff, system shutdown.
pub async fn wait_for_signal() -> io::Result<&'static str> {
    imp::wait().await
}

#[cfg(unix)]
mod imp {
    use std::io;

    use tokio::signal::unix::{SignalKind, signal};

    pub async fn wait() -> io::Result<&'static str> {
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        Ok(tokio::select! {
            _ = interrupt.recv() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
            _ = hangup.recv() => "SIGHUP",
        })
    }
}

#[cfg(windows)]
mod imp {
    use std::io;

    use tokio::signal::windows;

    pub async fn wait() -> io::Result<&'static str> {
        let mut ctrl_c = windows::ctrl_c()?;
        let mut ctrl_break = windows::ctrl_break()?;
        let mut close = windows::ctrl_close()?;
        let mut logoff = windows::ctrl_logoff()?;
        let mut shutdown = windows::ctrl_shutdown()?;
        Ok(tokio::select! {
            _ = ctrl_c.recv() => "CTRL_C",
            _ = ctrl_break.recv() => "CTRL_BREAK",
            _ = close.recv() => "CTRL_CLOSE",
            _ = logoff.recv() => "CTRL_LOGOFF",
            _ = shutdown.recv() => "CTRL_SHUTDOWN",
        })
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::io;

    pub async fn wait() -> io::Result<&'static str> {
        tokio::signal::ctrl_c().await?;
        Ok("CTRL_C")
    }
}
