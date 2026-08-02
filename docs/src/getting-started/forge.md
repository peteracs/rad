# Using Rad CLI

Rad provides a native CLI (`rad`) for running, testing, formatting, linting code, project scaffolding, and snapshots.

## Create a new project

```bash
rad new myproject
cd myproject
```

This generates a project directory with a `main.rad` entry point, a `rad.toml` config, test stubs, and a snapshot test scaffold.

## Commands

| Command | What it does |
|---|---|
| `rad <file>` | Compile and run a `.rad` file |
| `rad test [dir]` | Run all `.rad` test files |
| `rad fmt [file\|dir]` | Format `.rad` files natively |
| `rad fmt --check` | Check formatting without changing files (CI mode) |
| `rad lint [--preset P]` | Lint with a preset: `enterprise`, `strict`, or `teaching` |
| `rad new <name>` | Scaffold a new Rad project (default template) |
| `rad new <name> --template T` | Scaffold from a named template |
| `rad new --list-templates` | List available project templates |
| `rad snapshot [dir]` | Verify script output against stored `.snap` files |
| `rad snapshot --create` | Create snapshots for files that don't have one yet |
| `rad snapshot --update` | Overwrite all snapshots with current output |
| `rad play` | Open the browser playground |

## Typical workflow

```bash
rad new myproject && cd myproject

# edit src/main.rad ...

rad main.rad                         # run it
rad test                             # test it
rad fmt                              # format it
rad lint --preset teaching           # lint it (beginner-friendly)
rad snapshot --create tests/         # create output baselines
rad play                             # share it
```

## Formatting

`rad fmt` normalizes indentation, operator spacing, blank lines, and trailing whitespace across all `.rad` files:

```bash
rad fmt                    # format everything in the current directory
rad fmt src/               # format only src/
rad fmt --check            # exit non-zero if any file needs formatting
```

The formatter is idempotent — running it twice produces the same output. The same engine powers the LSP's document formatting.

## Linting

`rad lint` combines custom source-level checks with the Rust VM's type checker. Choose a preset to match your project's maturity:

```bash
rad lint --preset enterprise    # strictest — production codebases
rad lint --preset strict        # strict types + warnings as errors
rad lint --preset teaching      # suggestions only, no hard failures
```

### Preset comparison

| Rule | Enterprise | Strict | Teaching |
|---|:---:|:---:|:---:|
| Require type annotations | Yes | Yes | Suggest |
| Deny warnings | Yes | Yes | No |
| Max function lines | 50 | 80 | 100 |
| Max file lines | 500 | 1000 | 2000 |
| PascalCase for types | Yes | — | — |
| Suggest `pure fn` | — | — | Yes |
| Complex pipeline warning | — | — | Yes |

## Snapshot testing

Snapshot tests capture the stdout (and stderr) of `.rad` scripts and compare future runs against the stored baseline:

```bash
# Create snapshots for all .rad files in a directory
rad snapshot --create tests/snapshots/

# Verify snapshots (fails if output has changed)
rad snapshot tests/snapshots/

# Update all snapshots after intentional changes
rad snapshot --update tests/snapshots/
```

Each `.snap` file stores the source path, exit code, stdout, and stderr. On failure, a unified diff shows exactly what changed.

## Project templates

Start from an opinionated template instead of a blank project. Each template creates a runnable project that demonstrates a real production pattern — first success in under 10 minutes.

```bash
rad new my-app --template workflow       # approval pipeline with state machines
rad new my-app --template stream         # telemetry processor with windowed aggregation
rad new my-app --template simulation     # agent-based simulation with ECS ticks
rad new my-app --template control-plane  # fleet management with autoscaling
rad new my-app                           # minimal ECS hello-world (default)
```

List all available templates with descriptions:

```bash
rad new --list-templates
```

### Available templates

| Template | What you get |
|---|---|
| `default` | Minimal ECS hello-world (component, entity, system, `schedule`) |
| `workflow` | Approval pipeline with state machines, events, and SLA tracking |
| `stream` | Telemetry stream processor with windowed aggregation and alerting |
| `simulation` | Agent-based simulation with ECS entities, system ticks, and disruptions |
| `control-plane` | Service fleet management with autoscaling, health checks, and alerting |

Every template produces a project that runs with `rad main.rad` immediately. See the [example catalog](../examples/catalog.md) for runnable programs using the same language features.
