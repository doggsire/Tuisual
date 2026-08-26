use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ItemAction {
    ShellCommand(String),
    ShellCommandExit(String),
    ShellCommandWithFlag(ShellCommandWithFlag),
    ProviderHint(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShellCommandWithFlag {
    pub command: String,
    pub flag_prefix: String,
    pub prompt: String,
    #[serde(default)]
    pub exit_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionSubItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub exit_after: Option<bool>,
    #[serde(default)]
    pub require_sub_item: bool,
    #[serde(default)]
    pub input: Option<SubItemInput>,
    #[serde(default)]
    pub sub_items: Vec<ActionSubItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubItemInput {
    pub flag_prefix: String,
    pub prompt: String,
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
    #[serde(default)]
    pub require_sub_item: bool,
    #[serde(default)]
    pub sub_items: Vec<ActionSubItem>,
}

#[derive(Debug, Clone)]
pub struct AppItem {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub info: ItemInfo,
    pub action: ItemAction,
    pub require_sub_item: bool,
    pub sub_items: Vec<ActionSubItem>,
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

        for sub_item in &self.sub_items {
            validate_sub_item(sub_item)?;
        }

        if self.require_sub_item && self.sub_items.is_empty() {
            return Err("require_sub_item=true requires at least one sub_item".to_string());
        }

        match &self.action {
            ItemAction::ShellCommand(command) => {
                if command.trim().is_empty() {
                    return Err("shell command must not be empty".to_string());
                }
            }
            ItemAction::ShellCommandExit(command) => {
                if command.trim().is_empty() {
                    return Err("shell command exit must not be empty".to_string());
                }
            }
            ItemAction::ShellCommandWithFlag(config) => {
                if config.command.trim().is_empty() {
                    return Err("shell command with flag command must not be empty".to_string());
                }
                if config.flag_prefix.trim().is_empty() {
                    return Err("shell command with flag prefix must not be empty".to_string());
                }
                if config.prompt.trim().is_empty() {
                    return Err("shell command with flag prompt must not be empty".to_string());
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
            require_sub_item: item.require_sub_item,
            sub_items: item.sub_items,
        })
    }
}

fn validate_sub_item(sub_item: &ActionSubItem) -> Result<(), String> {
    if sub_item.id.trim().is_empty() {
        return Err("sub_items.id must not be empty".to_string());
    }
    if sub_item.title.trim().is_empty() {
        return Err("sub_items.title must not be empty".to_string());
    }
    for flag in &sub_item.flags {
        if flag.trim().is_empty() {
            return Err("sub_items.flags entries must not be empty".to_string());
        }
    }
    if let Some(input) = &sub_item.input {
        if input.prompt.trim().is_empty() {
            return Err("sub_items.input.prompt must not be empty".to_string());
        }
    }
    if sub_item.require_sub_item && sub_item.sub_items.is_empty() {
        return Err("sub_items.require_sub_item=true requires nested sub_items".to_string());
    }
    for nested in &sub_item.sub_items {
        validate_sub_item(nested)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ActionSubItem, AppItem, InfoField, ItemAction, ItemInfo, ProviderItem, SubItemInput,
    };

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
            require_sub_item: false,
            sub_items: vec![],
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
            require_sub_item: false,
            sub_items: vec![],
        };

        assert!(AppItem::from_provider_item("mock", item).is_ok());
    }

    #[test]
    fn app_item_accepts_sub_item_input_with_empty_flag_prefix() {
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
            action: ItemAction::ShellCommandExit("echo hi".to_string()),
            require_sub_item: false,
            sub_items: vec![ActionSubItem {
                id: "custom-args".to_string(),
                title: "Custom Args".to_string(),
                subtitle: "Type anything".to_string(),
                flags: vec![],
                exit_after: Some(true),
                require_sub_item: false,
                input: Some(SubItemInput {
                    flag_prefix: "".to_string(),
                    prompt: "Enter flags/args".to_string(),
                }),
                sub_items: vec![],
            }],
        };

        assert!(AppItem::from_provider_item("mock", item).is_ok());
    }
}
