use crate::models::AppItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedItem {
    pub index: usize,
    pub score: i64,
}

pub fn rank_items(query: &str, items: &[AppItem]) -> Vec<RankedItem> {
    if query.trim().is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(index, _)| RankedItem { index, score: 0 })
            .collect();
    }

    let mut ranked = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(score) = score_item(query, item) {
            ranked.push(RankedItem { index, score });
        }
    }

    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.index.cmp(&b.index))
    });

    ranked
}

fn score_item(query: &str, item: &AppItem) -> Option<i64> {
    let title_score = score_candidate(query, &item.title).map(|score| score + 600);
    let subtitle_score = score_candidate(query, &item.subtitle);
    let id_score = score_candidate(query, &item.id).map(|score| score - 120);

    [title_score, subtitle_score, id_score]
        .into_iter()
        .flatten()
        .max()
}

fn score_candidate(query: &str, candidate: &str) -> Option<i64> {
    let query_lower = query.to_lowercase();
    let candidate_lower = candidate.to_lowercase();

    if query_lower == candidate_lower {
        return Some(10_000 - candidate.len() as i64);
    }

    if candidate_lower.starts_with(&query_lower) {
        return Some(7_500 - candidate.len() as i64 + (query.len() as i64 * 20));
    }

    let qchars: Vec<char> = query_lower.chars().collect();
    let cchars: Vec<char> = candidate_lower.chars().collect();

    let mut score: i64 = 0;
    let mut qidx = 0usize;
    let mut last_match: Option<usize> = None;

    for (idx, c) in cchars.iter().enumerate() {
        if qidx >= qchars.len() {
            break;
        }

        if *c == qchars[qidx] {
            score += 100;

            if idx == 0 {
                score += 60;
            }

            if let Some(prev) = last_match {
                if idx == prev + 1 {
                    score += 50;
                }
            }

            if idx > 0 {
                let prev_char = cchars[idx - 1];
                if matches!(prev_char, ' ' | '_' | '-' | '/' | '.') {
                    score += 40;
                }
            }

            last_match = Some(idx);
            qidx += 1;
        }
    }

    if qidx != qchars.len() {
        return None;
    }

    score += (query_lower.len() as i64) * 40;
    score -= (candidate_lower.len() as i64) * 2;

    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};

    fn test_item(title: &str, command: &str) -> AppItem {
        let provider_item = ProviderItem {
            id: title.to_lowercase(),
            title: title.to_string(),
            subtitle: String::new(),
            info: ItemInfo {
                summary: "test summary".to_string(),
                fields: vec![InfoField {
                    label: "k".to_string(),
                    value: "v".to_string(),
                }],
            },
            action: ItemAction::ShellCommand(command.to_string()),
            require_sub_item: false,
            sub_items: vec![],
        };

        AppItem::from_provider_item("matcher-test", provider_item).expect("valid test item")
    }

    #[test]
    fn prefix_match_ranks_above_subsequence() {
        let items = vec![
            test_item("alpha", "echo alpha"),
            test_item("beta-alpha", "echo beta-alpha"),
        ];

        let ranked = rank_items("alp", &items);
        assert_eq!(ranked[0].index, 0);
    }

    #[test]
    fn subtitle_terms_are_searchable() {
        let item = ProviderItem {
            id: "files".to_string(),
            title: "Files".to_string(),
            subtitle: "File manager | terms: nautilus browser".to_string(),
            info: ItemInfo {
                summary: "test summary".to_string(),
                fields: vec![InfoField {
                    label: "k".to_string(),
                    value: "v".to_string(),
                }],
            },
            action: ItemAction::ShellCommand("echo files".to_string()),
            require_sub_item: false,
            sub_items: vec![],
        };

        let app_item = AppItem::from_provider_item("matcher-test", item).expect("valid item");
        let ranked = rank_items("nautilus", &[app_item]);
        assert_eq!(ranked.len(), 1);
    }
}
