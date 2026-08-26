use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

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

#[derive(Debug, Clone, Copy)]
enum CompletionFlavor {
	Bash,
	Zsh,
	Fish,
}

#[derive(Debug, Clone)]
struct CompletionSource {
	flavor: CompletionFlavor,
	path: PathBuf,
}

#[derive(Debug, Clone)]
struct AutoDiscoverConfig {
	enabled: bool,
	man_docs_command_limit: usize,
	man_docs_budget_ms: u64,
	doc_files_limit: usize,
}

#[derive(Debug, Default)]
struct MetadataIndex {
	man_commands: HashSet<String>,
	doc_directories: Vec<String>,
}

#[derive(Debug, Default)]
struct AutoDiscoveryResult {
	flags_by_command: HashMap<String, Vec<String>>,
	descriptions_by_command: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct PathFlagCatalog {
	#[serde(default)]
	commands: Vec<CatalogCommand>,
}

#[derive(Debug, Deserialize)]
struct CatalogCommand {
	command: String,
	#[serde(default)]
	flags: Vec<CatalogFlag>,
}

#[derive(Debug, Deserialize)]
struct CatalogFlag {
	flag: String,
	#[serde(default)]
	title: Option<String>,
	#[serde(default)]
	subtitle: Option<String>,
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

fn build_common_flag_sub_items() -> Vec<ActionSubItem> {
	let mut items = vec![ActionSubItem {
		id: "run".to_string(),
		title: "Run Command".to_string(),
		subtitle: "Launch without extra flags".to_string(),
		flags: vec![],
		exit_after: Some(true),
		input: None,
	}];

	items.push(ActionSubItem {
		id: "custom-flags".to_string(),
		title: "Custom Flags / Args".to_string(),
		subtitle: "Type any flags or arguments to append".to_string(),
		flags: vec![],
		exit_after: Some(true),
		input: Some(SubItemInput {
			flag_prefix: "".to_string(),
			prompt: "Enter flags/args".to_string(),
		}),
	});

	items
}

fn extend_with_discovered_flags(
	sub_items: &mut Vec<ActionSubItem>,
	discovered: &HashMap<String, Vec<String>>,
	descriptions: &HashMap<String, HashMap<String, String>>,
	command_name: &str,
) {
	let key = command_name.to_ascii_lowercase();
	let Some(flags) = discovered.get(&key) else {
		return;
	};
	let descriptions_for_command = descriptions.get(&key);

	let mut existing_flags: HashSet<String> = sub_items
		.iter()
		.flat_map(|item| item.flags.iter().cloned())
		.collect();

	for flag in flags {
		if !existing_flags.insert(flag.clone()) {
			continue;
		}

		let subtitle = descriptions_for_command
			.and_then(|items| items.get(flag))
			.cloned()
			.unwrap_or_else(|| "Discovered from completion/man/docs metadata".to_string());

		sub_items.push(ActionSubItem {
			id: format!("auto-{}", slugify(flag)),
			title: format!("Auto {}", flag),
			subtitle,
			flags: vec![flag.clone()],
			exit_after: Some(true),
			input: None,
		});
	}
}

fn extend_with_catalog_flags(
	sub_items: &mut Vec<ActionSubItem>,
	catalog: &HashMap<String, Vec<CatalogFlag>>,
	command_name: &str,
) {
	let key = command_name.to_ascii_lowercase();
	let Some(flags) = catalog.get(&key) else {
		return;
	};

	let mut existing_flags: HashSet<String> = sub_items
		.iter()
		.flat_map(|item| item.flags.iter().cloned())
		.collect();

	for entry in flags {
		let trimmed = entry.flag.trim();
		if !is_explicit_flag(trimmed) {
			continue;
		}

		if !existing_flags.insert(trimmed.to_string()) {
			continue;
		}

		sub_items.push(ActionSubItem {
			id: format!("catalog-{}", slugify(trimmed)),
			title: entry
				.title
				.clone()
				.unwrap_or_else(|| format!("Catalog {}", trimmed)),
			subtitle: entry
				.subtitle
				.clone()
				.unwrap_or_else(|| "Explicitly defined in path flag catalog".to_string()),
			flags: vec![trimmed.to_string()],
			exit_after: Some(true),
			input: None,
		});
	}
}

fn load_catalog_map() -> HashMap<String, Vec<CatalogFlag>> {
	let path = resolve_catalog_path();
	let Ok(content) = fs::read_to_string(&path) else {
		return HashMap::new();
	};

	let Ok(doc) = serde_json::from_str::<PathFlagCatalog>(&content) else {
		return HashMap::new();
	};

	let mut map: HashMap<String, Vec<CatalogFlag>> = HashMap::new();
	for command in doc.commands {
		let name = command.command.trim().to_ascii_lowercase();
		if name.is_empty() || command.flags.is_empty() {
			continue;
		}

		map.entry(name).or_default().extend(command.flags);
	}

	map
}

fn load_auto_flags_for_commands(commands: &[PathCommand]) -> AutoDiscoveryResult {
	let config = AutoDiscoverConfig::from_env();
	if !config.enabled {
		return AutoDiscoveryResult::default();
	}

	let mut result = AutoDiscoveryResult::default();
	let mut unique_names = HashSet::new();
	let metadata = build_metadata_index();
	let started_at = Instant::now();
	let mut man_docs_attempted = 0usize;

	for command in commands {
		unique_names.insert(command.name.to_ascii_lowercase());
	}

	let mut ordered_names: Vec<String> = unique_names.into_iter().collect();
	ordered_names.sort_by(|a, b| {
		discovery_priority(a)
			.cmp(&discovery_priority(b))
			.then_with(|| a.cmp(b))
	});

	for name in ordered_names {
		let mut flags = Vec::new();
		let mut seen = HashSet::new();
		let mut descriptions: HashMap<String, String> = HashMap::new();

		for flag in discover_flags_from_completions(&name) {
			if !is_explicit_flag(&flag) {
				continue;
			}
			if seen.insert(flag.clone()) {
				flags.push(flag);
			}
		}

		let can_use_man_docs = man_docs_attempted < config.man_docs_command_limit
			&& started_at.elapsed().as_millis() < u128::from(config.man_docs_budget_ms);

		if can_use_man_docs && flags.is_empty() {
			man_docs_attempted += 1;

			for (flag, description) in discover_flags_from_man_page(&name, &metadata) {
				if !is_explicit_flag(&flag) {
					continue;
				}
				if seen.insert(flag.clone()) {
					if let Some(value) = description {
						descriptions.insert(flag.clone(), value);
					}
					flags.push(flag);
				}
			}

			if flags.is_empty() {
				for flag in discover_flags_from_docs(&name, &metadata, config.doc_files_limit) {
					if !is_explicit_flag(&flag) {
						continue;
					}
					if seen.insert(flag.clone()) {
						flags.push(flag);
					}
				}
			}
		}

		if !flags.is_empty() {
			if !descriptions.is_empty() {
				result
					.descriptions_by_command
					.insert(name.clone(), descriptions);
			}
			result.flags_by_command.insert(name, flags);
		}
	}

	result
}

fn discovery_priority(command_name: &str) -> u8 {
	match command_name {
		"echo" | "ls" | "grep" | "cat" | "sed" | "awk" | "find" | "git" | "cargo"
		| "rg" | "python" | "python3" | "bash" | "sh" | "zsh" | "fish" => 0,
		_ => 1,
	}
}

impl AutoDiscoverConfig {
	fn from_env() -> Self {
		Self {
			enabled: env_toggle_default_true("TUISUAL_PATH_AUTODISCOVER"),
			man_docs_command_limit: env_usize(
				"TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_LIMIT",
				120,
			),
			man_docs_budget_ms: env_u64(
				"TUISUAL_PATH_AUTODISCOVER_MAN_DOCS_BUDGET_MS",
				3000,
			),
			doc_files_limit: env_usize("TUISUAL_PATH_AUTODISCOVER_DOC_FILES_LIMIT", 8),
		}
	}
}

fn env_toggle_default_true(name: &str) -> bool {
	match env::var(name) {
		Ok(value) => {
			let normalized = value.trim().to_ascii_lowercase();
			!matches!(normalized.as_str(), "0" | "false" | "no" | "off")
		}
		Err(_) => true,
	}
}

fn discover_flags_from_completions(command_name: &str) -> Vec<String> {
	let sources = completion_sources_for_command(command_name);
	if sources.is_empty() {
		return Vec::new();
	}

	let mut flags = Vec::new();
	let mut seen = HashSet::new();

	for source in sources {
		let Ok(content) = fs::read_to_string(&source.path) else {
			continue;
		};

		let extracted = extract_flags_from_completion_text(&content, source.flavor);
		for flag in extracted {
			if seen.insert(flag.clone()) {
				flags.push(flag);
			}
		}
 	}

	flags
}

fn env_u64(name: &str, fallback: u64) -> u64 {
	env::var(name)
		.ok()
		.and_then(|value| value.trim().parse::<u64>().ok())
		.filter(|value| *value > 0)
		.unwrap_or(fallback)
}

fn env_usize(name: &str, fallback: usize) -> usize {
	env::var(name)
		.ok()
		.and_then(|value| value.trim().parse::<usize>().ok())
		.filter(|value| *value > 0)
		.unwrap_or(fallback)
}

fn discover_flags_from_man_page(
	command_name: &str,
	metadata: &MetadataIndex,
) -> Vec<(String, Option<String>)> {
	if !metadata.man_commands.contains(command_name) {
		return Vec::new();
	}

	let output = Command::new("man")
		.env("MANPAGER", "cat")
		.env("PAGER", "cat")
		.arg("--")
		.arg(command_name)
		.output();

	let Ok(output) = output else {
		return Vec::new();
	};

	if !output.status.success() || output.stdout.is_empty() {
		return Vec::new();
	}

	let text = normalize_man_text(&String::from_utf8_lossy(&output.stdout));
	extract_flags_with_descriptions_from_man_text(&text)
}

fn discover_flags_from_docs(
	command_name: &str,
	metadata: &MetadataIndex,
	doc_files_limit: usize,
) -> Vec<String> {
	let mut flags = Vec::new();
	let mut seen = HashSet::new();

	for doc_path in collect_doc_files(command_name, metadata, doc_files_limit) {
		let Ok(content) = fs::read_to_string(&doc_path) else {
			continue;
		};

		for flag in extract_flags_from_plain_text(&content) {
			if seen.insert(flag.clone()) {
				flags.push(flag);
			}
		}
	}

	flags
}

fn collect_doc_files(command_name: &str, metadata: &MetadataIndex, limit: usize) -> Vec<PathBuf> {
	let mut files = Vec::new();
	for root in ["/usr/share/doc", "/usr/local/share/doc"] {
		let root_path = Path::new(root);
		let Ok(entries) = fs::read_dir(root_path) else {
			continue;
		};

		for entry in entries.flatten() {
			let path = entry.path();
			if !path.is_dir() {
				continue;
			}

			let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
				continue;
			};

			if !metadata.doc_directories.iter().any(|entry| entry == &name.to_ascii_lowercase()) {
				continue;
			}

			if !doc_dir_matches_command(name, command_name) {
				continue;
			}

			collect_text_files_in_dir(&path, &mut files, limit);
			if files.len() >= limit {
				return files;
			}
		}
	}

	files
}

fn build_metadata_index() -> MetadataIndex {
	let mut index = MetadataIndex::default();

	for root in ["/usr/share/man", "/usr/local/share/man"] {
		collect_man_command_names(Path::new(root), &mut index.man_commands);
	}

	for root in ["/usr/share/doc", "/usr/local/share/doc"] {
		let root_path = Path::new(root);
		let Ok(entries) = fs::read_dir(root_path) else {
			continue;
		};

		for entry in entries.flatten() {
			let path = entry.path();
			if !path.is_dir() {
				continue;
			}
			if let Some(name) = path.file_name().and_then(|v| v.to_str()) {
				index.doc_directories.push(name.to_ascii_lowercase());
			}
		}
	}

	index
}

fn collect_man_command_names(root: &Path, names: &mut HashSet<String>) {
	let Ok(section_dirs) = fs::read_dir(root) else {
		return;
	};

	for section_entry in section_dirs.flatten() {
		let section_path = section_entry.path();
		if !section_path.is_dir() {
			continue;
		}

		let Some(section_name) = section_path.file_name().and_then(|v| v.to_str()) else {
			continue;
		};

		if !section_name.starts_with("man") {
			continue;
		}

		let Ok(files) = fs::read_dir(&section_path) else {
			continue;
		};

		for file_entry in files.flatten() {
			let path = file_entry.path();
			if !path.is_file() {
				continue;
			}

			let Some(filename) = path.file_name().and_then(|v| v.to_str()) else {
				continue;
			};

			let base = filename
				.split('.')
				.next()
				.unwrap_or("")
				.trim()
				.to_ascii_lowercase();

			if !base.is_empty() {
				names.insert(base);
			}
		}
	}
}

fn collect_text_files_in_dir(dir: &Path, files: &mut Vec<PathBuf>, limit: usize) {
	if files.len() >= limit {
		return;
	}

	let Ok(entries) = fs::read_dir(dir) else {
		return;
	};

	for entry in entries.flatten() {
		if files.len() >= limit {
			return;
		}

		let path = entry.path();
		if path.is_dir() {
			collect_text_files_in_dir(&path, files, limit);
			continue;
		}

		if !is_doc_text_file(&path) {
			continue;
		}

		files.push(path);
	}
}

fn doc_dir_matches_command(dir_name: &str, command_name: &str) -> bool {
	let dir = dir_name.to_ascii_lowercase();
	let cmd = command_name.to_ascii_lowercase();

	dir == cmd || dir.starts_with(&format!("{}-", cmd)) || dir.starts_with(&format!("{}.", cmd))
}

fn is_doc_text_file(path: &Path) -> bool {
	const ALLOWED: &[&str] = &[
		"txt", "text", "md", "markdown", "rst", "1", "2", "3", "4", "5", "6", "7", "8",
		"info",
	];

	let Some(ext) = path.extension().and_then(|v| v.to_str()) else {
		return false;
	};

	ALLOWED.contains(&ext.to_ascii_lowercase().as_str())
}

fn normalize_man_text(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	for ch in text.chars() {
		if ch == '\u{0008}' {
			out.pop();
			continue;
		}
		if ch != '\r' {
			out.push(ch);
		}
	}
	out
}

fn extract_flags_from_plain_text(content: &str) -> Vec<String> {
	let mut flags = Vec::new();
	let mut seen = HashSet::new();

	for token in content.split(|ch: char| {
		ch.is_whitespace()
			|| matches!(
				ch,
				'"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '|' | '='
			)
	}) {
		let trimmed = token.trim();
		if !is_explicit_flag(trimmed) {
			continue;
		}
		if seen.insert(trimmed.to_string()) {
			flags.push(trimmed.to_string());
		}
	}

	flags
}

fn extract_flags_with_descriptions_from_man_text(content: &str) -> Vec<(String, Option<String>)> {
	let mut results: Vec<(String, Option<String>)> = Vec::new();
	let mut index_by_flag: HashMap<String, usize> = HashMap::new();
	let mut pending_flags: Vec<String> = Vec::new();

	for raw_line in content.lines() {
		let trimmed = raw_line.trim();

		if trimmed.is_empty() {
			pending_flags.clear();
			continue;
		}

		if trimmed.starts_with('-') {
			let (flag_section, inline_description) = split_option_line(trimmed);
			let mut current_flags = Vec::new();

			for token in flag_section.split(|ch| ch == ',' || ch == '|') {
				let Some(flag) = normalize_flag_token(token) else {
					continue;
				};

				if !is_explicit_flag(&flag) {
					continue;
				}

				if let Some(index) = index_by_flag.get(&flag).copied() {
					if results[index].1.is_none() && inline_description.is_some() {
						results[index].1 = inline_description.clone();
					}
				} else {
					index_by_flag.insert(flag.clone(), results.len());
					results.push((flag.clone(), inline_description.clone()));
				}

				current_flags.push(flag);
			}

			if inline_description.is_none() {
				pending_flags = current_flags;
			} else {
				pending_flags.clear();
			}

			continue;
		}

		if !pending_flags.is_empty()
			&& (raw_line.starts_with(' ') || raw_line.starts_with('\t'))
			&& !trimmed.chars().all(|ch| ch.is_ascii_uppercase() || ch == ' ')
		{
			let description = trimmed.to_string();
			for flag in pending_flags.drain(..) {
				if let Some(index) = index_by_flag.get(&flag).copied() {
					if results[index].1.is_none() {
						results[index].1 = Some(description.clone());
					}
				}
			}
		} else {
			pending_flags.clear();
		}
	}

	if results.is_empty() {
		let mut seen = HashSet::new();
		for flag in extract_flags_from_plain_text(content) {
			if seen.insert(flag.clone()) {
				results.push((flag, None));
			}
		}
	}

	results
}

fn split_option_line(line: &str) -> (&str, Option<String>) {
	let bytes = line.as_bytes();
	let mut idx = 0usize;
	while idx + 1 < bytes.len() {
		if bytes[idx] == b' ' && bytes[idx + 1] == b' ' {
			let flag_part = line[..idx].trim();
			let desc = line[idx..].trim();
			if desc.is_empty() {
				return (flag_part, None);
			}
			return (flag_part, Some(desc.to_string()));
		}
		idx += 1;
	}

	(line, None)
}

fn normalize_flag_token(token: &str) -> Option<String> {
	let mut value = token.trim();
	if value.is_empty() {
		return None;
	}

	if let Some((left, _)) = value.split_once('=') {
		value = left;
	}

	if let Some((left, _)) = value.split_once('[') {
		value = left;
	}

	value = value.trim_matches(|ch: char| {
		matches!(ch, ',' | ';' | ':' | '.' | '(' | ')' | '{' | '}' | '<' | '>')
	});

	if value.is_empty() {
		None
	} else {
		Some(value.to_string())
	}
}

fn completion_sources_for_command(command_name: &str) -> Vec<CompletionSource> {
	let mut sources = Vec::new();

	let bash = PathBuf::from("/usr/share/bash-completion/completions").join(command_name);
	if bash.exists() {
		sources.push(CompletionSource {
			flavor: CompletionFlavor::Bash,
			path: bash,
		});
	}

	let zsh = PathBuf::from("/usr/share/zsh/site-functions").join(format!("_{}", command_name));
	if zsh.exists() {
		sources.push(CompletionSource {
			flavor: CompletionFlavor::Zsh,
			path: zsh,
		});
	}

	let fish = PathBuf::from("/usr/share/fish/vendor_completions.d")
		.join(format!("{}.fish", command_name));
	if fish.exists() {
		sources.push(CompletionSource {
			flavor: CompletionFlavor::Fish,
			path: fish,
		});
	}

	sources
}

fn extract_flags_from_completion_text(content: &str, flavor: CompletionFlavor) -> Vec<String> {
	match flavor {
		CompletionFlavor::Fish => extract_flags_from_fish_completion(content),
		CompletionFlavor::Bash | CompletionFlavor::Zsh => extract_flags_from_generic_completion(content),
	}
}

fn extract_flags_from_generic_completion(content: &str) -> Vec<String> {
	let mut flags = Vec::new();
	let mut seen = HashSet::new();

	for token in content.split(|ch: char| {
		ch.is_whitespace()
			|| matches!(ch, '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '|')
	}) {
		let trimmed = token.trim();
		if !is_explicit_flag(trimmed) {
			continue;
		}
		if seen.insert(trimmed.to_string()) {
			flags.push(trimmed.to_string());
		}
	}

	flags
}

fn extract_flags_from_fish_completion(content: &str) -> Vec<String> {
	let mut flags = Vec::new();
	let mut seen = HashSet::new();

	for line in content.lines() {
		if !line.contains("complete") {
			continue;
		}

		let tokens: Vec<&str> = line.split_whitespace().collect();
		let mut index = 0usize;
		while index < tokens.len() {
			match tokens[index] {
				"-l" | "--long" => {
					if let Some(value) = tokens.get(index + 1) {
						let long_flag = format!("--{}", trim_completion_value(value));
						if is_explicit_flag(&long_flag) && seen.insert(long_flag.clone()) {
							flags.push(long_flag);
						}
					}
					index += 2;
					continue;
				}
				"-s" | "--short" => {
					if let Some(value) = tokens.get(index + 1) {
						let short_value = trim_completion_value(value);
						if short_value.len() == 1 {
							let short_flag = format!("-{}", short_value);
							if is_explicit_flag(&short_flag) && seen.insert(short_flag.clone()) {
								flags.push(short_flag);
							}
						}
					}
					index += 2;
					continue;
				}
				_ => {}
			}

			index += 1;
		}
	}

	flags
}

fn trim_completion_value(value: &str) -> String {
	value
		.trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'))
		.to_string()
}

fn resolve_catalog_path() -> PathBuf {
	if let Ok(path) = env::var("TUISUAL_PATH_FLAGS_CATALOG") {
		let trimmed = path.trim();
		if !trimmed.is_empty() {
			return PathBuf::from(trimmed);
		}
	}

	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("providers")
		.join("path_flags_catalog.json")
}

fn is_explicit_flag(flag: &str) -> bool {
	if flag == "-" || flag == "--" || !flag.starts_with('-') {
		return false;
	}

	if flag.starts_with("---") {
		return false;
	}

	if flag.starts_with("--") {
		return flag
			.chars()
			.skip(2)
			.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
	}

	if flag.len() == 2 {
		return flag
			.chars()
			.nth(1)
			.is_some_and(|ch| ch.is_ascii_alphanumeric());
	}

	false
}

fn is_false(value: &bool) -> bool {
	!value
}
