use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Instant;
use std::time::SystemTime;

const PACMAN_REPO_CACHE_KEY: &str = "pacman_slq";
const FLATPAK_REMOTE_CACHE_KEY: &str = "flatpak_remote_ls";
const PACMAN_REPO_CACHE_TTL_SECS: u64 = 300;
const FLATPAK_REMOTE_CACHE_TTL_SECS: u64 = 300;

#[derive(Debug, Serialize, Clone)]
struct InfoField {
    label: String,
    value: String,
}

#[derive(Debug, Serialize, Clone)]
struct ItemInfo {
    summary: String,
    fields: Vec<InfoField>,
}

#[derive(Debug, Serialize, Clone)]
struct Action {
    #[serde(rename = "type")]
    action_type: String,
    value: String,
}

#[derive(Debug, Serialize, Clone)]
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

#[derive(Debug, Serialize, Clone)]
struct ActionSubItem {
    id: String,
    title: String,
    subtitle: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_after: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<SubItemInput>,
}

#[derive(Debug, Serialize, Clone)]
struct SubItemInput {
    flag_prefix: String,
    prompt: String,
}

#[derive(Debug, Clone)]
struct AvailableItem {
    key: String,
    title: String,
    source: String,
    installed: bool,
    install_command: String,
    uninstall_command: String,
    detail: String,
}

fn is_false(v: &bool) -> bool {
    !v
}

fn slugify(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut last_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn provider_query() -> Option<String> {
    env::var("TUISUAL_PROVIDER_QUERY")
        .ok()
        .map(|query| query.trim().to_ascii_lowercase())
        .filter(|query| !query.is_empty())
}

fn matches_query(query: Option<&str>, candidates: &[&str]) -> bool {
    let Some(query) = query else {
        return true;
    };

    candidates
        .iter()
        .map(|candidate| candidate.to_ascii_lowercase())
        .any(|candidate| candidate.contains(query))
}

fn timing_enabled() -> bool {
    env::var("TUISUAL_PROVIDER_TIMING")
        .map(|value| {
            let lowered = value.to_ascii_lowercase();
            lowered == "1" || lowered == "true" || lowered == "yes" || lowered == "on"
        })
        .unwrap_or(false)
}

fn cache_disabled() -> bool {
    env::var("TUISUAL_PROVIDER_DISABLE_CACHE")
        .map(|value| {
            let lowered = value.to_ascii_lowercase();
            lowered == "1" || lowered == "true" || lowered == "yes" || lowered == "on"
        })
        .unwrap_or(false)
}

fn read_ttl_env(var: &str, default: u64) -> u64 {
    env::var(var)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn cache_root() -> Option<PathBuf> {
    if cache_disabled() {
        return None;
    }

    if let Ok(root) = env::var("XDG_CACHE_HOME") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("tuisual"));
        }
    }

    env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".cache").join("tuisual"))
}

fn read_cached_lines(cache_key: &str, max_age_secs: u64) -> Option<Vec<String>> {
    let root = cache_root()?;
    let path = root.join(format!("{}.txt", cache_key));
    let metadata = fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?.as_secs();
    if age > max_age_secs {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if timing_enabled() {
        eprintln!(
            "[provider-timing] cache_hit key={} lines={} age_secs={}",
            cache_key,
            lines.len(),
            age
        );
    }

    Some(lines)
}

fn write_cached_lines(cache_key: &str, lines: &[String]) {
    let Some(root) = cache_root() else {
        return;
    };

    if fs::create_dir_all(&root).is_err() {
        return;
    }

    let path = root.join(format!("{}.txt", cache_key));
    let payload = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };

    let _ = fs::write(path, payload);
}

fn run_lines_cached(cmd: &str, args: &[&str], cache_key: &str, ttl_secs: u64) -> Vec<String> {
    if let Some(lines) = read_cached_lines(cache_key, ttl_secs) {
        return lines;
    }

    let lines = run_lines(cmd, args);
    write_cached_lines(cache_key, &lines);
    lines
}

fn run_lines(cmd: &str, args: &[&str]) -> Vec<String> {
    let timing = timing_enabled();
    let started = if timing { Some(Instant::now()) } else { None };

    let Ok(output) = Command::new(cmd).args(args).output() else {
        if let Some(started) = started {
            eprintln!(
                "[provider-timing] command={} args={} failed_to_spawn elapsed_ms={}",
                cmd,
                args.join(" "),
                started.elapsed().as_millis()
            );
        }
        return Vec::new();
    };

    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if let Some(started) = started {
        eprintln!(
            "[provider-timing] command={} args={} exit={:?} lines={} elapsed_ms={}",
            cmd,
            args.join(" "),
            output.status.code(),
            lines.len(),
            started.elapsed().as_millis()
        );
    }

    lines
}

fn installed_pacman_names(aur_names: &[String]) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in run_lines("pacman", &["-Qq"]) {
        names.insert(item);
    }
    for item in aur_names {
        names.insert(item.clone());
    }
    names
}

fn installed_aur_names() -> Vec<String> {
    let mut names = run_lines("pacman", &["-Qmq"]);
    names.sort();
    names.dedup();
    names
}

fn installed_flatpak_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    for item in run_lines("flatpak", &["list", "--app", "--columns=application"]) {
        ids.insert(item);
    }
    ids
}

fn pacman_repo_items(installed: &HashSet<String>, query: Option<&str>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut names = run_lines_cached(
        "pacman",
        &["-Slq"],
        PACMAN_REPO_CACHE_KEY,
        read_ttl_env("TUISUAL_PACMAN_REPO_CACHE_TTL_SECS", PACMAN_REPO_CACHE_TTL_SECS),
    );
    names.sort();
    names.dedup();

    for name in names {
        if !matches_query(query, &[name.as_str()]) {
            continue;
        }

        let key = format!("pacman:{}", name);
        let installed_here = installed.contains(&name);
        let install_command = format!("sudo pacman -S --needed {}", shell_quote(&name));
        let uninstall_command = format!("sudo pacman -R --noconfirm {}", shell_quote(&name));
        result.push(AvailableItem {
            key,
            title: name.clone(),
            source: "pacman".to_string(),
            installed: installed_here,
            install_command,
            uninstall_command,
            detail: "Official Arch repository package".to_string(),
        });
    }
    result
}

fn aur_installed_items(aur_names: &[String], query: Option<&str>) -> Vec<AvailableItem> {
    let mut result = Vec::new();

    for name in aur_names {
        if !matches_query(query, &[name.as_str()]) {
            continue;
        }

        let key = format!("aur:{}", name);
        let install_command = format!("paru -S --needed {}", shell_quote(name));
        let uninstall_command = format!("paru -R --noconfirm {}", shell_quote(name));
        result.push(AvailableItem {
            key,
            title: name.clone(),
            source: "aur".to_string(),
            installed: true,
            install_command,
            uninstall_command,
            detail: "Installed from the AUR via paru".to_string(),
        });
    }
    result
}

fn flatpak_items(installed: &HashSet<String>, query: Option<&str>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let lines = run_lines_cached(
        "flatpak",
        &["remote-ls", "--app", "--columns=application,name"],
        FLATPAK_REMOTE_CACHE_KEY,
        read_ttl_env(
            "TUISUAL_FLATPAK_REMOTE_CACHE_TTL_SECS",
            FLATPAK_REMOTE_CACHE_TTL_SECS,
        ),
    );

    for line in lines {
        let mut parts = line.splitn(2, '\t');
        let app_id = match parts.next() {
            Some(v) if !v.trim().is_empty() => v.trim().to_string(),
            _ => continue,
        };
        let display_name = parts.next().map(str::trim).unwrap_or(&app_id).to_string();
        if display_name.trim().is_empty() || !seen.insert(app_id.clone()) {
            continue;
        }

        if !matches_query(query, &[app_id.as_str(), display_name.as_str()]) {
            continue;
        }

        let installed_here = installed.contains(&app_id);
        let key = format!("flatpak:{}", app_id);
        let title = if display_name.is_empty() { app_id.clone() } else { display_name };
        let install_command = format!("flatpak install --noninteractive flathub {}", shell_quote(&app_id));
        let uninstall_command = format!("flatpak uninstall --noninteractive {}", shell_quote(&app_id));
        result.push(AvailableItem {
            key,
            title,
            source: "flatpak".to_string(),
            installed: installed_here,
            install_command,
            uninstall_command,
            detail: "Flatpak app from a configured remote".to_string(),
        });
    }
    result
}

fn build_available_items() -> Vec<ProviderItem> {
    let timing = timing_enabled();
    let query = provider_query();

    let aur_started = Instant::now();
    let aur_names = installed_aur_names();
    if timing {
        eprintln!(
            "[provider-timing] step=installed_aur_names count={} elapsed_ms={}",
            aur_names.len(),
            aur_started.elapsed().as_millis()
        );
    }

    let pacman_installed_started = Instant::now();
    let installed_pacman = installed_pacman_names(&aur_names);
    if timing {
        eprintln!(
            "[provider-timing] step=installed_pacman_names count={} elapsed_ms={}",
            installed_pacman.len(),
            pacman_installed_started.elapsed().as_millis()
        );
    }

    let flatpak_installed_started = Instant::now();
    let installed_flatpak = installed_flatpak_ids();
    if timing {
        eprintln!(
            "[provider-timing] step=installed_flatpak_ids count={} elapsed_ms={}",
            installed_flatpak.len(),
            flatpak_installed_started.elapsed().as_millis()
        );
    }

    let pacman_installed_for_thread = installed_pacman.clone();
    let flatpak_installed_for_thread = installed_flatpak.clone();
    let pacman_query = query.clone();
    let flatpak_query = query.clone();

    let pacman_repo_handle = thread::spawn(move || {
        let started = Instant::now();
        let items = pacman_repo_items(&pacman_installed_for_thread, pacman_query.as_deref());
        (items, started.elapsed().as_millis())
    });

    let flatpak_items_handle = thread::spawn(move || {
        let started = Instant::now();
        let items = flatpak_items(&flatpak_installed_for_thread, flatpak_query.as_deref());
        (items, started.elapsed().as_millis())
    });

    let aur_items_started = Instant::now();
    let aur_items = aur_installed_items(&aur_names, query.as_deref());
    let aur_items_elapsed_ms = aur_items_started.elapsed().as_millis();

    let (pacman_items, pacman_elapsed_ms) = pacman_repo_handle.join().unwrap_or_default();

    let (flatpak_items_result, flatpak_elapsed_ms) =
        flatpak_items_handle.join().unwrap_or_default();

    let mut map: HashMap<String, AvailableItem> = HashMap::new();

    for item in pacman_items {
        map.insert(item.key.clone(), item);
    }
    if timing {
        eprintln!(
            "[provider-timing] step=pacman_repo_items map_size={} elapsed_ms={}",
            map.len(),
            pacman_elapsed_ms
        );
    }

    for item in aur_items {
        map.insert(item.key.clone(), item);
    }
    if timing {
        eprintln!(
            "[provider-timing] step=aur_installed_items map_size={} elapsed_ms={}",
            map.len(),
            aur_items_elapsed_ms
        );
    }

    for item in flatpak_items_result {
        map.insert(item.key.clone(), item);
    }
    if timing {
        eprintln!(
            "[provider-timing] step=flatpak_items map_size={} elapsed_ms={}",
            map.len(),
            flatpak_elapsed_ms
        );
    }

    let mut items: Vec<ProviderItem> = map
        .into_values()
        .map(|item| {
            let title = if item.installed {
                format!("{} [installed]", item.title)
            } else {
                item.title.clone()
            };

            let status = if item.installed {
                "Installed".to_string()
            } else {
                "Available".to_string()
            };

            let action_value = if item.installed {
                item.uninstall_command.clone()
            } else {
                item.install_command.clone()
            };

            let subtitle = format!("{} | {} | {}", item.detail, item.source, status);
            let info = ItemInfo {
                summary: if item.installed {
                    format!("'{}' is already installed and can be removed.", item.title)
                } else {
                    format!("'{}' is available to install from {}.", item.title, item.source)
                },
                fields: vec![
                    InfoField {
                        label: "Source".to_string(),
                        value: item.source.clone(),
                    },
                    InfoField {
                        label: "Status".to_string(),
                        value: status,
                    },
                    InfoField {
                        label: "Command".to_string(),
                        value: action_value.clone(),
                    },
                ],
            };

            ProviderItem {
                id: format!("pkg-{}-{}", item.source, slugify(&item.key)),
                title,
                subtitle,
                info,
                action: Action {
                    action_type: "shell_command_exit".to_string(),
                    value: action_value,
                },
                require_sub_item: false,
                sub_items: Vec::new(),
            }
        })
        .collect();

    items.sort_by_key(|a| a.title.to_ascii_lowercase());

    let install_aur = ProviderItem {
        id: "pkg-aur-install".to_string(),
        title: "Install package (AUR via paru)".to_string(),
        subtitle: "Search or install a package from the AUR".to_string(),
        info: ItemInfo {
            summary: "Install a package from the AUR by typing a package name. This is a manual fallback when an AUR package cannot be enumerated from the local metadata.".to_string(),
            fields: vec![
                InfoField { label: "Source".to_string(), value: "AUR / paru".to_string() },
                InfoField { label: "Command".to_string(), value: "paru -S --needed <package>".to_string() },
            ],
        },
        action: Action {
            action_type: "shell_command_exit".to_string(),
            value: "paru -S --needed".to_string(),
        },
        require_sub_item: true,
        sub_items: vec![ActionSubItem {
            id: "enter-package".to_string(),
            title: "Type package name".to_string(),
            subtitle: "Install a package from the AUR using paru".to_string(),
            flags: vec![],
            exit_after: Some(true),
            input: Some(SubItemInput {
                flag_prefix: " ".to_string(),
                prompt: "AUR package name".to_string(),
            }),
        }],
    };

    items.insert(0, install_aur);
    items
}

fn main() {
    if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
        eprintln!(
            "This is a Tuisual provider helper. Run the app via 'tuisual --pkg-manager' or 'cargo run --bin tuisual -- --pkg-manager'."
        );
        std::process::exit(2);
    }

    let timing = timing_enabled();
    let total_started = Instant::now();

    let items = build_available_items();
    if timing {
        eprintln!(
            "[provider-timing] step=build_available_items items={} elapsed_ms={}",
            items.len(),
            total_started.elapsed().as_millis()
        );
    }
    match serde_json::to_string(&items) {
        Ok(output) => println!("{}", output),
        Err(_) => {
            println!("[]");
            std::process::exit(1);
        }
    }
}
