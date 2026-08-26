use crate::models::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};
use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub trait Provider {
    fn name(&self) -> &'static str;
    fn short_flag(&self) -> Option<char> {
        None
    }
    fn list_items(&self) -> Vec<ProviderItem>;
}

#[derive(Debug, Default)]
pub struct ProviderLoadReport {
    pub items: Vec<AppItem>,
    pub rejected: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalProviderDoc {
    name: String,
    short_flag: Option<String>,
    #[serde(default)]
    items: Vec<ProviderItem>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    shell_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderDescriptor {
    name: String,
    short_flag: Option<char>,
    source: String,
}

#[derive(Debug, Default)]
struct ProviderFilter {
    long_flags: HashSet<String>,
    short_flags: HashSet<char>,
}

impl ProviderFilter {
    fn from_args(args: &[String]) -> Self {
        let mut filter = Self::default();

        for arg in args {
            if let Some(name) = arg.strip_prefix("--") {
                if !name.is_empty() {
                    filter.long_flags.insert(name.to_ascii_lowercase());
                }
                continue;
            }

            if let Some(shorts) = arg.strip_prefix('-')
                && !shorts.is_empty()
            {
                for ch in shorts.chars() {
                    filter.short_flags.insert(ch.to_ascii_lowercase());
                }
            }
        }

        filter
    }

    fn is_active(&self) -> bool {
        !self.long_flags.is_empty() || !self.short_flags.is_empty()
    }

    fn matches(&self, name: &str, short_flag: Option<char>) -> bool {
        if !self.is_active() {
            return true;
        }

        if self.long_flags.contains(&name.to_ascii_lowercase()) {
            return true;
        }

        if let Some(short) = short_flag {
            return self.short_flags.contains(&short.to_ascii_lowercase());
        }

        false
    }
}

pub fn load_provider_items(providers: &[Box<dyn Provider>]) -> ProviderLoadReport {
    load_provider_items_filtered(providers, &ProviderFilter::default())
}

fn load_provider_items_filtered(
    providers: &[Box<dyn Provider>],
    filter: &ProviderFilter,
) -> ProviderLoadReport {
    let mut report = ProviderLoadReport::default();

    for provider in providers {
        if !filter.matches(provider.name(), provider.short_flag()) {
            continue;
        }

        for raw in provider.list_items() {
            let item_id = raw.id.clone();
            match AppItem::from_provider_item(provider.name(), raw) {
                Ok(item) => report.items.push(item),
                Err(err) => report
                    .rejected
                    .push(format!("provider={} item={} error={}", provider.name(), item_id, err)),
            }
        }
    }

    report
}

fn parse_external_provider_doc(content: &str) -> Result<ExternalProviderDoc, String> {
    serde_json::from_str(content).map_err(|err| format!("json parse error: {}", err))
}

fn parse_short_flag(raw: Option<&str>) -> Result<Option<char>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("short_flag must be one character".to_string());
    };

    if chars.next().is_some() {
        return Err("short_flag must be one character".to_string());
    }

    Ok(Some(first.to_ascii_lowercase()))
}

fn append_external_doc(
    report: &mut ProviderLoadReport,
    source: &str,
    filter: &ProviderFilter,
    doc: ExternalProviderDoc,
) {
    let short_flag = match parse_short_flag(doc.short_flag.as_deref()) {
        Ok(value) => value,
        Err(err) => {
            report.rejected.push(format!(
                "source={} provider={} error={}",
                source, doc.name, err
            ));
            return;
        }
    };

    if !filter.matches(&doc.name, short_flag) {
        return;
    }

    let provider_name = doc.name;
    let mut collected_items = doc.items;

    match (doc.command, doc.shell_command) {
        (Some(_), Some(_)) => {
            report.rejected.push(format!(
                "source={} provider={} error=dynamic provider must define only one of 'command' or 'shell_command'",
                source, provider_name
            ));
            return;
        }
        (Some(command), None) => match run_provider_binary_command(&command) {
            Ok(mut generated) => collected_items.append(&mut generated),
            Err(err) => {
                report.rejected.push(format!(
                    "source={} provider={} error=dynamic command failed: {}",
                    source, provider_name, err
                ));
            }
        },
        (None, Some(shell_command)) => match run_provider_shell_command(&shell_command) {
            Ok(mut generated) => collected_items.append(&mut generated),
            Err(err) => {
                report.rejected.push(format!(
                    "source={} provider={} error=dynamic shell command failed: {}",
                    source, provider_name, err
                ));
            }
        },
        (None, None) => {}
    }

    if collected_items.is_empty() {
        report.rejected.push(format!(
            "source={} provider={} error=no items produced",
            source, provider_name
        ));
        return;
    }

    for raw in collected_items {
        let item_id = raw.id.clone();
        match AppItem::from_provider_item(&provider_name, raw) {
            Ok(item) => report.items.push(item),
            Err(err) => report.rejected.push(format!(
                "source={} provider={} item={} error={}",
                source, provider_name, item_id, err
            )),
        }
    }
}

fn resolve_provider_command(command: &str) -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let trimmed = command.trim();

    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || trimmed.starts_with("~") || trimmed.starts_with("$") {
        return trimmed.to_string();
    }

    let project_relative = manifest_dir.join(trimmed);
    if project_relative.exists() {
        return project_relative.display().to_string();
    }

    trimmed.to_string()
}

fn ensure_provider_binary_exists(command: &str) -> Result<String, String> {
    let resolved = resolve_provider_command(command);
    if Path::new(&resolved).exists() {
        return Ok(resolved);
    }

    let binary_name = Path::new(&resolved)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("provider command has no executable target: {}", command))?;

    let status = Command::new("cargo")
        .arg("build")
        .arg("--bin")
        .arg(binary_name)
        .arg("--quiet")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .map_err(|err| format!("failed to build provider binary {}: {}", binary_name, err))?;

    if !status.success() {
        return Err(format!(
            "provider binary '{}' could not be built for command '{}', exit status {:?}",
            binary_name,
            command,
            status.code()
        ));
    }

    if Path::new(&resolved).exists() {
        Ok(resolved)
    } else {
        Err(format!(
            "provider binary '{}' was not created for command '{}'",
            binary_name, command
        ))
    }
}

fn run_provider_binary_command(command: &str) -> Result<Vec<ProviderItem>, String> {
    let resolved = ensure_provider_binary_exists(command)?;
    run_provider_shell_and_parse(&resolved, "command", "provider command")
}

fn run_provider_shell_command(command: &str) -> Result<Vec<ProviderItem>, String> {
    run_provider_shell_and_parse(command, "shell command", "provider shell command")
}

fn run_provider_shell_and_parse(
    command: &str,
    error_context: &str,
    output_context: &str,
) -> Result<Vec<ProviderItem>, String> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let output = Command::new(&shell)
        .arg("-lc")
        .arg(format!("cd {} && {}", env!("CARGO_MANIFEST_DIR"), command))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TUISUAL_PROVIDER_MODE", "1")
        .output()
        .map_err(|err| format!("failed to run {}: {}", error_context, err))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "exit status {:?}: {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("{} output was not utf-8: {}", output_context, err))?;

    let items: Vec<ProviderItem> = serde_json::from_str(&stdout)
        .map_err(|err| format!("{} output json parse error: {}", output_context, err))?;

    Ok(items)
}

fn load_external_provider_items(dir: &Path, filter: &ProviderFilter) -> ProviderLoadReport {
    let mut report = ProviderLoadReport::default();

    if !dir.exists() {
        return report;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            report.rejected.push(format!(
                "source={} error=failed to read providers directory: {}",
                dir.display(),
                err
            ));
            return report;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                report
                    .rejected
                    .push(format!("source={} error=directory entry error: {}", dir.display(), err));
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let source = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(err) => {
                report.rejected.push(format!(
                    "source={} error=failed to read provider file: {}",
                    source, err
                ));
                continue;
            }
        };

        match parse_external_provider_doc(&content) {
            Ok(doc) => append_external_doc(&mut report, &source, filter, doc),
            Err(err) => report
                .rejected
                .push(format!("source={} error={}", source, err)),
        }
    }

    report
}

fn merge_reports(mut base: ProviderLoadReport, next: ProviderLoadReport) -> ProviderLoadReport {
    base.items.extend(next.items);
    base.rejected.extend(next.rejected);
    base
}

pub fn load_all_items() -> ProviderLoadReport {
    let providers_dir = std::env::var("TUISUAL_PROVIDERS_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "providers".to_string());
    let mut report = ProviderLoadReport::default();
    let mut seen = HashSet::new();

    for descriptor in built_in_provider_descriptors() {
        if seen.insert(descriptor.name.to_ascii_lowercase()) {
            report.items.push(provider_catalog_item(&descriptor));
        }
    }

    let (external_descriptors, external_rejected) =
        load_external_provider_descriptors(Path::new(&providers_dir));
    report.rejected.extend(external_rejected);

    for descriptor in external_descriptors {
        if seen.insert(descriptor.name.to_ascii_lowercase()) {
            report.items.push(provider_catalog_item(&descriptor));
        }
    }

    if report.items.is_empty() {
        report.rejected.push("no providers discovered".to_string());
    }

    report
}

pub fn load_all_items_from_args(args: &[String]) -> ProviderLoadReport {
    let filter = ProviderFilter::from_args(args);
    let built_in = if filter.is_active() {
        load_provider_items_filtered(&default_providers(), &filter)
    } else {
        load_provider_items(&default_providers())
    };
    let providers_dir = std::env::var("TUISUAL_PROVIDERS_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "providers".to_string());
    let external = load_external_provider_items(Path::new(&providers_dir), &filter);
    merge_reports(built_in, external)
}

fn built_in_provider_descriptors() -> Vec<ProviderDescriptor> {
    default_providers()
        .iter()
        .map(|provider| ProviderDescriptor {
            name: provider.name().to_string(),
            short_flag: provider.short_flag(),
            source: "built-in".to_string(),
        })
        .collect()
}

fn load_external_provider_descriptors(dir: &Path) -> (Vec<ProviderDescriptor>, Vec<String>) {
    let mut descriptors = Vec::new();
    let mut rejected = Vec::new();

    if !dir.exists() {
        return (descriptors, rejected);
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            rejected.push(format!(
                "source={} error=failed to read providers directory: {}",
                dir.display(),
                err
            ));
            return (descriptors, rejected);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                rejected.push(format!(
                    "source={} error=directory entry error: {}",
                    dir.display(),
                    err
                ));
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let source = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(err) => {
                rejected.push(format!(
                    "source={} error=failed to read provider file: {}",
                    source, err
                ));
                continue;
            }
        };

        match parse_external_provider_doc(&content) {
            Ok(doc) => match parse_short_flag(doc.short_flag.as_deref()) {
                Ok(short_flag) => descriptors.push(ProviderDescriptor {
                    name: doc.name,
                    short_flag,
                    source,
                }),
                Err(err) => rejected.push(format!(
                    "source={} provider={} error={}",
                    source, doc.name, err
                )),
            },
            Err(err) => rejected.push(format!("source={} error={}", source, err)),
        }
    }

    (descriptors, rejected)
}

fn provider_catalog_item(descriptor: &ProviderDescriptor) -> AppItem {
    AppItem {
        provider: "catalog".to_string(),
        id: format!("provider:{}", descriptor.name),
        title: descriptor.name.clone(),
        subtitle: format!("{} provider", descriptor.source),
        info: ItemInfo {
            summary: "Discovered provider. Launch with its flag to load provider items.".to_string(),
            fields: vec![
                InfoField {
                    label: "Provider".to_string(),
                    value: descriptor.name.clone(),
                },
                InfoField {
                    label: "Short Flag".to_string(),
                    value: descriptor
                        .short_flag
                        .map(|value| format!("-{}", value))
                        .unwrap_or_else(|| "(none)".to_string()),
                },
                InfoField {
                    label: "Source".to_string(),
                    value: descriptor.source.clone(),
                },
            ],
        },
        action: ItemAction::ProviderHint(descriptor.name.clone()),
        require_sub_item: false,
        sub_items: vec![],
    }
}

pub fn default_providers() -> Vec<Box<dyn Provider>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalProviderDoc, Provider, ProviderFilter, ProviderLoadReport, append_external_doc,
        load_all_items, load_all_items_from_args, load_provider_items, merge_reports,
        parse_external_provider_doc,
    };
    use crate::models::{InfoField, ItemAction, ItemInfo, ProviderItem};

    struct BadProvider;

    impl Provider for BadProvider {
        fn name(&self) -> &'static str {
            "bad"
        }

        fn list_items(&self) -> Vec<ProviderItem> {
            vec![ProviderItem {
                id: "broken".to_string(),
                title: " ".to_string(),
                subtitle: "Missing title".to_string(),
                info: ItemInfo {
                    summary: "Has summary".to_string(),
                    fields: vec![InfoField {
                        label: "L".to_string(),
                        value: "V".to_string(),
                    }],
                },
                action: ItemAction::ShellCommand("echo broken".to_string()),
                require_sub_item: false,
                sub_items: vec![],
            }]
        }
    }

    #[test]
    fn invalid_provider_items_are_rejected() {
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(BadProvider)];
        let report = load_provider_items(&providers);

        assert!(report.items.is_empty());
        assert_eq!(report.rejected.len(), 1);
    }

    #[test]
    fn parses_valid_external_doc() {
        let json = r#"{
            "name": "external",
            "items": [
                {
                    "id": "one",
                    "title": "One",
                    "subtitle": "First",
                    "info": {
                        "summary": "Sample",
                        "fields": [{"label": "A", "value": "B"}]
                    },
                    "action": {"type": "shell_command", "value": "echo one"}
                }
            ]
        }"#;

        let doc = parse_external_provider_doc(json).expect("doc should parse");
        assert_eq!(doc.name, "external");
        assert_eq!(doc.items.len(), 1);
    }

    #[test]
    fn rejects_bad_external_doc_json() {
        let json = "{ this is not json }";
        let result = parse_external_provider_doc(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_items_inside_external_doc() {
        let mut report = ProviderLoadReport::default();
        let doc = ExternalProviderDoc {
            name: "external".to_string(),
            short_flag: None,
            items: vec![ProviderItem {
                id: "broken".to_string(),
                title: " ".to_string(),
                subtitle: "bad".to_string(),
                info: ItemInfo {
                    summary: "ok".to_string(),
                    fields: vec![InfoField {
                        label: "a".to_string(),
                        value: "b".to_string(),
                    }],
                },
                action: ItemAction::ShellCommand("echo bad".to_string()),
                require_sub_item: false,
                sub_items: vec![],
            }],
            command: None,
            shell_command: None,
        };

        append_external_doc(&mut report, "external.json", &ProviderFilter::default(), doc);
        assert!(report.items.is_empty());
        assert_eq!(report.rejected.len(), 1);
    }

    #[test]
    fn merges_reports() {
        let base = ProviderLoadReport {
            items: Vec::new(),
            rejected: vec!["r1".to_string()],
        };
        let next = ProviderLoadReport {
            items: Vec::new(),
            rejected: vec!["r2".to_string()],
        };

        let merged = merge_reports(base, next);
        assert_eq!(merged.rejected.len(), 2);
    }

    #[test]
    fn long_flag_selects_matching_provider() {
        let filter = ProviderFilter::from_args(&["--example".to_string()]);
        assert!(filter.matches("example", Some('x')));
        assert!(!filter.matches("mock", Some('m')));
    }

    #[test]
    fn short_flag_selects_matching_provider() {
        let filter = ProviderFilter::from_args(&["-x".to_string()]);
        assert!(filter.matches("example", Some('x')));
        assert!(!filter.matches("mock", Some('m')));
    }

    #[test]
    fn short_flag_ignored_when_invalid_length() {
        let mut report = ProviderLoadReport::default();
        let doc = ExternalProviderDoc {
            name: "example".to_string(),
            short_flag: Some("xy".to_string()),
            items: vec![],
            command: None,
            shell_command: None,
        };

        append_external_doc(&mut report, "example.json", &ProviderFilter::default(), doc);
        assert_eq!(report.rejected.len(), 1);
    }

    #[test]
    fn no_flag_loads_provider_catalog() {
        let report = load_all_items();
        assert!(!report.items.is_empty());
        assert!(
            report
                .items
                .iter()
                .all(|item| item.provider == "catalog" && item.id.starts_with("provider:"))
        );
    }

    #[test]
    fn provider_item_sub_items_stay_attached_to_parent() {
        struct SubItemProvider;

        impl Provider for SubItemProvider {
            fn name(&self) -> &'static str {
                "sub"
            }

            fn list_items(&self) -> Vec<ProviderItem> {
                vec![ProviderItem {
                    id: "base".to_string(),
                    title: "Base".to_string(),
                    subtitle: "Launch base".to_string(),
                    info: ItemInfo {
                        summary: "Base launcher".to_string(),
                        fields: vec![],
                    },
                    action: ItemAction::ShellCommand("demo".to_string()),
                    require_sub_item: false,
                    sub_items: vec![crate::models::ActionSubItem {
                        id: "with-flag".to_string(),
                        title: "With Flag".to_string(),
                        subtitle: "Launch with extra flag".to_string(),
                        flags: vec!["--example".to_string()],
                        exit_after: None,
                        require_sub_item: false,
                        input: None,
                        sub_items: vec![],
                    }],
                }]
            }
        }

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(SubItemProvider)];
        let report = load_provider_items(&providers);

        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].id, "base");
        assert_eq!(report.items[0].sub_items.len(), 1);
    }

    #[test]
    fn dynamic_provider_command_generates_items() {
        let mut report = ProviderLoadReport::default();
        let doc = ExternalProviderDoc {
            name: "dynamic".to_string(),
            short_flag: Some("y".to_string()),
            items: vec![],
            command: None,
            shell_command: Some(
                "printf '%s' '[{".to_string()
                    + "\"id\":\"dyn-1\","
                    + "\"title\":\"Dyn 1\","
                    + "\"subtitle\":\"Generated\","
                    + "\"info\":{\"summary\":\"S\",\"fields\":[{\"label\":\"L\",\"value\":\"V\"}]},"
                    + "\"action\":{\"type\":\"shell_command\",\"value\":\"echo dyn\"}"
                    + "}]'"
            ),
        };

        append_external_doc(&mut report, "dynamic.json", &ProviderFilter::default(), doc);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].provider, "dynamic");
    }

    #[test]
    fn path_provider_helper_emits_items_when_invoked_by_tuisual() {
        let previous = std::env::var_os("TUISUAL_PROVIDER_MODE");
        // SAFETY: single-threaded test context
        unsafe { std::env::set_var("TUISUAL_PROVIDER_MODE", "1"); }

        let result = super::run_provider_binary_command("./target/debug/path_commands_provider");

        if let Some(old) = previous {
            unsafe { std::env::set_var("TUISUAL_PROVIDER_MODE", old); }
        } else {
            unsafe { std::env::remove_var("TUISUAL_PROVIDER_MODE"); }
        }

        let items = result.expect("PATH provider should emit JSON items");
        assert!(!items.is_empty(), "PATH provider emitted no items");
    }

    #[test]
    fn dynamic_provider_requires_items_or_command_output() {
        let mut report = ProviderLoadReport::default();
        let doc = ExternalProviderDoc {
            name: "empty".to_string(),
            short_flag: None,
            items: vec![],
            command: None,
            shell_command: None,
        };

        append_external_doc(&mut report, "empty.json", &ProviderFilter::default(), doc);
        assert_eq!(report.items.len(), 0);
        assert_eq!(report.rejected.len(), 1);
    }

    #[test]
    fn short_flag_p_loads_path_provider_items() {
        let report = load_all_items_from_args(&["-p".to_string()]);

        assert!(!report.items.is_empty(), "-p should load provider items");
        assert!(
            report.items.iter().any(|item| item.provider == "path-commands"),
            "-p should include PATH command items"
        );
    }

    #[test]
    fn dynamic_provider_rejects_ambiguous_command_fields() {
        let mut report = ProviderLoadReport::default();
        let doc = ExternalProviderDoc {
            name: "ambiguous".to_string(),
            short_flag: None,
            items: vec![],
            command: Some("./target/debug/path_commands_provider".to_string()),
            shell_command: Some("printf '[]'".to_string()),
        };

        append_external_doc(&mut report, "ambiguous.json", &ProviderFilter::default(), doc);

        assert!(report.items.is_empty());
        assert_eq!(report.rejected.len(), 1);
        assert!(report.rejected[0].contains("only one of 'command' or 'shell_command'"));
    }

}
