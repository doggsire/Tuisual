use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct InfoField {
	label: String,
	value: String,
}

#[derive(Debug, Serialize)]
struct ItemInfo {
	summary: String,
	fields: Vec<InfoField>,
}

#[derive(Debug, Serialize)]
struct Action {
	#[serde(rename = "type")]
	action_type: String,
	value: String,
}

#[derive(Debug, Serialize)]
struct ProviderItem {
	id: String,
	title: String,
	subtitle: String,
	info: ItemInfo,
	action: Action,
	#[serde(skip_serializing_if = "is_false")]
	require_sub_item: bool,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	sub_items: Vec<ActionSubItem>,
}

#[derive(Debug, Serialize)]
struct ActionSubItem {
	id: String,
	title: String,
	subtitle: String,
	flags: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	exit_after: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	input: Option<SubItemInput>,
}

#[derive(Debug, Serialize)]
struct SubItemInput {
	flag_prefix: String,
	prompt: String,
}

#[derive(Debug)]
struct PathCommand {
	name: String,
	path: PathBuf,
}

// This provider scans every executable in the PATH and turns it into a launchable item.
// This is how Tuisual can search for commands like git, python, or other installed tools.
fn main() {
	// The helper is meant to be called by Tuisual, which sets this environment variable.
	// Refuse direct execution so a user does not mistake diagnostic output for provider JSON.
	if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
		eprintln!(
			"This is a Tuisual provider helper. Run the app via 'tuisual -p' or 'cargo run --bin tuisual -- -p'."
		);
		std::process::exit(2);
	}

	// Discover executable files first so the output vector can reserve enough capacity.
	let commands = collect_path_commands();
	let mut items = Vec::with_capacity(commands.len());
	let mut seen_ids = HashSet::new();

	for command in commands {
		// Include both the command name and its path before slugifying. Two copies of a
		// command in different PATH directories must not accidentally share an ID.
		let mut id = slugify(&format!("{}-{}", command.name, command.path.display()));
		if id.is_empty() {
			id = "path-command".to_string();
		}

		if !seen_ids.insert(id.clone()) {
			// If the generated ID already exists, append an increasing suffix until the
			// candidate can be inserted into the set.
			let base = id.clone();
			let mut suffix = 2usize;
			let mut candidate = format!("{}-{}", base, suffix);
			while !seen_ids.insert(candidate.clone()) {
				suffix += 1;
				candidate = format!("{}-{}", base, suffix);
			}
			id = candidate;
		}

		// PATH commands do not provide predefined flag children; Tuisual can discover
		// those later when the user opens the command.
		let sub_items = Vec::new();

		// These fields become the details shown beside the command in the TUI.
		let fields = vec![
			InfoField {
				label: "Command".to_string(),
				value: command.name.clone(),
			},
			InfoField {
				label: "Path".to_string(),
				value: command.path.display().to_string(),
			},
		];

		// Convert the filesystem result into the JSON shape expected by Tuisual.
		items.push(ProviderItem {
			id,
			title: command.name.clone(),
			subtitle: command.path.display().to_string(),
			info: ItemInfo {
				summary: "Executable discovered in PATH.".to_string(),
				fields,
			},
			action: Action {
				action_type: "shell_command_exit".to_string(),
				value: shell_escape_path(&command.path),
			},
			require_sub_item: false,
			sub_items,
		});
	}

	// Serialize the complete list once. A serialization failure produces an empty JSON
	// array so the caller still receives valid JSON before the helper exits with failure.
	match serde_json::to_string(&items) {
		Ok(output) => println!("{}", output),
		Err(_) => {
			println!("[]");
			std::process::exit(1);
		}
	}
}

// Walk through every directory in PATH and collect files that are executable.
// We keep only one copy of each path so the same command is not listed more than once.
fn collect_path_commands() -> Vec<PathCommand> {
	let mut results = Vec::new();
	let mut seen_paths: HashSet<PathBuf> = HashSet::new();

	// Read PATH as an OS string because directory names may not be valid UTF-8.
	let Some(path_var) = env::var_os("PATH") else {
		return results;
	};

	// Split PATH using the platform-aware separator, then inspect each directory.
	for dir in env::split_paths(&path_var) {
		if !dir.exists() || !dir.is_dir() {
			continue;
		}

		// An unreadable directory should not prevent scanning the rest of PATH.
		let Ok(entries) = fs::read_dir(&dir) else {
			continue;
		};

		for entry in entries.flatten() {
			// Ignore directory-entry errors and consider only regular files.
			let path = entry.path();
			if !path.is_file() {
				continue;
			}

			// Permission bits, not the filename extension, decide whether this can run.
			if !is_executable(&path) {
				continue;
			}

			// Avoid emitting the same path twice if PATH contains duplicate directories.
			if !seen_paths.insert(path.clone()) {
				continue;
			}

			// A non-UTF-8 filename cannot be copied into the String-based JSON model.
			let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
				continue;
			};

			results.push(PathCommand {
				name: name.to_string(),
				path,
			});
		}
	}

	// Sort after scanning so output is deterministic even when the filesystem returns
	// directory entries in an arbitrary order.
	results.sort_by(|a, b| {
		a.name
			.cmp(&b.name)
			.then_with(|| a.path.as_os_str().cmp(b.path.as_os_str()))
	});

	results
}

// Check whether a file is executable by testing its Unix permission bits.
fn is_executable(path: &Path) -> bool {
	// Read metadata, take the Unix permission bits, and test any of the three execute
	// bits. Missing metadata is treated as not executable.
	fs::metadata(path)
		.map(|meta| meta.permissions().mode() & 0o111 != 0)
		.unwrap_or(false)
}

// Convert a command name into a stable, lowercase, URL-safe-ish ID.
fn slugify(text: &str) -> String {
	// Build the ID one character at a time. Letters and digits remain readable; every
	// run of other characters becomes one dash.
	let mut slug = String::with_capacity(text.len());
	let mut last_dash = false;

	for ch in text.chars() {
		if ch.is_ascii_alphanumeric() {
			slug.push(ch.to_ascii_lowercase());
			last_dash = false;
		} else if !last_dash {
			// Prevent repeated punctuation from creating repeated dashes.
			slug.push('-');
			last_dash = true;
		}
	}

	slug.trim_matches('-').to_string()
}

// Wrap a path in shell quotes so it is safe to pass to a shell command.
// This matters because a path could contain spaces or special characters.
fn shell_escape_path(path: &Path) -> String {
	// Convert the path to text, escape embedded single quotes using shell syntax, and
	// surround the result with single quotes so spaces stay inside one argument.
	let raw = path.display().to_string();
	format!("'{}'", raw.replace('\'', "'\\''"))
}

// Used by serde to skip boolean fields when they are false.
fn is_false(value: &bool) -> bool {
	// Serde calls this predicate to omit a boolean field when its value is false.
	!value
}
