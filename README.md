# Tuisual

Tuisual is a fast, keyboard-driven launcher that runs in your terminal. Instead of digging through
menus, you type a few letters and instantly find and run what you're looking for — an app, a
command, a system action, or anything else you've told it about.

Think of it like a command palette (the kind you find in modern editors) but for your whole
desktop, running directly in a terminal window.

## What it can do out of the box

- **Launch desktop apps** — search and open any installed application.
- **Run PATH commands** — search every command available on your system and run it, with or
  without extra flags.
- **Check for updates** — trigger `pacman` / `paru` / `flatpak` update actions.
- **Power menu** — lock, log out, restart, or shut down.
- **Anything you add yourself** — Tuisual is built around a simple provider system, so you can add
  your own custom searchable actions without touching the Rust source code.

## Why use it

- **Fast** — fuzzy search filters results as you type.
- **Informative** — a side panel shows details about whatever is currently selected.
- **Extensible** — new actions are added by writing a small JSON file, no need to recompile.
- **Scriptable** — every action is just a shell command under the hood, so if you can script it,
  Tuisual can launch it.

## Getting started

If you want to install directly from GitHub without cloning the repo first:

```
curl -fsSL https://raw.githubusercontent.com/doggsire/Tuisual/main/install.sh | bash
```

Or, if you prefer the local checkout flow:

```
git clone <this repo>
cd Tuisual
./install.sh
```

This builds Tuisual and installs it, along with its helper programs, to your system. Then just run:

```
tuisual
```

For Tuisual's QuickShell frontend, launched as `quisual` on Wayland, install the optional
frontend (QuickShell's `qs` command is required) and run it with:

```
curl -fsSL https://raw.githubusercontent.com/doggsire/Tuisual/main/install-quickshell.sh | bash
```
```
quisual
```

You can also run the script locally after cloning:

```
./install-quickshell.sh
quisual
```

## Learn more

- **[Usage Guide](docs/usage.md)** — how to run Tuisual, the keybindings, and the available
  command-line flags.
- **[Configuration Guide](docs/configuration.md)** — how the provider system works, and how to
  add or customize your own searchable actions.
