# Configuration Guide

Everything Tuisual shows you comes from a **provider** — a source of searchable items. This guide
explains how providers work, how to write your own, and every field the JSON format supports.

If you just want to use Tuisual, see the [Usage Guide](usage.md) instead. This guide is for
customizing and extending it.

---

## Table of contents

- [Where providers live](#where-providers-live)
- [Static providers (plain JSON)](#static-providers-plain-json)
- [Dynamic providers (JSON + a program)](#dynamic-providers-json--a-program)
- [The item schema](#the-item-schema)
- [Action types](#action-types)
- [Sub-items (drill-down menus)](#sub-items-drill-down-menus)
- [Input prompts (compose flow)](#input-prompts-compose-flow)
- [Writing your own dynamic provider](#writing-your-own-dynamic-provider)
- [The PATH command provider](#the-path-command-provider)
- [Environment variables](#environment-variables)

---

## Where providers live

Provider files are `.json` files in a `providers/` directory. By default that's the `providers/`
folder next to the binary (or the repo root when running with `cargo run`). The installed version
of Tuisual points at `~/.config/tuisual/providers` instead (set up automatically by `install.sh`).

You can override the location at any time with an environment variable:

```
TUISUAL_PROVIDERS_DIR=/path/to/my/providers tuisual
```

To add a new provider, drop a new `.json` file into that directory — no rebuild or restart script
required, just a fresh `tuisual` run.

---

## Static providers (plain JSON)

The simplest provider is a file listing its items directly. Good for a handful of fixed actions
(shortcuts, scripts, bookmarks, etc.).

```json
{
  "name": "example",
  "short_flag": "x",
  "items": [
    {
      "id": "hello",
      "title": "Say Hello",
      "subtitle": "Prints a greeting",
      "info": {
        "summary": "Runs `echo` to print a greeting to the terminal.",
        "fields": [
          { "label": "Command", "value": "echo" }
        ]
      },
      "action": { "type": "shell_command", "value": "echo Hello from Tuisual!" }
    }
  ]
}
```

- `name` — the provider's unique identifier. Also usable as a long CLI flag: `--example`.
- `short_flag` — a single character usable as a short CLI flag: `-x`. Optional.
- `items` — an array of items following the [item schema](#the-item-schema) below.

Run it with:

```
tuisual -x
# or
tuisual --example
```

---

## Dynamic providers (JSON + a program)

A dynamic provider generates its items by running a program instead of listing them statically —
useful when the list depends on the current state of your system (installed apps, PATH binaries,
available updates, etc.).

Instead of `items`, the JSON file points at something to run. Use **one** of:

- `command` — path to an executable (recommended; use for compiled helper binaries).
- `shell_command` — an inline shell snippet that prints JSON to stdout (quick one-liners).

A provider file must not set both.

**Binary example:**

```json
{
  "name": "desktop-app-launcher",
  "short_flag": "l",
  "command": "./target/debug/desktop_apps_provider"
}
```

**Inline shell example:**

```json
{
  "name": "quick-dynamic",
  "short_flag": "q",
  "shell_command": "printf '%s' '[{\"id\":\"quick\",\"title\":\"Quick\",\"subtitle\":\"Generated\",\"info\":{\"summary\":\"S\",\"fields\":[]},\"action\":{\"type\":\"shell_command\",\"value\":\"echo quick\"}}]'"
}
```

Either way, the program is expected to print a JSON array of items (same [item schema](#the-item-schema)
as static providers) to stdout, then exit `0`.

---

## The item schema

Every item — whether from a static list or generated dynamically — must match this shape:

```json
{
  "id": "unique-slug",
  "title": "Display Name",
  "subtitle": "Short description shown in the list",
  "info": {
    "summary": "Longer description shown in the right-hand info pane.",
    "fields": [
      { "label": "Key", "value": "Value" }
    ]
  },
  "action": {
    "type": "shell_command",
    "value": "echo hello"
  }
}
```

| Field           | Required | Notes                                                              |
|-----------------|----------|---------------------------------------------------------------------|
| `id`            | yes      | Unique within the provider. Must not be empty.                     |
| `title`         | yes      | What's shown as the item's name. Must not be empty.                |
| `subtitle`      | yes      | Short one-line description shown next to the title.                |
| `info.summary`  | yes      | Shown in the right-hand info pane when the item is selected. Must not be empty. |
| `info.fields`   | yes (may be empty array) | Extra label/value rows shown under the summary.        |
| `action`        | yes      | What happens when the item is launched. See [action types](#action-types). |
| `require_sub_item` | no    | If `true`, the user must pick a sub-item before anything runs.     |
| `sub_items`     | no       | Drill-down menu. See [sub-items](#sub-items-drill-down-menus).      |

Invalid items (empty required fields, malformed actions, etc.) are rejected at load time and
reported instead of crashing the whole provider — check the catalog/warning output if an item
doesn't show up.

---

## Action types

| Type                       | Behavior                                                         |
|----------------------------|--------------------------------------------------------------------|
| `shell_command`            | Runs the command, then returns you to the Tuisual list.           |
| `shell_command_exit`       | Runs the command, then exits Tuisual.                              |
| `shell_command_with_flag`  | Prompts for input, builds a flag from it, then runs the command.   |
| `provider_hint`            | Loads another provider by name instead of running a command.       |

Examples:

```json
{ "action": { "type": "shell_command", "value": "notify-send Hello" } }
```

```json
{ "action": { "type": "shell_command_exit", "value": "hyprshutdown" } }
```

```json
{
  "action": {
    "type": "shell_command_with_flag",
    "value": {
      "command": "ping",
      "flag_prefix": "-c ",
      "prompt": "Number of pings",
      "exit_after": false
    }
  }
}
```

```json
{ "action": { "type": "provider_hint", "value": "desktop-app-launcher" } }
```

---

## Sub-items (drill-down menus)

An item can carry a `sub_items` list to present a small menu instead of running immediately.
Set `require_sub_item: true` to force a choice — the parent item's own action never runs in that
case.

```json
{
  "id": "my-tool",
  "title": "My Tool",
  "subtitle": "A tool with flag options",
  "info": { "summary": "Runs mytool with optional flags.", "fields": [] },
  "action": { "type": "shell_command_exit", "value": "mytool" },
  "require_sub_item": true,
  "sub_items": [
    {
      "id": "run",
      "title": "Run Tool",
      "flags": []
    },
    {
      "id": "verbose",
      "title": "Verbose mode",
      "subtitle": "Runs with --verbose",
      "flags": ["--verbose"]
    }
  ]
}
```

Each sub-item appends its `flags` to the parent's command before running it. Sub-items can also
nest further sub-items (`sub_items` inside a sub-item), useful for multi-level menus.

---

## Input prompts (compose flow)

A sub-item can ask for freeform text instead of (or in addition to) fixed flags:

```json
{
  "id": "name-input",
  "title": "Set name",
  "input": {
    "flag_prefix": "name=",
    "prompt": "Enter the name"
  }
}
```

When selected, Tuisual switches to a text input. Whatever you type is appended after
`flag_prefix` (e.g. `name=alice`) and the whole thing is appended to the parent command before
it runs.

---

## Writing your own dynamic provider

Use this when a static list isn't enough — e.g. you want to scan installed packages, query a
service, or generate items from some other program.

1. **Create a binary**, e.g. `src/bin/my_provider.rs`.
2. **Guard against accidental direct execution** — Tuisual sets an environment variable when it
   invokes providers, so require it:

   ```rust
   if std::env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
       eprintln!("Run via Tuisual.");
       std::process::exit(2);
   }
   ```

3. **Print a JSON array of items** (matching the [item schema](#the-item-schema)) to stdout, then
   exit with status `0`.
4. **Register it** with a provider file, `providers/my_provider.json`:

   ```json
   { "name": "my-provider", "short_flag": "m", "command": "./target/debug/my_provider" }
   ```

5. **Build and run**:

   ```
   cargo build --bin my_provider
   cargo run -- -m
   ```

This is exactly how the built-in desktop app launcher, PATH command explorer, and package manager
provider work — see `src/bin/desktop_apps_provider.rs`, `src/bin/path_commands_provider.rs`, and
`src/bin/pkg_manager_provider.rs` for real examples.

---

## The PATH command provider

The built-in PATH provider (`-P` / `--path-launcher`) deserves a special mention since it's the
most complex built-in provider.

- It lists every executable found on your `PATH`.
- Selecting a command opens a sub-item menu rather than running it immediately.
- **Run Command** executes the base command with no extra arguments.
- **Custom Flags / Args** prompts for freeform text and appends it to the command.
- It also tries to discover known flags for each command automatically (see below), plus any
  flags you've explicitly curated in a catalog file.

### Curated flag catalog

Tuisual doesn't guess flags for a command unless you tell it to. Command-specific flags come from
an explicit catalog file at `providers/path_flags_catalog.json`:

```json
{
  "commands": [
    {
      "command": "ls",
      "flags": [
        { "flag": "--all", "title": "All (--all)", "subtitle": "Do not ignore entries starting with ." }
      ]
    }
  ]
}
```

Only well-formed flags (`--long-flag` or `-x`) are accepted; malformed entries are ignored. Add
an entry here any time you want a specific flag to show up as a sub-item for a specific command.

### Automatic flag discovery

Separately, Tuisual tries to discover flags on its own by reading (not executing) these sources
for each command:

- `/usr/share/bash-completion/completions/<command>`
- `/usr/share/zsh/site-functions/_<command>`
- `/usr/share/fish/vendor_completions.d/<command>.fish`
- `man -- <command>` (when available)
- Docs under `/usr/share/doc` and `/usr/local/share/doc` matching the command name

Discovered flags appear as `Auto <flag>` sub-items, with a description pulled from the man page
when available. If none of these sources exist for a command, no automatic flags are added — it
never runs or probes the binary itself to guess.

Auto-discovery is bounded so startup stays fast; see the relevant environment variables below if
you need to tune it.

---

## Environment variables

| Variable                                       | Effect                                                                 |
|-------------------------------------------------|-------------------------------------------------------------------------|
| `TUISUAL_PROVIDERS_DIR`                          | Override the directory Tuisual loads provider `.json` files from.       |
| `TUISUAL_PROVIDER_MODE`                          | Set to `1` by Tuisual automatically when invoking a dynamic provider.   |
| `TUISUAL_PATH_FLAGS_CATALOG`                     | Override the path to the curated PATH flag catalog JSON.                |
| `TUISUAL_PATH_AUTODISCOVER`                      | Toggle completion/man/docs-based auto-discovery (default: on; set `0`/`false`/`no`/`off` to disable). |
| `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_LIMIT`       | Max commands to scan with man/docs when completion data is unavailable (default `120`). |
| `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_BUDGET_MS`   | Total time budget in milliseconds for man/docs discovery per run (default `3000`). |
| `TUISUAL_PATH_AUTODISCOVER_DOC_FILES_LIMIT`      | Max documentation files read per command during docs discovery (default `8`). |
| `TUISUAL_PROVIDER_TIMING`                        | Set to `1` to print provider load timing to help debug slow startups.   |
| `TUISUAL_PROVIDER_DISABLE_CACHE`                 | Set to `1` to disable caching of heavy provider command output (e.g. package manager queries). |
| `TUISUAL_PACMAN_REPO_CACHE_TTL_SECS`             | Cache lifetime for `pacman` repository queries used by the package manager provider. |
| `TUISUAL_FLATPAK_REMOTE_CACHE_TTL_SECS`          | Cache lifetime for `flatpak remote-ls` queries used by the package manager provider. |
