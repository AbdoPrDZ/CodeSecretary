//! CodeSecretary local agent (`secretary-agent`).
//!
//! The only component that runs on the developer's machine. It collects,
//! redacts, buffers and uploads activity evidence; it never calculates time.

mod api;
mod app;
mod auth;
mod buffer;
mod cli;
mod config;
mod heartbeat;
mod instance;
mod logging;
mod login;
mod paths;
mod pipeline;
mod platform;
mod presence;
mod probe;
mod remote_config;
mod shutdown;
mod state;
mod supervisor;
#[cfg(test)]
mod upload_tests;
mod uploader;

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

use crate::cli::{Cli, Command, RunArgs};
use crate::config::LocalConfig;
use crate::instance::{InstanceLock, LockError};
use crate::paths::AgentPaths;

/// Process exit codes. Stable: installers and service managers rely on them.
mod exit {
    pub const FAILURE: u8 = 1;
    pub const CONFIG: u8 = 2;
    pub const ALREADY_RUNNING: u8 = 3;
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run(RunArgs::default())) {
        Command::Run(args) => run(args),
        Command::Login(args) => login::login(args),
        Command::Logout => login::logout(),
        Command::Status => login::status(),
        Command::Paths => print_paths(),
        Command::Probe(args) => probe::run(args),
    }
}

fn run(args: RunArgs) -> ExitCode {
    // Everything before logging is initialized reports to stderr only.
    let paths = match AgentPaths::resolve().and_then(|p| p.ensure().map(|()| p)) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("secretary-agent: {err:#}");
            return ExitCode::from(exit::FAILURE);
        }
    };

    let config = match LocalConfig::load(&paths.config_file()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("secretary-agent: {err}");
            return ExitCode::from(exit::CONFIG);
        }
    };

    // Held for the whole process lifetime; released when dropped on return.
    let _lock = match InstanceLock::acquire(&paths.lock_file()) {
        Ok(lock) => lock,
        Err(LockError::AlreadyRunning) => {
            eprintln!("secretary-agent: {}", LockError::AlreadyRunning);
            return ExitCode::from(exit::ALREADY_RUNNING);
        }
        Err(err) => {
            eprintln!("secretary-agent: {err}");
            return ExitCode::from(exit::FAILURE);
        }
    };

    // Flushes buffered log lines when dropped, so it must outlive the runtime.
    let _log_guard = match logging::init(&paths.log_dir, &config.log_level) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("secretary-agent: {err:#}");
            return ExitCode::from(exit::CONFIG);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("secretary-worker")
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            tracing::error!(error = %err, "failed to start async runtime");
            return ExitCode::from(exit::FAILURE);
        }
    };

    let run_for = args.run_for.map(Duration::from_secs);
    match runtime.block_on(app::run(paths, config, run_for)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %format!("{err:#}"), "agent failed");
            ExitCode::from(exit::FAILURE)
        }
    }
}

fn print_paths() -> ExitCode {
    match AgentPaths::resolve() {
        Ok(paths) => {
            println!("config: {}", paths.config_dir.display());
            println!("data:   {}", paths.data_dir.display());
            println!("logs:   {}", paths.log_dir.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("secretary-agent: {err:#}");
            ExitCode::from(exit::FAILURE)
        }
    }
}
