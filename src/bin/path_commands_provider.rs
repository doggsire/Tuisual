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

fn main() {
	if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
		eprintln!(
			"This is a Tuisual provider helper. Run the app via 'tuisual -p' or 'cargo run --bin tuisual -- -p'."
		);
		std::process::exit(2);
	}

	let commands = collect_path_commands();
	let mut items = Vec::with_capacity(commands.len());
	let mut seen_ids = HashSet::new();

	for command in commands {
		let mut id = slugify(&format!("{}-{}", command.name, command.path.display()));
		if id.is_empty() {
			id = "path-command".to_string();
		}

		if !seen_ids.insert(id.clone()) {
			let base = id.clone();
			let mut suffix = 2usize;
			let mut candidate = format!("{}-{}", base, suffix);
			while !seen_ids.insert(candidate.clone()) {
				suffix += 1;
				candidate = format!("{}-{}", base, suffix);
			}
			id = candidate;
		}

		let sub_items = Vec::new();

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

	match serde_json::to_string(&items) {
		Ok(output) => println!("{}", output),
		Err(_) => {
			println!("[]");
			std::process::exit(1);
		}
	}
}

fn collect_path_commands() -> Vec<PathCommand> {
	let mut results = Vec::new();
	let mut seen_paths: HashSet<PathBuf> = HashSet::new();

	let Some(path_var) = env::var_os("PATH") else {
		return results;
	};

	for dir in env::split_paths(&path_var) {
		if !dir.exists() || !dir.is_dir() {
			continue;
		}

		let Ok(entries) = fs::read_dir(&dir) else {
			continue;
		};

		for entry in entries.flatten() {
			let path = entry.path();
			if !path.is_file() {
				continue;
			}

			if !is_executable(&path) {
				continue;
			}

			if !seen_paths.insert(path.clone()) {
				continue;
			}

			let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
				continue;
			};

			results.push(PathCommand {
				name: name.to_string(),
				path,
			});
		}
	}

	results.sort_by(|a, b| {
		a.name
			.cmp(&b.name)
			.then_with(|| a.path.as_os_str().cmp(b.path.as_os_str()))
	});

	results
}

fn is_executable(path: &Path) -> bool {
	fs::metadata(path)
		.map(|meta| meta.permissions().mode() & 0o111 != 0)
		.unwrap_or(false)
}

fn slugify(text: &str) -> String {
	let mut slug = String::with_capacity(text.len());
	let mut last_dash = false;

	for ch in text.chars() {
		if ch.is_ascii_alphanumeric() {
			slug.push(ch.to_ascii_lowercase());
			last_dash = false;
		} else if !last_dash {
			slug.push('-');
			last_dash = true;
		}
	}

	slug.trim_matches('-').to_string()
}

fn shell_escape_path(path: &Path) -> String {
	let raw = path.display().to_string();
	format!("'{}'", raw.replace('\'', "'\\''"))
}

fn is_false(value: &bool) -> bool {
	!value
}
