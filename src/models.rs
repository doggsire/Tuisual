use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ItemAction {
    ShellCommand(String),
    ProviderHint(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoField {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ItemInfo {
    pub summary: String,
    pub fields: Vec<InfoField>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub info: ItemInfo,
    pub action: ItemAction,
}

#[derive(Debug, Clone)]
pub struct AppItem {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub info: ItemInfo,
    pub action: ItemAction,
}

impl ProviderItem {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("id must not be empty".to_string());
        }

        if self.title.trim().is_empty() {
            return Err("title must not be empty".to_string());
        }

        if self.info.summary.trim().is_empty() {
            return Err("info.summary must not be empty".to_string());
        }

        for field in &self.info.fields {
            if field.label.trim().is_empty() {
                return Err("info.fields.label must not be empty".to_string());
            }
            if field.value.trim().is_empty() {
                return Err("info.fields.value must not be empty".to_string());
            }
        }

        match &self.action {
            ItemAction::ShellCommand(command) => {
                if command.trim().is_empty() {
                    return Err("shell command must not be empty".to_string());
                }
            }
            ItemAction::ProviderHint(message) => {
                if message.trim().is_empty() {
                    return Err("provider hint must not be empty".to_string());
                }
            }
        }

        Ok(())
    }
}

impl AppItem {
    pub fn from_provider_item(provider_name: &str, item: ProviderItem) -> Result<Self, String> {
        if provider_name.trim().is_empty() {
            return Err("provider name must not be empty".to_string());
        }

        item.validate()?;

        Ok(Self {
            provider: provider_name.to_string(),
            id: item.id,
            title: item.title,
            subtitle: item.subtitle,
            info: item.info,
            action: item.action,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};

    #[test]
    fn provider_item_validation_rejects_empty_title() {
        let item = ProviderItem {
            id: "test-id".to_string(),
            title: "".to_string(),
            subtitle: "x".to_string(),
            info: ItemInfo {
                summary: "Summary".to_string(),
                fields: vec![InfoField {
                    label: "A".to_string(),
                    value: "B".to_string(),
                }],
            },
            action: ItemAction::ShellCommand("echo hi".to_string()),
        };

        assert!(item.validate().is_err());
    }

    #[test]
    fn app_item_conversion_accepts_valid_payload() {
        let item = ProviderItem {
            id: "test-id".to_string(),
            title: "Title".to_string(),
            subtitle: "x".to_string(),
            info: ItemInfo {
                summary: "Summary".to_string(),
                fields: vec![InfoField {
                    label: "A".to_string(),
                    value: "B".to_string(),
                }],
            },
            action: ItemAction::ShellCommand("echo hi".to_string()),
        };

        assert!(AppItem::from_provider_item("mock", item).is_ok());
    }
}
