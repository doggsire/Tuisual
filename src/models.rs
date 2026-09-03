use serde::{Deserialize, Serialize};

// A single action describes what happens when a user picks an item.
// This is like a tiny script for a menu choice.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ItemAction {
    ShellCommand(String),
    ShellCommandExit(String),
    ShellCommandWithFlag(ShellCommandWithFlag),
    ProviderHint(String),
}

// Some commands need a flag typed in by the user, like "--help" or a package name.
// This struct stores the command template and the words shown to the user.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShellCommandWithFlag {
    pub command: String,
    pub flag_prefix: String,
    pub prompt: String,
    #[serde(default)]
    pub exit_after: bool,
}

// A submenu item is a child choice inside a bigger action.
// For example: "Run command" or "Install package" can be a sub-item.
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
    // `flag_prefix` is placed immediately before the text the user types.
    // It may be empty when the user is entering free-form arguments.
    pub flag_prefix: String,
    // `prompt` is the human-readable question shown while compose mode is open.
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InfoField {
    // This is the name shown on the left side of an information row.
    pub label: String,
    // This is the value shown beside the label.
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ItemInfo {
    // A short description displayed for the selected item.
    pub summary: String,
    // Extra labeled facts displayed below the summary.
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

#[derive(Debug, Clone, Serialize)]
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
    // Check provider data before allowing it into the running application.
    //
    // Providers may be built-in programs, external JSON files, or separate binaries.
    // They all produce the same ProviderItem shape, so this is the shared gate that
    // catches bad data before the UI tries to display or execute it.
    pub fn validate(&self) -> Result<(), String> {
        // An ID identifies the item internally, so an empty ID would make it difficult
        // to distinguish this item from other items.
        if self.id.trim().is_empty() {
            return Err("id must not be empty".to_string());
        }

        // The title is what the user sees in the result list. Without it, the item
        // would have no useful name to display or search for.
        if self.title.trim().is_empty() {
            return Err("title must not be empty".to_string());
        }

        // The summary is shown in the information panel, so it must contain something
        // meaningful instead of only spaces.
        if self.info.summary.trim().is_empty() {
            return Err("info.summary must not be empty".to_string());
        }

        // Validate every information row one at a time. The first bad row stops the
        // check and returns a message that identifies which part is missing.
        for field in &self.info.fields {
            if field.label.trim().is_empty() {
                return Err("info.fields.label must not be empty".to_string());
            }
            if field.value.trim().is_empty() {
                return Err("info.fields.value must not be empty".to_string());
            }
        }

        // Sub-items can contain more sub-items, so their validator calls itself again
        // for each child and checks the complete tree from the outside inward.
        for sub_item in &self.sub_items {
            validate_sub_item(sub_item)?;
        }

        // A required submenu must actually have a choice to follow.
        if self.require_sub_item && self.sub_items.is_empty() {
            return Err("require_sub_item=true requires at least one sub_item".to_string());
        }

        // Different action variants have different required pieces. Match on the
        // chosen variant so each kind can check the fields that only it owns.
        match &self.action {
            ItemAction::ShellCommand(command) => {
            // A command with no text cannot be executed.
                if command.trim().is_empty() {
                    return Err("shell command must not be empty".to_string());
                }
            }
            ItemAction::ShellCommandExit(command) => {
                // This variant also needs a real command, even though it exits afterward.
                if command.trim().is_empty() {
                    return Err("shell command exit must not be empty".to_string());
                }
            }
            ItemAction::ShellCommandWithFlag(config) => {
                // A prompted command needs a base command, a prefix to join the input,
                // and a prompt that tells the user what to type.
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
                // A provider hint must name the provider that should be loaded next.
                if message.trim().is_empty() {
                    return Err("provider hint must not be empty".to_string());
                }
            }
        }

        Ok(())
    }
}

impl AppItem {
    // Add the provider name to a validated ProviderItem.
    //
    // ProviderItem describes data produced by a provider. AppItem is the form used by
    // the application, where the origin is also recorded so the UI and provider-specific
    // behavior can identify where the item came from.
    pub fn from_provider_item(provider_name: &str, item: ProviderItem) -> Result<Self, String> {
        // Reject an unnamed provider before copying any fields into the app item.
        if provider_name.trim().is_empty() {
            return Err("provider name must not be empty".to_string());
        }

        // Validate first, so no partially trusted item is constructed.
        item.validate()?;

        // Move the already validated fields into the application-facing structure.
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

// Validate one submenu node and every nested node below it.
//
// This function is recursive because a child can contain another child. Each call checks
// the current node first, then repeats the same rules for its children.
fn validate_sub_item(sub_item: &ActionSubItem) -> Result<(), String> {
    // Every submenu option needs its own stable identifier and visible title.
    if sub_item.id.trim().is_empty() {
        return Err("sub_items.id must not be empty".to_string());
    }
    if sub_item.title.trim().is_empty() {
        return Err("sub_items.title must not be empty".to_string());
    }
    // Empty flag strings would create confusing extra spaces in the final command,
    // so reject them before command composition happens.
    for flag in &sub_item.flags {
        if flag.trim().is_empty() {
            return Err("sub_items.flags entries must not be empty".to_string());
        }
    }
    // Input prompts may intentionally have an empty flag prefix, but the prompt itself
    // must explain what the user is expected to type.
    if let Some(input) = &sub_item.input
        && input.prompt.trim().is_empty()
    {
        return Err("sub_items.input.prompt must not be empty".to_string());
    }
    // A required nested step must provide at least one nested choice to follow.
    if sub_item.require_sub_item && sub_item.sub_items.is_empty() {
        return Err("sub_items.require_sub_item=true requires nested sub_items".to_string());
    }
    // Walk deeper into the tree. If any descendant is invalid, pass its error upward.
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

    // This test creates an otherwise valid item with one missing required field.
    // Validation should reject it instead of allowing an unnamed result into the app.
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

    // This test builds a complete valid provider payload and sends it through the same
    // conversion function used by the real provider loader. A successful result proves
    // that valid data is accepted and receives its provider name.
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

    // This test protects an intentional rule: an input may have no prefix.
    // That lets the user type free-form arguments rather than forcing text such as
    // "name=" before the value. The prompt is still present, so the item remains valid.
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
