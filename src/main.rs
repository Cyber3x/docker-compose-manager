use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use colored::Colorize;
use dcm::{cmd_add, cmd_list, cmd_remove, cmd_rename, cmd_status, run_compose};
use std::env;
use std::io::{self, IsTerminal};
use std::process::exit;

#[derive(Parser)]
#[command(
    name = "dcm",
    about = "Docker Compose Manager — save and run named compose projects",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save a project (path to compose file or directory)
    Add {
        name: String,
        path: String,
    },

    /// Remove a saved project
    #[command(alias = "rm")]
    Remove {
        name: String,
    },

    /// List all saved projects
    #[command(alias = "ls")]
    List,

    /// Run `docker compose up` for a saved project
    Up {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Run `docker compose down` for a saved project
    Down {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Run any `docker compose` subcommand for a saved project
    Run {
        name: String,
        subcommand: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Show running status of a saved project's services
    #[command(alias = "ps")]
    Status {
        /// Name of the project
        name: String,
    },

    /// Rename a saved project
    #[command(alias = "mv")]
    Rename {
        /// Current name
        old: String,
        /// New name
        new: String,
    },

    /// Follow logs for a saved project (`docker compose logs -f`)
    Logs {
        /// Name of the project
        name: String,
        /// Extra arguments forwarded to `docker compose logs`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Print shell completion script
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

fn main() {
    if env::var("NO_COLOR").is_ok() || !io::stdout().is_terminal() {
        colored::control::set_override(false);
    }

    let cli = Cli::parse();

    let result = match cli.command {
        Cmd::Add {
            name,
            path,
        } => cmd_add(&name, &path),
        Cmd::Remove {
            name,
        } => cmd_remove(&name),
        Cmd::List => cmd_list(),
        Cmd::Up {
            name,
            extra,
        } => run_compose(&name, "up", &extra),
        Cmd::Down {
            name,
            extra,
        } => run_compose(&name, "down", &extra),
        Cmd::Run {
            name,
            subcommand,
            extra,
        } => run_compose(&name, &subcommand, &extra),
        Cmd::Status {
            name,
        } => cmd_status(&name),
        Cmd::Rename {
            old,
            new,
        } => cmd_rename(&old, &new),
        Cmd::Logs {
            name,
            mut extra,
        } => {
            extra.insert(0, "-f".to_string());
            run_compose(&name, "logs", &extra)
        },
        Cmd::Completions {
            shell,
        } => {
            generate(shell, &mut Cli::command(), "dcm", &mut io::stdout());
            Ok(())
        },
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        exit(1);
    }
}
