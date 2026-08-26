use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::process::Command;

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
    version: String,
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

fn run_lines(cmd: &str, args: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new(cmd).args(args).output() else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn installed_pacman_names() -> HashSet<String> {
    let mut names = HashSet::new();
    for item in run_lines("pacman", &["-Qq"]) {
        names.insert(item);
    }
    for item in run_lines("pacman", &["-Qmq"]) {
        names.insert(item);
    }
    names
}

fn installed_flatpak_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    for item in run_lines("flatpak", &["list", "--app", "--columns=application"]) {
        ids.insert(item);
    }
    ids
}

fn pacman_repo_items(installed: &HashSet<String>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut names = run_lines("pacman", &["-Slq"]);
    names.sort();
    names.dedup();

    for name in names {
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
            version: String::new(),
        });
    }
    result
}

fn aur_installed_items() -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut names = run_lines("pacman", &["-Qmq"]);
    names.sort();
    names.dedup();

    for name in names {
        let key = format!("aur:{}", name);
        let install_command = format!("paru -S --needed {}", shell_quote(&name));
        let uninstall_command = format!("paru -R --noconfirm {}", shell_quote(&name));
        result.push(AvailableItem {
            key,
            title: name.clone(),
            source: "aur".to_string(),
            installed: true,
            install_command,
            uninstall_command,
            detail: "Installed from the AUR via paru".to_string(),
            version: String::new(),
        });
    }
    result
}

fn flatpak_items(installed: &HashSet<String>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let lines = run_lines("flatpak", &["remote-ls", "--app", "--columns=application,name"]);

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
            version: String::new(),
        });
    }
    result
}

fn build_available_items() -> Vec<ProviderItem> {
    let installed_pacman = installed_pacman_names();
    let installed_flatpak = installed_flatpak_ids();

    let mut map: HashMap<String, AvailableItem> = HashMap::new();

    for item in pacman_repo_items(&installed_pacman) {
        map.insert(item.key.clone(), item);
    }
    for item in aur_installed_items() {
        map.insert(item.key.clone(), item);
    }
    for item in flatpak_items(&installed_flatpak) {
        map.insert(item.key.clone(), item);
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

    items.sort_by(|a, b| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()));

    let mut install_aur = ProviderItem {
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

    let items = build_available_items();
    match serde_json::to_string(&items) {
        Ok(output) => println!("{}", output),
        Err(_) => {
            println!("[]");
            std::process::exit(1);
        }
    }
}
