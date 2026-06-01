use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, Table, presets::UTF8_FULL};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Config persistence
// ---------------------------------------------------------------------------

fn config_path() -> Result<PathBuf> {
    let base = if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = env::var("HOME")
            .context("$HOME is not set — cannot locate config directory")?;
        PathBuf::from(home).join(".config")
    };
    Ok(base.join("dcm").join("projects"))
}

fn load_projects() -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if let Ok(contents) = fs::read_to_string(config_path()?) {
        for (i, line) in contents.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((name, loc)) = line.split_once('=') {
                map.insert(name.trim().to_string(), loc.trim().to_string());
            } else {
                eprintln!("{} malformed config line {}: {:?}", "warning:".yellow().bold(), i + 1, line);
            }
        }
    }
    Ok(map)
}

fn save_projects(projects: &HashMap<String, String>) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("could not create config directory")?;
    }
    let mut lines: Vec<String> = projects
        .iter()
        .map(|(name, loc)| format!("{name}={loc}"))
        .collect();
    lines.sort();
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, lines.join("\n") + "\n").context("could not write config file")?;
    fs::rename(&tmp, &path).context("could not save config file")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("invalid project name '{}' — only letters, digits, _ and - are allowed", name);
    }
    Ok(())
}

fn cmd_add(name: &str, raw_path: &str) -> Result<()> {
    validate_name(name)?;

    let given = Path::new(raw_path);
    let absolute = if raw_path == "." {
        env::current_dir().context("could not read current directory")?
    } else if given.is_absolute() {
        given.to_path_buf()
    } else {
        env::current_dir()
            .context("could not read current directory")?
            .join(given)
    };

    let compose_path = if absolute.is_dir() {
        let candidates = [
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ];
        candidates
            .iter()
            .map(|f| absolute.join(f))
            .find(|p| p.exists())
            .unwrap_or_else(|| absolute.join("docker-compose.yml"))
    } else {
        absolute
    };

    let path_str = compose_path.to_string_lossy().to_string();

    if !compose_path.exists() {
        eprintln!("{} '{}' does not exist yet — saving anyway", "warning:".yellow().bold(), path_str);
    }

    let mut projects = load_projects()?;

    if let Some(existing) = projects.get(name) {
        print!("'{}' already points to '{}'. Overwrite? [y/N] ", name.cyan(), existing);
        io::stdout().flush().ok();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).ok();
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("{}", "aborted.".dimmed());
            return Ok(());
        }
    }

    projects.insert(name.to_string(), path_str.clone());
    save_projects(&projects)?;
    println!("{} {} → {}", "saved:".green().bold(), name.cyan(), path_str);
    Ok(())
}

fn cmd_remove(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut projects = load_projects()?;
    if projects.remove(name).is_some() {
        save_projects(&projects)?;
        println!("{} {}", "removed:".red().bold(), name.cyan());
    } else {
        bail!("no project named '{name}'");
    }
    Ok(())
}

enum RunState {
    Running(usize),
    Partial(usize, usize),
    Stopped,
    Unknown,
}

impl RunState {
    fn label(&self) -> String {
        match self {
            Self::Running(n)        => format!("running ({n})"),
            Self::Partial(n, total) => format!("partial ({n}/{total})"),
            Self::Stopped           => "stopped".to_string(),
            Self::Unknown           => "unknown".to_string(),
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Running(_)              => Color::Green,
            Self::Partial(..)             => Color::Yellow,
            Self::Stopped | Self::Unknown => Color::DarkGrey,
        }
    }
}

fn running_status(path: &str) -> RunState {
    let Ok(output) = Command::new("docker")
        .args(["compose", "-f", path, "ps", "--format", "{{.State}}"])
        .output()
    else {
        return RunState::Unknown;
    };

    if !output.status.success() {
        return RunState::Unknown;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let states: Vec<&str> = stdout.lines().collect();

    if states.is_empty() {
        return RunState::Stopped;
    }

    let total = states.len();
    let running = states.iter().filter(|&&s| s == "running").count();

    match running {
        0               => RunState::Stopped,
        n if n == total => RunState::Running(n),
        n               => RunState::Partial(n, total),
    }
}

fn cmd_list() -> Result<()> {
    let projects = load_projects()?;
    if projects.is_empty() {
        println!("no projects saved yet. use {} to add one.", "`dcm add <name> <path>`".cyan());
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("NAME").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("PATH").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("FILE").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("RUNNING").add_attribute(Attribute::Bold).fg(Color::Cyan),
    ]);

    let mut sorted: Vec<(String, String)> = projects.into_iter().collect();
    sorted.sort_by(|(a, _), (b, _)| a.cmp(b));

    let handles: Vec<_> = sorted
        .into_iter()
        .map(|(name, path)| {
            std::thread::spawn(move || {
                let file_exists = Path::new(&path).exists();
                let run = if file_exists { Some(running_status(&path)) } else { None };
                (name, path, file_exists, run)
            })
        })
        .collect();

    for handle in handles {
        let Ok((name, path, file_exists, run)) = handle.join() else {
            continue;
        };
        let (file_cell, path_cell, run_cell) = if file_exists {
            let state = run.unwrap();
            (
                Cell::new("ok").fg(Color::Green),
                Cell::new(&path),
                Cell::new(state.label()).fg(state.color()),
            )
        } else {
            (
                Cell::new("missing").fg(Color::Red),
                Cell::new(&path).fg(Color::DarkGrey),
                Cell::new("-").fg(Color::DarkGrey),
            )
        };
        table.add_row(vec![
            Cell::new(&name).fg(Color::Cyan),
            path_cell,
            file_cell,
            run_cell,
        ]);
    }

    println!("{table}");
    println!("{} {}", "config:".dimmed(), config_path()?.display().to_string().dimmed());
    Ok(())
}

fn cmd_status(name: &str) -> Result<()> {
    validate_name(name)?;
    let projects = load_projects()?;
    let path = projects.get(name)
        .with_context(|| format!("no project named '{name}' — run `dcm list` to see saved projects"))?;

    if !Path::new(path).exists() {
        bail!("compose file not found: {path}");
    }

    println!("{} {}", "project:".bold(), name.cyan());
    println!("{} {}", "file:".bold(), path.dimmed());
    println!();

    let output = Command::new("docker")
        .args(["compose", "-f", path, "ps", "--format", "{{.Service}}\t{{.State}}\t{{.Status}}"])
        .output()
        .context("failed to launch docker")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.is_empty() {
        println!("{}", "no containers — project is not running".yellow());
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("SERVICE").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("STATE").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("STATUS").add_attribute(Attribute::Bold).fg(Color::Cyan),
    ]);

    for line in lines {
        let cols: Vec<&str> = line.splitn(3, '\t').collect();
        let service = cols.first().copied().unwrap_or("-");
        let state   = cols.get(1).copied().unwrap_or("-");
        let status  = cols.get(2).copied().unwrap_or("-");

        let state_cell = match state {
            "running"    => Cell::new(state).fg(Color::Green),
            "exited"     => Cell::new(state).fg(Color::Red),
            "paused"     => Cell::new(state).fg(Color::Yellow),
            "restarting" => Cell::new(state).fg(Color::Yellow),
            "dead"       => Cell::new(state).fg(Color::Red),
            _            => Cell::new(state).fg(Color::DarkGrey),
        };

        table.add_row(vec![
            Cell::new(service).fg(Color::Cyan),
            state_cell,
            Cell::new(status),
        ]);
    }

    println!("{table}");
    Ok(())
}

fn cmd_rename(old: &str, new: &str) -> Result<()> {
    validate_name(old)?;
    validate_name(new)?;

    let mut projects = load_projects()?;

    if !projects.contains_key(old) {
        bail!("no project named '{old}'");
    }
    if projects.contains_key(new) {
        bail!("a project named '{new}' already exists");
    }

    let path = projects.remove(old).unwrap();
    projects.insert(new.to_string(), path);
    save_projects(&projects)?;
    println!("{} {} → {}", "renamed:".green().bold(), old.cyan(), new.cyan());
    Ok(())
}

fn run_compose(name: &str, subcommand: &str, extra: &[String]) -> Result<()> {
    validate_name(name)?;
    let projects = load_projects()?;
    let path = projects.get(name)
        .with_context(|| format!("no project named '{name}' — run `dcm list` to see saved projects"))?;

    if !Path::new(path).exists() {
        bail!("compose file not found: {path}");
    }

    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        path.clone(),
        subcommand.to_string(),
    ];
    args.extend_from_slice(extra);

    println!("{} docker {}", "→".bold().cyan(), args.join(" ").dimmed());

    let status = Command::new("docker")
        .args(&args)
        .status()
        .context("failed to launch docker")?;

    exit(status.code().unwrap_or(1));
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    if env::var("NO_COLOR").is_ok() || !io::stdout().is_terminal() {
        colored::control::set_override(false);
    }

    let cli = Cli::parse();

    let result = match cli.command {
        Cmd::Add { name, path }              => cmd_add(&name, &path),
        Cmd::Remove { name }                 => cmd_remove(&name),
        Cmd::List                            => cmd_list(),
        Cmd::Up { name, extra }              => run_compose(&name, "up", &extra),
        Cmd::Down { name, extra }            => run_compose(&name, "down", &extra),
        Cmd::Run { name, subcommand, extra } => run_compose(&name, &subcommand, &extra),
        Cmd::Status { name }                 => cmd_status(&name),
        Cmd::Rename { old, new }             => cmd_rename(&old, &new),
        Cmd::Logs { name, mut extra }        => { extra.insert(0, "-f".to_string()); run_compose(&name, "logs", &extra) },
        Cmd::Completions { shell }           => { generate(shell, &mut Cli::command(), "dcm", &mut io::stdout()); Ok(()) },
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        exit(1);
    }
}
