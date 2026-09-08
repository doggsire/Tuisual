use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Instant;
use std::time::SystemTime;

// This provider builds a searchable list of installable and installed packages.
// It talks to pacman, AUR tools, and flatpak, then turns them into friendly menu items.
//
// The important idea here is: this file does not launch packages directly.
// It discovers package data, caches it, filters it by the current query, and emits JSON
// that Tuisual can show to the user and use to build install or uninstall commands.
//
// In plain English: we are not just listing packages. We are collecting data from system package
// managers, remembering the results for a little while, and then turning that information into menu
// items the user can click to install or remove software.
const PACMAN_REPO_CACHE_KEY: &str = "pacman_slq";
const FLATPAK_REMOTE_CACHE_KEY: &str = "flatpak_remote_ls";
const PACMAN_INSTALLED_CACHE_KEY: &str = "pacman_qq";
const AUR_INSTALLED_CACHE_KEY: &str = "pacman_qmq";
const FLATPAK_INSTALLED_CACHE_KEY: &str = "flatpak_installed";
const PACMAN_REPO_CACHE_TTL_SECS: u64 = 300;
const FLATPAK_REMOTE_CACHE_TTL_SECS: u64 = 300;
const INSTALLED_CACHE_TTL_SECS: u64 = 300;

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
    // Serde uses this predicate to omit false boolean fields from the JSON output.
    !v
}

fn slugify(text: &str) -> String {
    // Build a lowercase ID by keeping letters/digits and collapsing punctuation into dashes.
    let mut slug = String::with_capacity(text.len());
    let mut last_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            // Avoid a leading dash and avoid repeating dashes for adjacent punctuation.
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn shell_quote(value: &str) -> String {
    // Escape single quotes using the shell pattern `'\''`, then surround the whole
    // value with quotes so package names remain one shell argument.
    format!("'{}'", value.replace('\'', "'\\''"))
}

// The app can pass a search query in the environment.
// If the user is typing a package name, this helps filter the result list.
//
// This query usually comes from the main Tuisual app while the user is typing in the search box.
// The provider listens for it and reuses it to narrow package results.
fn provider_query() -> Option<String> {
    env::var("TUISUAL_PROVIDER_QUERY")
        .ok()
        .map(|query| query.trim().to_ascii_lowercase())
        .filter(|query| !query.is_empty())
}

// A candidate matches if any candidate value contains the current query.
// Example: query = "fire" and candidates = ["firefox", "vlc"] => firefox matches.
//
// This is the filter step. Without this, the provider would dump every package and the search box
// would be useless. With this, the user can type a few letters and instantly narrow the results.
fn matches_query(query: Option<&str>, candidates: &[&str]) -> bool {
    // No query means the caller is building the complete list, so every candidate passes.
    let Some(query) = query else {
        return true;
    };

    // Lowercase each candidate and accept the item when at least one searchable value
    // contains the already-lowercase query.
    candidates
        .iter()
        .map(|candidate| candidate.to_ascii_lowercase())
        .any(|candidate| candidate.contains(query))
}

// Optional debug mode: if enabled, each provider prints timing info to stderr.
fn timing_enabled() -> bool {
    // Accept several familiar true spellings so shell environment configuration is forgiving.
    env::var("TUISUAL_PROVIDER_TIMING")
        .map(|value| {
            let lowered = value.to_ascii_lowercase();
            lowered == "1" || lowered == "true" || lowered == "yes" || lowered == "on"
        })
        .unwrap_or(false)
}

// Turn caching off if the environment says so.
fn cache_disabled() -> bool {
    // Use the same environment parsing convention for the cache switch.
    env::var("TUISUAL_PROVIDER_DISABLE_CACHE")
        .map(|value| {
            let lowered = value.to_ascii_lowercase();
            lowered == "1" || lowered == "true" || lowered == "yes" || lowered == "on"
        })
        .unwrap_or(false)
}

fn read_ttl_env(var: &str, default: u64) -> u64 {
    // Read a positive integer override; malformed or missing values use the supplied default.
    env::var(var)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

// Pick the folder where the provider stores cached data.
// Usually this is ~/.cache/tuisual or $XDG_CACHE_HOME/tuisual.
//
// This is the place where the provider saves recent package lists so it does not need to run the
// slow system commands again and again while the user is typing.
fn cache_root() -> Option<PathBuf> {
    // Returning None is the signal used by cache readers and writers to bypass disk caching.
    if cache_disabled() {
        return None;
    }

    if let Ok(root) = env::var("XDG_CACHE_HOME") {
		// Prefer the standard per-user cache location when it is configured.
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("tuisual"));
        }
    }

    // Fall back to the conventional `.cache` directory below HOME.
    env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".cache").join("tuisual"))
}

// Read cached command output if it is still fresh enough.
// This makes the provider much faster when the user is typing queries repeatedly.
//
// Step by step:
// 1. find the cache file for this command result
// 2. check when it was last modified
// 3. if its age is still below the time limit, reuse it
// 4. otherwise, throw it away and refresh it from the real command
fn read_cached_lines(cache_key: &str, max_age_secs: u64) -> Option<Vec<String>> {
    // Build the cache filename, then return None at the first unavailable or stale step.
    let root = cache_root()?;
    let path = root.join(format!("{}.txt", cache_key));
    let metadata = fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    // Compare the file's modification time with now and convert the age to seconds.
    let age = SystemTime::now().duration_since(modified).ok()?.as_secs();
    if age > max_age_secs {
        return None;
    }

    // Read the cached text and turn each non-empty line into one owned result string.
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

// Save command output to disk so future calls can reuse it without running expensive commands again.
//
// This is the write-side of the cache: we store the package list as a plain text file and come back
// later when the same query or package list is needed again.
fn write_cached_lines(cache_key: &str, lines: &[String]) {
    let Some(root) = cache_root() else {
        return;
    };

    // Create the cache directory on first use. A failed directory creation disables this write.
    if fs::create_dir_all(&root).is_err() {
        return;
    }

    let path = root.join(format!("{}.txt", cache_key));
    // Keep one command result per line and add a final newline for normal text-file formatting.
    let payload = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };

    let _ = fs::write(path, payload);
}

// Reusable wrapper: try cache first, then run the command only if needed.
// This is the main performance trick in the file.
fn run_lines_cached(cmd: &str, args: &[&str], cache_key: &str, ttl_secs: u64) -> Vec<String> {
    // Return fresh cached data immediately; otherwise run the command and save its output.
    if let Some(lines) = read_cached_lines(cache_key, ttl_secs) {
        return lines;
    }

    let lines = run_lines(cmd, args);
    write_cached_lines(cache_key, &lines);
    lines
}

fn run_catalog_lines(cmd: &str, args: &[&str], cache_key: &str, ttl_secs: u64, query: Option<&str>) -> Vec<String> {
    if query.is_some() {
        if let Some(lines) = read_cached_lines(cache_key, u64::MAX) {
            return lines;
        }
    }

    run_lines_cached(cmd, args, cache_key, ttl_secs)
}

// Run a command and capture standard output as lines.
// This is the basic building block for reading package data from pacman, flatpak, etc.
//
// We take the output of a command and turn it into a list of strings, one line per item. That makes
// it easy to treat package names or app IDs as rows of data that can be filtered, sorted, and merged.
fn run_lines(cmd: &str, args: &[&str]) -> Vec<String> {
    // Start the timer only when timing output is enabled so normal provider runs do no extra work.
    let timing = timing_enabled();
    let started = if timing { Some(Instant::now()) } else { None };

    // Capture stdout and the exit status. A spawn failure produces an empty result list.
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

    // Decode stdout lossily, split it into lines, trim each line, and remove blank results.
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

// Read the names of all installed packages from pacman.
// Also include AUR names because AUR packages are often installed through pacman too.
//
// This tells the app which packages are already installed so it can show the correct status and choose
// whether an item should be shown as "install" or "uninstall".
fn installed_pacman_names(aur_names: &[String]) -> HashSet<String> {
    // Start with packages reported by pacman, then add AUR names because pacman also
    // records those installed packages in its local database.
    let mut names = HashSet::new();
    for item in run_lines_cached(
        "pacman",
        &["-Qq"],
        PACMAN_INSTALLED_CACHE_KEY,
        read_ttl_env("TUISUAL_PACMAN_INSTALLED_CACHE_TTL_SECS", INSTALLED_CACHE_TTL_SECS),
    ) {
        names.insert(item);
    }
    for item in aur_names {
        names.insert(item.clone());
    }
    names
}

fn installed_aur_names() -> Vec<String> {
    // Read cached AUR package names, then sort and deduplicate for stable downstream iteration.
    let mut names = run_lines_cached(
        "pacman",
        &["-Qmq"],
        AUR_INSTALLED_CACHE_KEY,
        read_ttl_env("TUISUAL_AUR_INSTALLED_CACHE_TTL_SECS", INSTALLED_CACHE_TTL_SECS),
    );
    names.sort();
    names.dedup();
    names
}

fn installed_flatpak_ids() -> HashSet<String> {
    // Flatpak identifies installed apps by application ID, so store those IDs in a set.
    let mut ids = HashSet::new();
    for item in run_lines_cached(
        "flatpak",
        &["list", "--app", "--columns=application"],
        FLATPAK_INSTALLED_CACHE_KEY,
        read_ttl_env("TUISUAL_FLATPAK_INSTALLED_CACHE_TTL_SECS", INSTALLED_CACHE_TTL_SECS),
    ) {
        ids.insert(item);
    }
    ids
}

// Build the list of package names available in the official Arch repositories.
// We filter by the search query and keep track of whether each package is already installed.
//
// This is one of the main output builders. It reads the list of repo packages, checks if the package
// is installed, and prepares the install/uninstall commands for the menu item.
fn pacman_repo_items(installed: &HashSet<String>, query: Option<&str>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut names = run_catalog_lines(
        "pacman",
        &["-Slq"],
        PACMAN_REPO_CACHE_KEY,
        read_ttl_env("TUISUAL_PACMAN_REPO_CACHE_TTL_SECS", PACMAN_REPO_CACHE_TTL_SECS),
        query,
    );
    // Sort and deduplicate raw command output before building menu records.
    names.sort();
    names.dedup();

    for name in names {
        // Skip packages that do not contain the user's query.
        if !matches_query(query, &[name.as_str()]) {
            continue;
        }

        let key = format!("pacman:{}", name);
        // The same package name gets different actions depending on installed state.
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
        // AUR names come from the installed list, so every accepted item is installed.
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

// Build the list of available flatpak apps from configured remotes.
// Each flatpak app can be searched by its app ID or its display name.
//
// Flatpak works a bit differently from pacman, but the idea is the same: gather app metadata, check
// if it is installed, and create a menu item that knows how to install or uninstall it.
fn flatpak_items(installed: &HashSet<String>, query: Option<&str>) -> Vec<AvailableItem> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let lines = run_catalog_lines(
        "flatpak",
        &["remote-ls", "--app", "--columns=application,name"],
        FLATPAK_REMOTE_CACHE_KEY,
        read_ttl_env(
            "TUISUAL_FLATPAK_REMOTE_CACHE_TTL_SECS",
            FLATPAK_REMOTE_CACHE_TTL_SECS,
        ),
        query,
    );

    for line in lines {
        // Flatpak prints the application ID and display name separated by a tab.
        let mut parts = line.splitn(2, '\t');
        let app_id = match parts.next() {
            Some(v) if !v.trim().is_empty() => v.trim().to_string(),
            _ => continue,
        };
        // If no display name was printed, use the application ID as the fallback title.
        let display_name = parts.next().map(str::trim).unwrap_or(&app_id).to_string();
        // Ignore malformed rows and duplicate IDs before checking the query.
        if display_name.trim().is_empty() || !seen.insert(app_id.clone()) {
            continue;
        }

        if !matches_query(query, &[app_id.as_str(), display_name.as_str()]) {
            continue;
        }

        // Choose install/uninstall state from the installed-ID set.
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

// This is the main assembly step.
// It gathers package information from pacman, AUR, and flatpak, merges them together,
// and then converts them into ProviderItem values that Tuisual can display.
//
// This is the big combine step where all the package data finally becomes menu items.
// We gather all the source lists, map them by key, and then produce one list of user-visible results.
fn build_available_items() -> Vec<ProviderItem> {
    // Read the query once so every source applies the same filter.
    let timing = timing_enabled();
    let query = provider_query();

    // Load the installed sets before starting source-specific enumeration.
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

    // Pacman and Flatpak are independent external commands, so run them concurrently.
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

    // AUR items use the already-loaded local AUR names and can be built on this thread.
    let aur_items_started = Instant::now();
    let aur_items = aur_installed_items(&aur_names, query.as_deref());
    let aur_items_elapsed_ms = aur_items_started.elapsed().as_millis();

    // Join both workers before merging. A worker panic becomes an empty result via the default tuple.
    let (pacman_items, pacman_elapsed_ms) = pacman_repo_handle.join().unwrap_or_default();

    let (flatpak_items_result, flatpak_elapsed_ms) =
        flatpak_items_handle.join().unwrap_or_default();

    // Key by source-qualified ID so similarly named packages from different ecosystems coexist.
    let mut map: HashMap<String, AvailableItem> = HashMap::new();

    for item in pacman_items {
        // Insert each source result; the qualified key prevents accidental collisions.
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

    // Convert internal AvailableItems into the public provider schema.
    let mut items: Vec<ProviderItem> = map
        .into_values()
        .map(|item| {
			// Select display text, status, and command from the installed flag.
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

    // Sort final titles case-insensitively for predictable search results.
    items.sort_by_cached_key(|a| a.title.to_ascii_lowercase());

    items
}

fn main() {
    // Require provider mode so direct execution does not produce JSON unexpectedly.
    if env::var_os("TUISUAL_PROVIDER_MODE").is_none() {
        eprintln!(
            "This is a Tuisual provider helper. Run the app via 'tuisual --installer' or 'cargo run --bin tuisual -- --installer'."
        );
        std::process::exit(2);
    }

    let timing = timing_enabled();
    let total_started = Instant::now();

    // Build all package actions, optionally report timing, and serialize the result for Tuisual.
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
