use crate::models::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

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
    items: Vec<ProviderItem>,
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

            if let Some(shorts) = arg.strip_prefix('-') {
                if !shorts.is_empty() {
                    for ch in shorts.chars() {
                        filter.short_flags.insert(ch.to_ascii_lowercase());
                    }
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
    for raw in doc.items {
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
    let built_in = load_provider_items_filtered(&default_providers(), &filter);
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
    let launch_hint = match descriptor.short_flag {
        Some(short) => format!(
            "Launch provider with --{} or -{}",
            descriptor.name, short
        ),
        None => format!("Launch provider with --{}", descriptor.name),
    };

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
        action: ItemAction::ProviderHint(launch_hint),
    }
}

pub fn default_providers() -> Vec<Box<dyn Provider>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalProviderDoc, Provider, ProviderFilter, ProviderLoadReport, append_external_doc,
        load_all_items, load_provider_items, merge_reports, parse_external_provider_doc,
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
            }],
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
}
