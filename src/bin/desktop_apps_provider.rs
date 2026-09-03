use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

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

fn main() {
    if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
        eprintln!(
            "This is a Tuisual provider helper. Run the app via 'tuisual -l' or 'cargo run --bin tuisual -- -l'."
        );
        std::process::exit(2);
    }

    let mut items = Vec::new();
    let mut seen_ids = HashSet::new();

    for desktop_file in collect_desktop_files() {
        let Ok(content) = fs::read_to_string(&desktop_file) else {
            continue;
        };

        let Some(entry) = parse_desktop_entry(&content, desktop_file.clone()) else {
            continue;
        };

        let mut item_id = slugify(&entry.name);
        if !seen_ids.insert(item_id.clone()) {
            let base = item_id.clone();
            let mut suffix = 2usize;
            let mut candidate = format!("{}-{}", base, suffix);
            while !seen_ids.insert(candidate.clone()) {
                suffix += 1;
                candidate = format!("{}-{}", base, suffix);
            }
            item_id = candidate;
        }

        let subtitle = if entry.comment.trim().is_empty() {
            "Launch desktop application".to_string()
        } else {
            entry.comment.clone()
        };

        let aliases = build_aliases(&entry);
        let enriched_subtitle = if aliases.is_empty() {
            subtitle
        } else {
            format!("{} | terms: {}", subtitle, aliases.join(", "))
        };

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

    match serde_json::to_string(&items) {
        Ok(output) => println!("{}", output),
        Err(_) => {
            println!("[]");
            std::process::exit(1);
        }
    }
}

fn collect_desktop_files() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }

    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = roots.into_iter().filter(|path| path.exists()).collect();

    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };

        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
                continue;
            }

            if child.extension().and_then(|ext| ext.to_str()) == Some("desktop") {
                files.push(child);
            }
        }
    }

    files
}

fn parse_desktop_entry(content: &str, path: PathBuf) -> Option<DesktopEntry> {
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
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };

        let key = raw_key.trim();
        let value = raw_value.trim();

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

    if name.trim().is_empty() || exec.trim().is_empty() || hidden || no_display {
        return None;
    }

    if !entry_type.is_empty() && entry_type != "Application" {
        return None;
    }

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

fn parse_keywords(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn build_aliases(entry: &DesktopEntry) -> Vec<String> {
    let mut aliases = Vec::new();
    let mut seen = HashSet::new();

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
        let key = keyword.trim().to_ascii_lowercase();
        if !key.is_empty() && seen.insert(key) {
            aliases.push(keyword.trim().to_string());
        }
    }

    // Expand aliases with common app synonyms to improve fuzzy discovery.
    let seed_terms = aliases.clone();
    for term in seed_terms {
        append_synonyms(&term, &mut aliases, &mut seen);
    }

    append_synonyms(&entry.name, &mut aliases, &mut seen);
    append_synonyms(&entry.comment, &mut aliases, &mut seen);

    aliases
}

fn append_synonyms(term: &str, aliases: &mut Vec<String>, seen: &mut HashSet<String>) {
    let normalized = term.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return;
    }

    for synonym in synonyms_for_term(&normalized) {
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

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

fn sanitize_exec(raw: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] == '%' {
            idx += 1;
            if idx < chars.len() && chars[idx] == '%' {
                result.push('%');
                idx += 1;
            } else if idx < chars.len() {
                idx += 1;
            }
            continue;
        }

        result.push(chars[idx]);
        idx += 1;
    }

    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }

    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        return "app".to_string();
    }

    slug
}

fn detached_launch_command(exec: &str) -> String {
    // setsid -f fully detaches into a new session so the launched app survives
    // even if the invoking terminal/session is torn down immediately after exit.
    format!("setsid -f nohup {} </dev/null >/dev/null 2>&1", exec)
}

