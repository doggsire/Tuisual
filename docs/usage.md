# Usage Guide

This guide covers how to run Tuisual day-to-day: launching it, navigating the interface, and the
command-line flags that control what shows up.

## Running Tuisual

If you installed it with `./install.sh`, just run:

```
tuisual
```

If you're running from a source checkout without installing, use:

```
cargo run
```

Running with no flags shows the **provider catalog** — a list of every provider (category of
items) that Tuisual knows about, along with the flag you'd use to load it directly.

After installation, add custom provider files to `~/.config/tuisual/providers/`. They are loaded
on the next run without rebuilding Tuisual. For a source checkout, add them to `providers/`, or
set `TUISUAL_PROVIDERS_DIR` to another directory.

## QuickShell launcher

Tuisual also includes a QuickShell version of the launcher for Wayland desktops. It uses the
same provider JSON files and helper binaries as the terminal UI.

After installing or rebuilding the main application, install the QuickShell configuration:

```
./install-quickshell.sh
```

Then launch the popup with:

```
quisual
```

The launcher is installed to `/usr/local/bin` by default. Set `BIN_DIR` before running the
installer to use a different location.

The QuickShell launcher supports searching, provider drill-down, sub-items, and input prompts.
Commands run through `sh -lc`; commands that exit Tuisual close the popup after succeeding.
The built-in `arch-updates` and `installer` providers launch commands in `alacritty` by default so
interactive package commands have a terminal. GUI actions that exit the launcher are fully
detached before the popup closes. Other providers run through `sh -lc`; configure their commands
to launch a terminal themselves when they need interactive terminal input.

QuickShell supports the same basic navigation as the terminal UI. Use `Tab` or `Shift+Tab` to
switch focus between Results and Info, `Up`/`Down` for the focused pane, and `PageUp`/`PageDown`
to scroll the Info pane. Its wrapper forwards provider flags, so `quisual -P` and
`quisual --path-launcher` load the PATH provider directly.

## Loading specific providers

Pass one or more flags to skip the catalog and load specific sets of items directly:

```
tuisual -l              # Desktop apps
tuisual -P              # PATH commands
tuisual -u              # Arch package updates
tuisual -p              # Power menu (lock/logout/restart/shutdown)
tuisual -i              # Package manager (install/search packages)
```

Every provider also has a long-form flag, shown in the catalog view (e.g. `--desktop-app-launcher`
instead of `-l`). Combine short flags to load more than one provider at once:

```
tuisual -lP              # desktop apps + PATH commands together
```

Run `tuisual -h` or `tuisual --help` at any time to print the list of available flags and exit.

The repository also contains `providers/example.json` as a reference for creating custom
providers. It is not installed as a normal provider; see the [Configuration Guide](configuration.md)
for the examples it demonstrates.

### Advanced command-line modes

These modes are primarily useful for integrations such as the QuickShell frontend:

| Option | Purpose |
|--------|---------|
| `--json` | Print the provider catalog or selected items as JSON instead of opening the TUI. |
| `--query TEXT` | Pass a search query to a provider that supports provider-side filtering, such as `--installer`. |
| `--path-flags COMMAND` | Print discovered PATH sub-items for a command as JSON. |

For example:

```
tuisual --json --installer --query firefox
tuisual --path-flags git
```

## Navigating the interface

| Key        | Action                                          |
|------------|--------------------------------------------------|
| Type       | Filter the result list (fuzzy search)             |
| `↑` / `↓`  | Move the selection up/down                        |
| `Tab` / `Shift+Tab` | Toggle focus between Results and Info panels |
| `PageUp` / `PageDown` | Scroll the Info panel when it is focused       |
| `Enter`    | Launch the selected item                          |
| `Space`    | Open sub-items, or begin a compose (input) flow   |
| `Esc`      | Go back a step / cancel / quit                    |
| `Ctrl-U`   | Clear the current search/input                    |
| `Ctrl-C`   | Quit immediately                                  |
| Mouse      | Scroll panels; click or double-click results     |

The right-hand panel always shows details about whichever item is currently highlighted, so you
can see what an action does before you run it.

### Sub-items

Some items open a small menu of related options instead of launching immediately — for example,
a PATH command might offer "Run Command" alongside a list of common flags. Press `Space` (or
`Enter`, depending on the item) to drill in, then pick the option you want.

The PATH provider always offers `Run Command` and `Custom Flags / Args`. Pressing `Space` on a
command also discovers flags from installed Bash, Zsh, or Fish completions, man pages, and local
documentation, then combines them with entries in `providers/path_flags_catalog.json`.

The package manager provider (`-i` / `--installer`) starts with an empty result list. Type a
package name to search installed and available pacman, AUR, and Flatpak packages; press `Enter`
to run the lookup when needed.

### Typing extra input

A few items ask you to type something before they run — for instance, a "Custom Flags / Args"
option that lets you type arbitrary arguments for a command. Just type your input and press
`Enter` to run it with what you typed appended.

## What happens after launching something

Depending on the item, Tuisual will either:

- Run the command and immediately **return you to the list** (so you can launch something else), or
- Run the command and **exit Tuisual** (used for things like opening a GUI app or shutting down).

If a command fails, Tuisual shows you the error output instead of silently exiting, so you can see
what went wrong.

## Next steps

Want to add your own searchable commands, apps, or scripts? See the
**[Configuration Guide](configuration.md)** for how the provider system works.
