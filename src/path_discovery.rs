use crate::models::{ActionSubItem, SubItemInput};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn discover_path_sub_items(command_name: &str) -> Vec<ActionSubItem> {
    let mut sub_items = vec![ActionSubItem {
        id: "run".to_string(),
        title: "Run Command".to_string(),
        subtitle: "Launch without extra flags".to_string(),
        flags: vec![],
        exit_after: Some(true),
        input: None,
        require_sub_item: false,
        sub_items: vec![],
    }];

    let discovered = discover_auto_flags(command_name);
    let catalog = load_catalog_flags(command_name);

    let mut seen_flags: HashSet<String> = HashSet::new();
    for item in &sub_items {
        seen_flags.extend(item.flags.iter().cloned());
    }

    append_auto_flags(&mut sub_items, &mut seen_flags, discovered);
    append_catalog_flags(&mut sub_items, &mut seen_flags, catalog);

    sub_items.push(ActionSubItem {
        id: "custom-flags".to_string(),
        title: "Custom Flags / Args".to_string(),
        subtitle: "Type any flags or arguments to append".to_string(),
        flags: vec![],
        exit_after: Some(true),
        input: Some(SubItemInput {
            flag_prefix: "".to_string(),
            prompt: "Enter flags/args".to_string(),
        }),
        require_sub_item: false,
        sub_items: vec![],
    });

    sub_items
}

fn append_auto_flags(
    sub_items: &mut Vec<ActionSubItem>,
    seen_flags: &mut HashSet<String>,
    discovered: HashMap<String, Option<String>>,
) {
    for (flag, description) in discovered {
        if !seen_flags.insert(flag.clone()) {
            continue;
        }

        let title = flag.clone();
        sub_items.push(ActionSubItem {
            id: format!("auto-{}", slugify(&flag)),
            title,
            subtitle: description.unwrap_or_else(|| "Discovered from completion/man/docs metadata".to_string()),
            flags: vec![flag],
            exit_after: Some(true),
            input: None,
            require_sub_item: false,
            sub_items: vec![],
        });
    }
}

fn append_catalog_flags(
    sub_items: &mut Vec<ActionSubItem>,
    seen_flags: &mut HashSet<String>,
    flags: Vec<CatalogFlag>,
) {
    for entry in flags {
        let trimmed = entry.flag.trim();
        if !is_explicit_flag(trimmed) {
            continue;
        }

        if !seen_flags.insert(trimmed.to_string()) {
            continue;
        }

        sub_items.push(ActionSubItem {
            id: format!("catalog-{}", slugify(trimmed)),
            title: entry.title.unwrap_or_else(|| trimmed.to_string()),
            subtitle: entry
                .subtitle
                .unwrap_or_else(|| "Explicitly defined in path flag catalog".to_string()),
            flags: vec![trimmed.to_string()],
            exit_after: Some(true),
            input: None,
            require_sub_item: false,
            sub_items: vec![],
        });
    }
}

fn discover_auto_flags(command_name: &str) -> HashMap<String, Option<String>> {
    let mut flags = HashMap::new();
    let mut seen = HashSet::new();

    for flag in discover_flags_from_completions(command_name) {
        if is_explicit_flag(&flag) && seen.insert(flag.clone()) {
            flags.insert(flag, None);
        }
    }

    for (flag, description) in discover_flags_from_man_page(command_name) {
        if is_explicit_flag(&flag) && seen.insert(flag.clone()) {
            flags.insert(flag, description);
        }
    }

    for flag in discover_flags_from_docs(command_name) {
        if is_explicit_flag(&flag) && seen.insert(flag.clone()) {
            flags.insert(flag, None);
        }
    }

    flags
}

fn discover_flags_from_completions(command_name: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    for source in completion_sources_for_command(command_name) {
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

fn discover_flags_from_man_page(command_name: &str) -> Vec<(String, Option<String>)> {
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

fn discover_flags_from_docs(command_name: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    for doc_path in collect_doc_files(command_name, 12) {
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

fn load_catalog_flags(command_name: &str) -> Vec<CatalogFlag> {
    let path = resolve_catalog_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    let Ok(doc) = serde_json::from_str::<PathFlagCatalog>(&content) else {
        return Vec::new();
    };

    let name = command_name.trim().to_ascii_lowercase();
    let mut flags = Vec::new();
    for command in doc.commands {
        if command.command.trim().to_ascii_lowercase() == name {
            flags.extend(command.flags);
        }
    }

    flags
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

fn collect_doc_files(command_name: &str, limit: usize) -> Vec<PathBuf> {
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
