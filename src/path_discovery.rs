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

// This is the "smart helper" for path-based commands.
// It looks around the system for likely flags and turns them into friendly
// menu choices that the user can pick instead of typing everything by hand.
//
// Step by step, the flow is:
// 1. always add a default "run with no extra flags" option
// 2. add a "custom flags / args" option so the user can type anything
// 3. scan the system for discovered flags from completions, man pages, docs, and catalogs
// 4. remove duplicates so the menu does not show the same flag twice
// 5. turn each real flag into a child menu item that can be clicked
//
// This makes the app feel like a guided teacher: it offers safe, real flag choices instead of
// forcing the user to memorize command syntax.
pub fn discover_path_sub_items(command_name: &str) -> Vec<ActionSubItem> {
    // The first option is always the simple safe version: run the command exactly as-is.
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

    // Ask several discovery sources where a real command might keep extra flag information.
    let discovered = discover_auto_flags(command_name);
    let catalog = load_catalog_flags(command_name);

    // This set remembers flags we have already added, so we do not show duplicates.
    let mut seen_flags: HashSet<String> = HashSet::new();
    for item in &sub_items {
        seen_flags.extend(item.flags.iter().cloned());
    }

    // A user may want to type something the system did not discover automatically.
    // So we always include an input-based option that allows arbitrary arguments.
    sub_items.insert(
        1,
        ActionSubItem {
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
        },
    );

    append_auto_flags(&mut sub_items, &mut seen_flags, discovered);
    append_catalog_flags(&mut sub_items, &mut seen_flags, catalog);

    sub_items
}

// This helper adds the flags discovered from shell completion files and man pages.
// Each one becomes a menu item with a label and an action that appends the flag to the command.
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
    // Catalog entries are curated, but they still pass through the same shape and
    // duplicate checks as automatically discovered flags.
    for entry in flags {
        let trimmed = entry.flag.trim();
        // Ignore catalog mistakes that do not look like real flags.
        if !is_explicit_flag(trimmed) {
            continue;
        }

        // A flag found earlier should only appear once in the submenu.
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

// Try to find flags from several sources:
// shell completion files, man pages, and plain docs on the system.
// The result is a map of flag -> optional description.
//
// Why do this? We are trying to be helpful, not just show a blank command input.
// If the user chooses a path-based command like `git`, the system might be able to say,
// "This command supports --help, --version, --config, etc." and present them as clickable buttons.
fn discover_auto_flags(command_name: &str) -> HashMap<String, Option<String>> {
    let mut flags = HashMap::new();
    let mut seen = HashSet::new();

    // Add completion flags first because they are usually the most direct machine-readable source.
    for flag in discover_flags_from_completions(command_name) {
        if is_explicit_flag(&flag) && seen.insert(flag.clone()) {
            flags.insert(flag, None);
        }
    }

    // Man pages can provide descriptions, so keep those descriptions when adding new flags.
    for (flag, description) in discover_flags_from_man_page(command_name) {
        if is_explicit_flag(&flag) && seen.insert(flag.clone()) {
            flags.insert(flag, description);
        }
    }

    // Plain documentation is the final automatic source. It usually gives names without
    // descriptions, and the set prevents it from replacing stronger earlier results.
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

    // Check every completion file that exists for this command and skip files that cannot be read.
    for source in completion_sources_for_command(command_name) {
        let Ok(content) = fs::read_to_string(&source.path) else {
            continue;
        };

        let extracted = extract_flags_from_completion_text(&content, source.flavor);
        // Merge flags from this source while preserving the first-seen order.
        for flag in extracted {
            if seen.insert(flag.clone()) {
                flags.push(flag);
            }
        }
    }

    flags
}

// Ask the system's man page for a command and read its flag list.
// This is useful because many CLIs describe their flags in a human-readable way in the man page,
// even when their shell completions are incomplete or missing.
fn discover_flags_from_man_page(command_name: &str) -> Vec<(String, Option<String>)> {
    // Ask `man` to print directly to stdout instead of opening an interactive pager.
    let output = Command::new("man")
        .env("MANPAGER", "cat")
        .env("PAGER", "cat")
        .stdin(std::process::Stdio::null())
        .arg("--")
        .arg(command_name)
        .output();

    // If the command is missing or could not be started, treat the man page as unavailable.
    let Ok(output) = output else {
        return Vec::new();
    };

    // A failed command or empty output cannot provide useful flag information.
    if !output.status.success() || output.stdout.is_empty() {
        return Vec::new();
    }

    let text = normalize_man_text(&String::from_utf8_lossy(&output.stdout));
    extract_flags_with_descriptions_from_man_text(&text)
}

// Fall back to local documentation files when completions and man pages do not help.
// Many programs ship docs in /usr/share/doc, and those docs often say "--help" or "--output"
// in plain text. We scan a few likely files and pull out the flag names.
fn discover_flags_from_docs(command_name: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    // Limit the number of files so a broad system documentation tree does not make
    // opening a submenu unexpectedly slow.
    for doc_path in collect_doc_files(command_name, 12) {
        let Ok(content) = fs::read_to_string(&doc_path) else {
            continue;
        };

        for flag in extract_flags_from_plain_text(&content) {
            // Keep only the first occurrence from all matching documentation files.
            if seen.insert(flag.clone()) {
                flags.push(flag);
            }
        }
    }

    flags
}

// Read the project-owned flag catalog if it exists.
// This file is a curated list of known-good command flags, and it acts like a backup source of truth
// when the system is missing metadata or the app wants explicitly defined entries.
fn load_catalog_flags(command_name: &str) -> Vec<CatalogFlag> {
    // Missing or malformed catalog files are optional failures. Automatic discovery can
    // still continue, so return an empty list instead of stopping the whole menu.
    let path = resolve_catalog_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    let Ok(doc) = serde_json::from_str::<PathFlagCatalog>(&content) else {
        return Vec::new();
    };

    let name = command_name.trim().to_ascii_lowercase();
    let mut flags = Vec::new();
    // A catalog may contain more than one entry for a command, so collect flags from
    // every matching command record.
    for command in doc.commands {
        if command.command.trim().to_ascii_lowercase() == name {
            flags.extend(command.flags);
        }
    }

    flags
}

// Look in the common places where shell completion files live.
// Different shells keep their completion scripts in different folders.
//
// This is effectively a search for "where does this shell store the available flags for this command?"
// and then we read those scripts to extract the flag names.
fn completion_sources_for_command(command_name: &str) -> Vec<CompletionSource> {
    let mut sources = Vec::new();

    // Bash names its completion file exactly after the command.
    let bash = PathBuf::from("/usr/share/bash-completion/completions").join(command_name);
    if bash.exists() {
        sources.push(CompletionSource {
            flavor: CompletionFlavor::Bash,
            path: bash,
        });
    }

    // Zsh uses an underscore before the command name in its completion function file.
    let zsh = PathBuf::from("/usr/share/zsh/site-functions").join(format!("_{}", command_name));
    if zsh.exists() {
        sources.push(CompletionSource {
            flavor: CompletionFlavor::Zsh,
            path: zsh,
        });
    }

    // Fish stores completion files with the command name followed by `.fish`.
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

// Given a completion script, pull out the flag names that are likely valid.
// This is a simple text scan: we look for things that begin with '-' and keep the good ones.
//
// The completion script is basically a list of instructions for the shell. We are not trying to execute it;
// we are just looking for obvious flag names like --help, -l, or --config.
fn extract_flags_from_completion_text(content: &str, flavor: CompletionFlavor) -> Vec<String> {
    // Fish explicitly labels long and short options, while Bash and Zsh are handled
    // by the more general token scanner.
    match flavor {
        CompletionFlavor::Fish => extract_flags_from_fish_completion(content),
        CompletionFlavor::Bash | CompletionFlavor::Zsh => extract_flags_from_generic_completion(content),
    }
}

// Generic shell completion files are usually a long stream of text with command names and flag tokens.
// This function splits the text into smaller bits and keeps only the ones that look like actual flags.
fn extract_flags_from_generic_completion(content: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    // Split on whitespace and common script punctuation so quoted flag tokens become
    // individual candidates instead of remaining attached to shell syntax.
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

// Fish completions use a different syntax.
// They often write lines like "complete -c git -n ... -l ..." where the flag is described explicitly.
// This function looks for those flag definitions and extracts the real names.
fn extract_flags_from_fish_completion(content: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    // Fish completion syntax is line-oriented, so inspect one completion command at a time.
    for line in content.lines() {
        if !line.contains("complete") {
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        let mut index = 0usize;
        // Walk the tokens and look for the `-l`/`--long` and `-s`/`--short` markers.
        while index < tokens.len() {
            match tokens[index] {
                "-l" | "--long" => {
                    // The token after a long-option marker is the option name.
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
                    // The token after a short-option marker must be exactly one character.
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

// This is the trickiest part of the file.
// Man pages are written in plain text, and the flag definitions are often spread out
// across several lines. We scan line by line and keep track of the flag names that
// were just seen so we can attach the right description to them later.
//
// Example:
//   -h, --help
//       Show the help page
//
// The code reads the first line, sees "-h" and "--help", then remembers that the next
// indented line is the description for those flags.
fn extract_flags_with_descriptions_from_man_text(content: &str) -> Vec<(String, Option<String>)> {
    let mut results: Vec<(String, Option<String>)> = Vec::new();
    let mut index_by_flag: HashMap<String, usize> = HashMap::new();
    let mut pending_flags: Vec<String> = Vec::new();

    // Read the man page in order. `pending_flags` connects an option line to a
    // description printed on the following indented line.
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            // A blank line ends the relationship between an option and its description.
            pending_flags.clear();
            continue;
        }

        if trimmed.starts_with('-') {
            // This line begins a new option declaration. Extract all flags on it and
            // remember their positions in the result list.
            let (flag_section, inline_description) = split_option_line(trimmed);
            let mut current_flags = Vec::new();

            for token in flag_section.split([',', '|']) {
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
                // No description was on this line, so an indented following line may describe it.
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
            // Attach one shared description to every flag from the preceding option line.
            let description = trimmed.to_string();
            for flag in pending_flags.drain(..) {
                if let Some(index) = index_by_flag.get(&flag).copied()
                    && results[index].1.is_none()
                {
                    results[index].1 = Some(description.clone());
                }
            }
        } else {
            // A non-indented or unrelated line ends the pending description.
            pending_flags.clear();
        }
    }

    if results.is_empty() {
        // If the structured man-page scan found nothing, use the looser plain-text scanner
        // so unusual man-page formatting still has a chance to produce flags.
        let mut seen = HashSet::new();
        for flag in extract_flags_from_plain_text(content) {
            if seen.insert(flag.clone()) {
                results.push((flag, None));
            }
        }
    }

    results
}

// A man-page option line often looks like this:
// "  -h, --help   Show help"
// This helper splits the string into the part that is the flags and the part that is the description.
fn split_option_line(line: &str) -> (&str, Option<String>) {
    // Two spaces are treated as the visual boundary between an option declaration and
    // its description in typical formatted man-page output.
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

// Clean up a token from a man page so it becomes a usable flag string.
// For example, remove trailing punctuation like commas or brackets and ignore silly fragments.
fn normalize_flag_token(token: &str) -> Option<String> {
    let mut value = token.trim();
    if value.is_empty() {
        return None;
    }

    // Remove value examples such as `--output=file` and keep only `--output`.
    if let Some((left, _)) = value.split_once('=') {
        value = left;
    }

    // Remove optional-value notation such as `--color[=WHEN]`.
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

// Completion files may quote values like "--color" or "-n".
// This helper strips off the quote marks and punctuation so we keep only the real flag.
fn trim_completion_value(value: &str) -> String {
    value
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'))
        .to_string()
}

// Pick the catalog file path, or fall back to the app's bundled catalog.
fn resolve_catalog_path() -> PathBuf {
    // An environment variable lets users point at a custom catalog. Ignore an empty
    // value so the bundled project catalog remains the fallback.
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

// Search likely documentation directories for files related to the command.
// This is a broad but useful search: the app only grabs a small number of likely doc files to save time.
fn collect_doc_files(command_name: &str, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    // Search both system-wide and locally installed documentation roots.
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

            // Recursively collect text files from a matching package directory.
            collect_text_files_in_dir(&path, &mut files, limit);
            if files.len() >= limit {
                return files;
            }
        }
    }

    files
}

// Walk a documentation folder recursively and keep only text-like files.
// This lets us inspect docs without reading every file on the machine.
fn collect_text_files_in_dir(dir: &Path, files: &mut Vec<PathBuf>, limit: usize) {
    // Stop both at the global limit and at directories that cannot be read.
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
            // Visit nested documentation folders using the same rules.
            collect_text_files_in_dir(&path, files, limit);
            continue;
        }

        if !is_doc_text_file(&path) {
            continue;
        }

        files.push(path);
    }
}

// Decide whether a documentation directory likely belongs to this command.
// For example, a directory named "git" or "git-doc" is probably relevant to the git command.
fn doc_dir_matches_command(dir_name: &str, command_name: &str) -> bool {
    let dir = dir_name.to_ascii_lowercase();
    let cmd = command_name.to_ascii_lowercase();

    dir == cmd || dir.starts_with(&format!("{}-", cmd)) || dir.starts_with(&format!("{}.", cmd))
}

// Only read file types that are likely text docs.
// This keeps the scan fast and avoids trying to parse binary or image files.
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

// Man pages are noisy.
// This function cleans out backspace characters and carriage returns so the flag text is easier to read.
//
// A plain man page is full of formatting characters and control codes. This function removes the junk
// so the actual flag names stand out clearly in the text.
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

// From a plain text doc, grab any tokens that look like real command flags.
// This is a loose scan, but it catches many flags that are written in prose or examples.
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

// This decides whether a string is actually a real command flag and not just random text.
//
// We want to accept values like:
// - "--help"
// - "-p"
//
// But not things like:
// - "-"
// - "---"
// - text without a dash
//
// This is the safety check that keeps garbage text out of the menu.
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

// Turn a string into a safe, lowercase ID-like version.
// This is used in menu items so IDs do not contain weird punctuation or uppercase characters.
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
