# dcm — Docker Compose Manager

Tired of `cd`-ing to the right directory just to run `docker compose up`? `dcm` lets you save your compose projects under short names and manage them from anywhere.

## Install

```sh
cargo install --git https://github.com/youruser/docker-compose-manager
```

## Usage

```sh
# Save a project
dcm add myapp /path/to/myapp          # explicit path
dcm add myapp .                       # current directory

# Start / stop
dcm up myapp
dcm up myapp -d                       # extra flags pass through
dcm down myapp

# Check status
dcm status myapp                      # per-service table
dcm list                              # all projects + running state

# Follow logs
dcm logs myapp                        # all services
dcm logs myapp web                    # specific service

# Run any compose subcommand
dcm run myapp exec web sh
dcm run myapp ps

# Manage saved projects
dcm rename myapp myapp-v2             # rename (alias: mv)
dcm rm myapp                          # remove

# Shell completions
eval "$(dcm completions bash)"        # bash
eval "$(dcm completions zsh)"         # zsh
dcm completions fish | source         # fish
```

## Config

Projects are stored in `$XDG_CONFIG_HOME/dcm/projects` (default: `~/.config/dcm/projects`).
