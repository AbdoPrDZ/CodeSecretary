use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "secretary-agent",
    version,
    about = "CodeSecretary local agent — collects redacted activity evidence and uploads it"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the agent in the foreground (default).
    Run(RunArgs),
    /// Sign this device in with your CodeSecretary account (opens a browser).
    Login(LoginArgs),
    /// Sign out: ends the session and removes the token from the OS keychain.
    Logout,
    /// Show sign-in state, device and how many events wait for upload.
    Status,
    /// Print the directories the agent uses for config, data and logs.
    Paths,
    /// Print idle/active state and the focused app/title live (Phase 0 prototype).
    /// Output stays in the terminal; nothing is logged, stored, or sent.
    Probe(ProbeArgs),
}

#[derive(Debug, Default, Args)]
pub struct RunArgs {
    /// Stop gracefully after this many seconds (used by tests and diagnostics).
    #[arg(long, hide = true, value_name = "SECONDS")]
    pub run_for: Option<u64>,
}

#[derive(Debug, Default, Args)]
pub struct LoginArgs {
    /// Warehouse URL to sign in to (default: `server_url` from agent.toml).
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,
    /// Do not try to open a browser; just print the link and code.
    #[arg(long)]
    pub no_browser: bool,
}

#[derive(Debug, Args)]
pub struct ProbeArgs {
    /// Seconds without input before the user counts as idle (default: 5 min).
    #[arg(long, default_value_t = 300, value_name = "SECONDS")]
    pub idle_after: u64,
    /// How often to sample, in milliseconds.
    #[arg(long, default_value_t = 1000, value_name = "MS")]
    pub interval_ms: u64,
    /// Show app names only, not window titles.
    #[arg(long)]
    pub no_titles: bool,
    /// Stop after this many samples (used by tests).
    #[arg(long, hide = true)]
    pub samples: Option<u64>,
}
