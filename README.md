# dcm — Docker Compose Manager

Tired of `cd`-ing to the right directory just to run `docker compose up`? `dcm` lets you save your compose projects under short names and manage them from anywhere.

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

# Run any compose subcommand
dcm run myapp logs -f
dcm run myapp exec web sh

# Remove a saved project
dcm rm myapp
```
