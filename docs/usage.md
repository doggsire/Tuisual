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

## QuickShell launcher

Tuisual also includes a QuickShell version of the launcher for Wayland desktops. It uses the
same provider JSON files and helper binaries as the terminal UI.

After installing or rebuilding the main application, install the QuickShell configuration:

```
./install-quickshell.sh
```

Then launch the popup with:

```
tuisual-qs
```

The launcher is installed to `/usr/local/bin` by default. Set `BIN_DIR` before running the
installer to use a different location.

The QuickShell launcher supports searching, provider drill-down, sub-items, and input prompts.
Commands run through `sh -lc`; commands that exit Tuisual close the popup after succeeding.
Interactive terminal programs should remain in the terminal launcher or be configured to start
in your preferred terminal emulator.

## Loading specific providers

Pass one or more flags to skip the catalog and load specific sets of items directly:

```
tuisual -a              # Desktop apps
tuisual -P              # PATH commands
tuisual -u              # Arch package updates
tuisual -p              # Power menu (lock/logout/restart/shutdown)
tuisual -i              # Package manager (install/search packages)
tuisual -x              # Example provider (for testing/reference)
```

Every provider also has a long-form flag, shown in the catalog view (e.g. `--desktop-app-launcher`
instead of `-l`). Combine short flags to load more than one provider at once:

```
tuisual -aP              # desktop apps + PATH commands together
```

Run `tuisual -h` or `tuisual --help` at any time to print the list of available flags and exit.

## Navigating the interface

| Key        | Action                                          |
|------------|--------------------------------------------------|
| Type       | Filter the result list (fuzzy search)             |
| `↑` / `↓`  | Move the selection up/down                        |
| `Enter`    | Launch the selected item                          |
| `Space`    | Open sub-items, or begin a compose (input) flow   |
| `Esc`      | Go back a step / cancel / quit                    |
| `Ctrl-C`   | Quit immediately                                  |

The right-hand panel always shows details about whichever item is currently highlighted, so you
can see what an action does before you run it.

### Sub-items

Some items open a small menu of related options instead of launching immediately — for example,
a PATH command might offer "Run Command" alongside a list of common flags. Press `Space` (or
`Enter`, depending on the item) to drill in, then pick the option you want.

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
