use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

// This file is a provider helper for desktop apps.
// It scans the system for .desktop files, reads the app metadata from them,
// and turns each app into a JSON item that Tuisual can display and launch.

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
}

#[derive(Debug)]
struct DesktopEntry {
    name: String,
    exec: String,
    comment: String,
    generic_name: String,
    keywords: Vec<String>,
    startup_wm_class: String,
    icon: String,
    desktop_id: String,
    path: PathBuf,
}

// Main entry point for the desktop app provider.
// This code only runs when the app calls it in provider mode, not when it is used directly
// by a person. Its job is to build a JSON list of desktop apps.
//
// The idea is simple:
// 1. find every .desktop launcher on the machine
// 2. read the launcher metadata from each file
// 3. turn that metadata into a menu item
// 4. generate a shell command that launches the app when the user presses Enter
// 5. print the whole list as JSON so Tuisual can display it
fn main() {
    // Only the parent Tuisual process should consume this helper's JSON output.
    // Without provider mode, print a diagnostic instead of emitting incomplete data.
    if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
        eprintln!(
            "This is a Tuisual provider helper. Run the app via 'tuisual -l' or 'cargo run --bin tuisual -- -l'."
        );
        std::process::exit(2);
    }

    // Start with an empty list of app items and a set of IDs we have already used,
    // so we can avoid duplicate names.
    // Keep output items and used IDs separately because an app name may occur in more
    // than one desktop file.
    let mut items = Vec::new();
    let mut seen_ids = HashSet::new();

    // Walk every .desktop file the system has and convert it into a Tuisual item.
    // Each file is like a tiny app description sheet for Linux desktops.
    for desktop_file in collect_desktop_files() {
        // Readable files are candidates; unreadable files are skipped independently.
        let Ok(content) = fs::read_to_string(&desktop_file) else {
            continue;
        };

        // Each desktop file is parsed into a cleaner structure that we can use.
        // Parsing also applies visibility and type rules, so a missing entry means skip it.
        let Some(entry) = parse_desktop_entry(&content, desktop_file.clone()) else {
            continue;
        };

        // Build a stable ID from the app name. If two apps share the same name,
        // add a number so the IDs stay unique.
        // Slugify the visible name for a stable JSON ID, then disambiguate duplicates.
        let mut item_id = slugify(&entry.name);
        if !seen_ids.insert(item_id.clone()) {
            // Keep adding suffixes until this desktop file gets a unique ID.
            let base = item_id.clone();
            let mut suffix = 2usize;
            let mut candidate = format!("{}-{}", base, suffix);
            while !seen_ids.insert(candidate.clone()) {
                suffix += 1;
                candidate = format!("{}-{}", base, suffix);
            }
            item_id = candidate;
        }

        // The subtitle is the small summary shown under an app title.
        // If the desktop file has a comment, show that. Otherwise use a generic label.
        // Prefer the desktop file's comment for the list subtitle and use a fallback when absent.
        let subtitle = if entry.comment.trim().is_empty() {
            "Launch desktop application".to_string()
        } else {
            entry.comment.clone()
        };

        // Add search aliases so the app can be discovered by things like browser, browser name,
        // file manager names, or keyword terms. This helps fuzzy matching work nicely.
        // Add searchable metadata to the subtitle so the matcher can find related terms.
        let aliases = build_aliases(&entry);
        let enriched_subtitle = if aliases.is_empty() {
            subtitle
        } else {
            format!("{} | terms: {}", subtitle, aliases.join(", "))
        };

        // Build the detail panel contents for the app.
        // This gives more information in the app's info pane when the user selects the app.
        // Start the info panel with fields that every valid desktop entry has.
        let mut fields = vec![
            InfoField {
                label: "Desktop File".to_string(),
                value: entry.path.display().to_string(),
            },
            InfoField {
                label: "Exec".to_string(),
                value: entry.exec.clone(),
            },
            InfoField {
                label: "Desktop ID".to_string(),
                value: entry.desktop_id.clone(),
            },
        ];

        if !entry.generic_name.is_empty() {
            // Optional desktop metadata is added only when it contains real text.
            fields.push(InfoField {
                label: "Generic Name".to_string(),
                value: entry.generic_name.clone(),
            });
        }
        if !entry.startup_wm_class.is_empty() {
            fields.push(InfoField {
                label: "Startup WM Class".to_string(),
                value: entry.startup_wm_class.clone(),
            });
        }
        if !entry.icon.is_empty() {
            fields.push(InfoField {
                label: "Icon".to_string(),
                value: entry.icon.clone(),
            });
        }
        if !entry.keywords.is_empty() {
            fields.push(InfoField {
                label: "Keywords".to_string(),
                value: entry.keywords.join(", "),
            });
        }
        if !aliases.is_empty() {
            fields.push(InfoField {
                label: "Search Terms".to_string(),
                value: aliases.join(", "),
            });
        }

        // Store the sanitized metadata and detached launch command in the provider schema.
        items.push(ProviderItem {
            id: item_id,
            title: entry.name,
            subtitle: enriched_subtitle,
            info: ItemInfo {
                summary: "Application discovered from .desktop entries.".to_string(),
                fields,
            },
            action: Action {
                action_type: "shell_command_exit".to_string(),
                value: detached_launch_command(&entry.exec),
            },
        });
    }

    // Emit one JSON array for the parent process to parse.
    match serde_json::to_string(&items) {
        Ok(output) => println!("{}", output),
        Err(_) => {
            println!("[]");
            std::process::exit(1);
        }
    }
}

// Search the common Linux app directories and collect every .desktop file.
// These files are usually where installed application launchers live.
//
// This is like asking: "Where does Linux keep its application launchers?" and then walking every
// folder that might contain them. We do not just read one directory; we walk nested folders too,
// because some desktop files are stored in subfolders under the main app directories.
fn collect_desktop_files() -> Vec<PathBuf> {
    // Start with system-wide locations and add the user's local application directory.
    let mut roots = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }

    // Use a stack so nested directories can be visited without recursive function calls.
    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = roots.into_iter().filter(|path| path.exists()).collect();

    while let Some(path) = stack.pop() {
        // An unreadable directory is skipped while other directories continue scanning.
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };

        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                // Save directories for later traversal.
                stack.push(child);
                continue;
            }

            // Only files ending in `.desktop` describe launchable desktop entries.
            if child.extension().and_then(|ext| ext.to_str()) == Some("desktop") {
                files.push(child);
            }
        }
    }

    files
}

// Read a single .desktop file and pull out the fields we need.
// Example: name, command to run, description, icon, keywords, etc.
//
// A .desktop file is basically a small config file describing how an application should appear in
// a Linux desktop environment. We are reading only the important parts and ignoring the rest.
fn parse_desktop_entry(content: &str, path: PathBuf) -> Option<DesktopEntry> {
    // These variables collect values while the file is read. They start empty because
    // the parser does not know whether a key will appear until it sees the line.
    let mut in_desktop_entry = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut comment = String::new();
    let mut generic_name = String::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut startup_wm_class = String::new();
    let mut icon = String::new();
    let mut entry_type = String::new();
    let mut no_display = false;
    let mut hidden = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // A section header changes whether following key/value lines belong to
            // `[Desktop Entry]`, the section this provider understands.
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry || trimmed.is_empty() || trimmed.starts_with('#') {
            // Ignore other sections, blank lines, and comments.
            continue;
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            // Lines without `=` are not key/value records, so they cannot fill a field.
            continue;
        };

        // Remove spaces around the key and value before matching known field names.
        let key = raw_key.trim();
        let value = raw_value.trim();

        // Copy recognized values into their matching accumulator; unknown keys are ignored.
        match key {
            "Name" => name = value.to_string(),
            "Exec" => exec = sanitize_exec(value),
            "Comment" => comment = value.to_string(),
            "GenericName" => generic_name = value.to_string(),
            "Keywords" => keywords = parse_keywords(value),
            "StartupWMClass" => startup_wm_class = value.to_string(),
            "Icon" => icon = value.to_string(),
            "Type" => entry_type = value.to_string(),
            "NoDisplay" => no_display = parse_bool(value),
            "Hidden" => hidden = parse_bool(value),
            _ => {}
        }
    }

    // A launcher needs a name and command, and hidden/no-display entries should not appear.
    if name.trim().is_empty() || exec.trim().is_empty() || hidden || no_display {
        return None;
    }

    // If Type is supplied, accept only application entries. An omitted type remains compatible.
    if !entry_type.is_empty() && entry_type != "Application" {
        return None;
    }

    // The filename without its extension is the desktop ID used by desktop environments.
    let desktop_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();

    Some(DesktopEntry {
        name,
        exec,
        comment,
        generic_name,
        keywords,
        startup_wm_class,
        icon,
        desktop_id,
        path,
    })
}

// Keywords are stored in a semicolon-separated list inside the .desktop file.
// We split them into individual words so they become searchable terms.
//
// Example: a file may say Keywords=browser;internet;web;firefox. We split that into
// "browser", "internet", "web", and "firefox" so they all work as search terms.
fn parse_keywords(value: &str) -> Vec<String> {
    // Split at semicolons, trim each piece, discard empty pieces, and own the remaining strings.
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

// Build extra words that can help the search engine find the app even when the user types
// a related term instead of the app's exact title.
// Example: searching for "browser" should match Firefox or Chromium.
//
// This is important because people usually search by what they are thinking, not by the exact
// program name. So the app adds helpful synonyms and metadata words around the title.
fn build_aliases(entry: &DesktopEntry) -> Vec<String> {
    // `aliases` is the output list; `seen` stores lowercase keys so duplicate spellings
    // with different capitalization are still treated as one term.
    let mut aliases = Vec::new();
    let mut seen = HashSet::new();

    // Keep only the executable name from the first Exec token, not its full path or arguments.
    let exec_base = entry
        .exec
        .split_whitespace()
        .next()
        .and_then(|cmd| cmd.rsplit('/').next())
        .unwrap_or("");

    for term in [
        entry.generic_name.as_str(),
        exec_base,
        entry.desktop_id.as_str(),
        entry.startup_wm_class.as_str(),
        entry.icon.as_str(),
    ] {
        // Add each metadata term once when it is not blank.
        let cleaned = term.trim();
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_ascii_lowercase();
        if seen.insert(key) {
            aliases.push(cleaned.to_string());
        }
    }

    for keyword in &entry.keywords {
        // Keywords were already split, but still trim and deduplicate them here.
        let key = keyword.trim().to_ascii_lowercase();
        if !key.is_empty() && seen.insert(key) {
            aliases.push(keyword.trim().to_string());
        }
    }

    // Expand aliases with common app synonyms to improve fuzzy discovery.
    // Clone the current aliases so appending synonyms does not change the collection being iterated.
    let seed_terms = aliases.clone();
    for term in seed_terms {
        append_synonyms(&term, &mut aliases, &mut seen);
    }

    append_synonyms(&entry.name, &mut aliases, &mut seen);
    append_synonyms(&entry.comment, &mut aliases, &mut seen);

    aliases
}

// Add extra common names for a term. This is a small dictionary of useful synonyms.
//
// For example, if the app is a browser, then related terms like "web browser" or "internet"
// can be added as extra aliases. That makes search more natural and forgiving.
fn append_synonyms(term: &str, aliases: &mut Vec<String>, seen: &mut HashSet<String>) {
    // Normalize only for dictionary lookup; preserve the dictionary's display spelling in output.
    let normalized = term.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return;
    }

    for synonym in synonyms_for_term(&normalized) {
        // Add a synonym only the first time its lowercase key appears.
        let key = synonym.to_ascii_lowercase();
        if seen.insert(key) {
            aliases.push((*synonym).to_string());
        }
    }
}

fn synonyms_for_term(term: &str) -> &'static [&'static str] {
    match term {
        "browser" | "web" | "web-browser" | "internet" => {
            &["web browser", "internet browser", "chrome", "chromium", "firefox"]
        }
        "chromium" | "chrome" => &["browser", "web browser", "internet"],
        "firefox" => &["browser", "web browser", "internet"],
        "files" | "file manager" | "file-manager" | "folders" | "folder" => {
            &["nautilus", "nautulus", "explorer", "browse files"]
        }
        "nautilus" | "org.gnome.nautilus" => {
            &["files", "file manager", "folder", "nautulus"]
        }
        "nautulus" => &["nautilus", "files", "file manager"],
        "terminal" | "shell" => &["console", "command line", "cli"],
        "editor" => &["code", "text editor", "ide"],
        _ => &[],
    }
}

// Some .desktop file options are booleans like "True" or "Yes".
// We convert them into a simple true/false value so the code can treat them as flags.
fn parse_bool(value: &str) -> bool {
    // Normalize the value and accept the three common true spellings used in desktop files.
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

// The Exec field can contain special desktop-file codes like %f or %U.
// Those are not literal shell arguments we want to run, so we strip them out.
//
// Example: a desktop entry might say "firefox %u". The %u is a placeholder for a URL, not a real
// literal argument we want to include when we launch the program. We remove those placeholders and
// leave only the real executable and its real arguments.
fn sanitize_exec(raw: &str) -> String {
    // Walk characters manually so `%` placeholders can be removed without accidentally
    // treating their following character as a normal argument character.
    let mut result = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] == '%' {
            // Skip `%f`, `%u`, and similar placeholders. Preserve `%%` as one literal `%`.
            idx += 1;
            if idx < chars.len() && chars[idx] == '%' {
                result.push('%');
                idx += 1;
            } else if idx < chars.len() {
                idx += 1;
            }
            continue;
        }

        // Non-placeholder characters remain part of the command.
        result.push(chars[idx]);
        idx += 1;
    }

    // Collapse repeated whitespace so the generated command has predictable spacing.
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// Turn a title into a safe ID. We keep only letters and numbers and replace spaces
// and punctuation with dashes. This makes IDs predictable and safe to use in JSON.
//
// Example: "Visual Studio Code" becomes "visual-studio-code". That makes the ID easy to read and
// less likely to break JSON or UI code.
fn slugify(value: &str) -> String {
    // Build a lowercase identifier by keeping ASCII letters/digits and replacing runs
    // of other characters with one dash.
    let mut out = String::new();
    let mut previous_dash = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            // Do not emit several dashes for one run of punctuation or whitespace.
            out.push('-');
            previous_dash = true;
        }
    }

    // Remove dashes created at the beginning or end by punctuation.
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        return "app".to_string();
    }

    slug
}

// Launch an app without tying it to the current shell session.
// This uses setsid/nohup so the application keeps running even if the terminal closes.
//
// Why do this? Because when someone launches a GUI app from Tuisual, they do not want the app to be
// killed when the TUI exits or the terminal gets closed. This command makes it continue in the background.
fn detached_launch_command(exec: &str) -> String {
    // Prefix the sanitized Exec command with session detachment and redirect all three
    // standard streams so the GUI app does not remain attached to the TUI terminal.
    // setsid -f fully detaches into a new session so the launched app survives
    // even if the invoking terminal/session is torn down immediately after exit.
    format!("setsid -f nohup {} </dev/null >/dev/null 2>&1", exec)
}

