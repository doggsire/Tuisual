# Tuisual

A terminal launcher with fuzzy search, a right-hand info pane, and a provider system for loading items from any source.

---

## Usage

```
cargo run                    # show provider catalog
cargo run -- -x              # load the example provider
cargo run -- -a              # load desktop apps (dynamic provider)
cargo run -- -p              # load PATH commands (dynamic provider)
cargo run -- -u              # load Arch updates provider
cargo run -- --example       # same as -x, using long flag
cargo run -- --arch-updates  # same as -u, using long flag
```

Multiple flags can be combined to load more than one provider at once.

---

## Keybindings

| Key        | Action                                         |
|------------|------------------------------------------------|
| Type       | Filter the result list (fuzzy search)          |
| `↑` / `↓`  | Move selection                                 |
| `Enter`    | Launch selected item                           |
| `Space`    | Open sub-items / begin compose flow            |
| `Esc`      | Go back / cancel compose / quit                |
| `Ctrl-C`   | Quit                                           |

---

## Provider system

Providers supply the items shown in the launcher. There are two kinds:

### Static providers (JSON)

Defined in `providers/*.json`. Items are embedded directly in the file.

```json
{
  "name": "example",
  "short_flag": "x",
  "items": [ ... ]
}
```

### Dynamic providers (JSON + binary)

The JSON file points to a helper binary. Tuisual runs the binary, captures its stdout, and parses the JSON it emits.

```json
{
  "name": "desktop-dynamic",
  "short_flag": "a",
  "command": "./target/debug/desktop_apps_provider"
}
```

The binary must:
- only run when `TUISUAL_PROVIDER_MODE=1` is set (Tuisual sets this automatically)
- print a JSON array of items to stdout and nothing else
- exit 0 on success

Notes about the built-in PATH dynamic provider (`-p`):
- PATH commands now open a sub-item menu first instead of launching immediately
- use `Run Command` to execute the base command without extra flags
- it includes a `Custom Flags / Args` sub-item that prompts for freeform flags/arguments
- it auto-discovers additional flags from installed completion files, man pages, and docs metadata
- additional command-specific flags come only from the explicit catalog file at `providers/path_flags_catalog.json`

### Curated PATH flag catalog (no guesswork)

The PATH provider does not infer flags. It only adds command-specific flags that are explicitly declared in:

`providers/path_flags_catalog.json`

Each entry maps a command name to a list of exact flags:

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

Only valid explicit flags are accepted (`--long-flag` or short `-x`). Invalid entries are ignored.

### Auto-discovery sources

Auto-discovery reads metadata sources and does not execute or probe PATH binaries:

- `/usr/share/bash-completion/completions/<command>`
- `/usr/share/zsh/site-functions/_<command>`
- `/usr/share/fish/vendor_completions.d/<command>.fish`
- `man -- <command>` (when available)
- docs under `/usr/share/doc` and `/usr/local/share/doc` matching the command name

Discovered flags appear as `Auto <flag>` sub-items.
When available (especially from man-page option lines), the sub-item subtitle shows the flag description.

If completion/man/docs are missing for a command, no auto flags are added for that command.

Autodiscovery is bounded to keep startup responsive:
- `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_LIMIT` (default: `120`) max commands to attempt man/docs for when completion data is missing
- `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_BUDGET_MS` (default: `3000`) total man/docs discovery time budget per provider run
- `TUISUAL_PATH_AUTODISCOVER_DOC_FILES_LIMIT` (default: `8`) max doc files read per command

---

## Provider item schema

Each item in the JSON array must match this shape:

```json
{
  "id": "unique-slug",
  "title": "Display Name",
  "subtitle": "Short description",
  "info": {
    "summary": "Shown in the right info pane.",
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

### Action types

| Type                  | Behaviour                                                        |
|-----------------------|------------------------------------------------------------------|
| `shell_command`       | Run the command, then return to the TUI                          |
| `shell_command_exit`  | Run the command, then exit Tuisual                               |
| `shell_command_with_flag` | Enter compose mode — prompts for input, builds a flag, then runs |
| `provider_hint`       | Load a provider by name instead of running a command             |

### Sub-items

An item can have `sub_items` to create a drill-down menu. Set `require_sub_item: true` to force the user to pick a sub-item before the action runs.

```json
{
  "id": "my-tool",
  "title": "My Tool",
  "action": { "type": "shell_command_exit", "value": "mytool" },
  "require_sub_item": true,
  "sub_items": [
    {
      "id": "verbose",
      "title": "Verbose mode",
      "flags": ["--verbose"]
    }
  ]
}
```

### Input prompts (compose flow)

A sub-item can ask for freeform input at launch time:

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

This appends `name=<typed value>` to the parent command before running it.

---

## Writing a dynamic provider

1. Create a binary in `src/bin/my_provider.rs`.
2. Guard against direct invocation:
   ```rust
   if std::env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
       eprintln!("Run via Tuisual.");
       std::process::exit(2);
   }
   ```
3. Print a JSON array of items to stdout and exit 0.
4. Create `providers/my_provider.json`:
   ```json
   { "name": "my-provider", "short_flag": "m", "command": "./target/debug/my_provider" }
   ```
5. Run with `cargo run -- -m`.

---

## Environment variables

| Variable                  | Effect                                              |
|---------------------------|-----------------------------------------------------|
| `TUISUAL_PROVIDERS_DIR`   | Override the `providers/` directory path            |
| `TUISUAL_PROVIDER_MODE`   | Set to `1` by Tuisual when invoking a dynamic provider |
| `TUISUAL_PATH_FLAGS_CATALOG` | Override the path to the curated PATH flag catalog JSON |
| `TUISUAL_PATH_AUTODISCOVER` | Toggle completion-based auto-discovery (default: enabled, set `0`/`false`/`no`/`off` to disable) |
| `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_LIMIT` | Max commands to scan with man/docs when completion data is unavailable |
| `TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_BUDGET_MS` | Total time budget for man/docs discovery per run |
| `TUISUAL_PATH_AUTODISCOVER_DOC_FILES_LIMIT` | Max documentation files read per command during docs discovery |
