use clap::{Parser, Subcommand};
use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, Table, presets::UTF8_FULL};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
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
}

// ---------------------------------------------------------------------------
// Config persistence
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".config").join("dcm").join("projects")
}

fn load_projects() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(contents) = fs::read_to_string(config_path()) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((name, loc)) = line.split_once('=') {
                map.insert(name.trim().to_string(), loc.trim().to_string());
            }
        }
    }
    map
}

fn save_projects(projects: &HashMap<String, String>) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("{} could not create config directory: {e}", "error:".red().bold());
            exit(1);
        });
    }
    let mut lines: Vec<String> = projects
        .iter()
        .map(|(name, loc)| format!("{name}={loc}"))
        .collect();
    lines.sort();
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, lines.join("\n") + "\n").unwrap_or_else(|e| {
        eprintln!("{} could not write config file: {e}", "error:".red().bold());
        exit(1);
    });
    fs::rename(&tmp, &path).unwrap_or_else(|e| {
        eprintln!("{} could not save config file: {e}", "error:".red().bold());
        exit(1);
    });
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn validate_name(name: &str) {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        eprintln!("{} invalid project name '{}' — only letters, digits, _ and - are allowed", "error:".red().bold(), name.cyan());
        exit(1);
    }
}

fn cmd_add(name: &str, raw_path: &str) {
    validate_name(name);
    let given = Path::new(raw_path);
    let absolute = if raw_path == "." {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else if given.is_absolute() {
        given.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
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

    let mut projects = load_projects();

    if let Some(existing) = projects.get(name) {
        print!("'{}' already points to '{}'. Overwrite? [y/N] ", name.cyan(), existing);
        io::stdout().flush().ok();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).ok();
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("{}", "aborted.".dimmed());
            return;
        }
    }

    projects.insert(name.to_string(), path_str.clone());
    save_projects(&projects);
    println!("{} {} → {}", "saved:".green().bold(), name.cyan(), path_str);
}

fn cmd_remove(name: &str) {
    validate_name(name);
    let mut projects = load_projects();
    if projects.remove(name).is_some() {
        save_projects(&projects);
        println!("{} {}", "removed:".red().bold(), name.cyan());
    } else {
        eprintln!("{} no project named '{}'", "error:".red().bold(), name.cyan());
        exit(1);
    }
}

fn running_status(path: &str) -> Cell {
    let output = Command::new("docker")
        .args(["compose", "-f", path, "ps", "--format", "{{.State}}"])
        .output();

    let Ok(output) = output else {
        return Cell::new("unknown").fg(Color::DarkGrey);
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let states: Vec<&str> = stdout.lines().collect();

    if states.is_empty() {
        return Cell::new("stopped").fg(Color::DarkGrey);
    }

    let total = states.len();
    let running = states.iter().filter(|&&s| s == "running").count();

    match running {
        0          => Cell::new("stopped").fg(Color::DarkGrey),
        n if n == total => Cell::new(format!("running ({n})")).fg(Color::Green),
        n          => Cell::new(format!("partial ({n}/{total})")).fg(Color::Yellow),
    }
}

fn cmd_list() {
    let projects = load_projects();
    if projects.is_empty() {
        println!("no projects saved yet. use {} to add one.", "`dcm add <name> <path>`".cyan());
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("NAME").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("PATH").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("FILE").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("RUNNING").add_attribute(Attribute::Bold).fg(Color::Cyan),
    ]);

    let mut sorted: Vec<_> = projects.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());

    for (name, path) in sorted {
        let (file_cell, path_cell, run_cell) = if Path::new(path).exists() {
            (
                Cell::new("ok").fg(Color::Green),
                Cell::new(path),
                running_status(path),
            )
        } else {
            (
                Cell::new("missing").fg(Color::Red),
                Cell::new(path).fg(Color::DarkGrey),
                Cell::new("-").fg(Color::DarkGrey),
            )
        };
        table.add_row(vec![
            Cell::new(name).fg(Color::Cyan),
            path_cell,
            file_cell,
            run_cell,
        ]);
    }

    println!("{table}");
    println!("{} {}", "config:".dimmed(), config_path().display().to_string().dimmed());
}

fn cmd_status(name: &str) {
    validate_name(name);
    let projects = load_projects();
    let path = projects.get(name).unwrap_or_else(|| {
        eprintln!("{} no project named '{}'", "error:".red().bold(), name.cyan());
        eprintln!("       run {} to see saved projects", "`dcm list`".cyan());
        exit(1);
    });

    if !Path::new(path).exists() {
        eprintln!("{} compose file not found: {}", "error:".red().bold(), path);
        exit(1);
    }

    println!("{} {}", "project:".bold(), name.cyan());
    println!("{} {}", "file:".bold(), path.dimmed());
    println!();

    let output = Command::new("docker")
        .args(["compose", "-f", path, "ps", "--format", "{{.Service}}\t{{.State}}\t{{.Status}}"])
        .output()
        .unwrap_or_else(|e| {
            eprintln!("{} failed to launch docker: {e}", "error:".red().bold());
            exit(1);
        });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{} {}", "error:".red().bold(), stderr.trim());
        exit(output.status.code().unwrap_or(1));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.is_empty() {
        println!("{}", "no containers — project is not running".yellow());
        return;
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
}

fn run_compose(name: &str, subcommand: &str, extra: &[String]) {
    validate_name(name);
    let projects = load_projects();
    let path = projects.get(name).unwrap_or_else(|| {
        eprintln!("{} no project named '{}'", "error:".red().bold(), name.cyan());
        eprintln!("       run {} to see saved projects", "`dcm list`".cyan());
        exit(1);
    });

    if !Path::new(path).exists() {
        eprintln!("{} compose file not found: {}", "error:".red().bold(), path);
        exit(1);
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
        .unwrap_or_else(|e| {
            eprintln!("{} failed to launch docker: {e}", "error:".red().bold());
            exit(1);
        });

    exit(status.code().unwrap_or(1));
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Cmd::Add { name, path } => cmd_add(&name, &path),
        Cmd::Remove { name } => cmd_remove(&name),
        Cmd::List => cmd_list(),
        Cmd::Up { name, extra } => run_compose(&name, "up", &extra),
        Cmd::Down { name, extra } => run_compose(&name, "down", &extra),
        Cmd::Run { name, subcommand, extra } => run_compose(&name, &subcommand, &extra),
        Cmd::Status { name } => cmd_status(&name),
    }
}
