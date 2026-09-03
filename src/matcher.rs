use crate::models::AppItem;

// This is the result of a search.
// Each item gets a number. Bigger numbers mean "this item matches the user's query better".
// The UI later uses this list to show the best match first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedItem {
    pub index: usize,
    pub score: i64,
}

// This is the main search function.
// It takes the text the user typed and the full list of possible items,
// and then decides which items are a good match and in what order.
pub fn rank_items(query: &str, items: &[AppItem]) -> Vec<RankedItem> {
    // If the user typed nothing, we don't want to filter anything.
    // Every item gets the same score of 0, and the list stays in its original order.
    if query.trim().is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(index, _)| RankedItem { index, score: 0 })
            .collect();
    }

    // Convert the user's input to lowercase so matching is case-insensitive.
    // Example: "Git" and "git" should be treated the same.
    let query_lower = query.to_lowercase();

    // Break the query into a list of characters so we can check whether the candidate
    // string contains them in order.
    // Example: query = "alp" becomes ['a', 'l', 'p'].
    let qchars: Vec<char> = query_lower.chars().collect();

    // This will store the items that match, together with their score.
    let mut ranked = Vec::with_capacity(items.len());

    // Check each item one by one.
    for (index, item) in items.iter().enumerate() {
        // score_item returns Some(score) if the item matches, or None if it does not.
        if let Some(score) = score_item(query, &query_lower, &qchars, item) {
            ranked.push(RankedItem { index, score });
        }
    }

    // Sort from highest score to lowest score.
    // If two items have the same score, use the original index as a tie-breaker.
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));

    ranked
}

// This is the score calculator for a single item.
// The item is checked against three places:
// 1. title
// 2. subtitle
// 3. id
//
// The title gets the strongest bonus because it is usually the most important label.
// The subtitle also matters, but less. The id is checked last and gets a small penalty,
// because the id is often technical and not what the user wants to see first.
fn score_item(query: &str, query_lower: &str, qchars: &[char], item: &AppItem) -> Option<i64> {
    // Score the item's title. It gets a big boost because a title is often the primary label.
    let title_score =
        score_candidate(query, query_lower, qchars, &item.title).map(|score| score + 600);

    // Score the subtitle. This is useful when the item description contains relevant words.
    let subtitle_score = score_candidate(query, query_lower, qchars, &item.subtitle);

    // Score the id as a weaker match. An id can be technical or shorthand, so it matters less.
    let id_score = score_candidate(query, query_lower, qchars, &item.id).map(|score| score - 120);

    // Take the highest score among the three places.
    // This means an item can match well in one field even if it is weak in another.
    [title_score, subtitle_score, id_score]
        .into_iter()
        .flatten()
        .max()
}

// This function scores one single text field against the query.
// Example: query = "alp" and candidate = "alpha".
//
// The algorithm has several rules:
// 1. If the candidate is exactly the same as the query, it gets a very high score.
// 2. If the candidate starts with the query, it also gets a very high score.
// 3. Otherwise, the code checks whether the query letters appear in order inside the candidate.
//    Example: "gti" can match "git" because g -> t -> i appear in that sequence.
// 4. The score goes up if the match is at the start of the string or right after a separator
//    like a space, underscore, dash, slash, or dot.
// 5. Longer candidates are slightly punished so shorter, better matches rise to the top.
fn score_candidate(
    query: &str,
    query_lower: &str,
    qchars: &[char],
    candidate: &str,
) -> Option<i64> {
    // Turn the candidate into lowercase so matching is case-insensitive.
    let candidate_lower = candidate.to_lowercase();

    // Rule 1: exact match.
    // If the query is exactly the same as the candidate, that is the strongest possible match.
    // We subtract the length so a shorter exact match beats a longer exact match.
    if query_lower == candidate_lower {
        return Some(10_000 - candidate.len() as i64);
    }

    // Rule 2: prefix match.
    // If the candidate starts with the query, it is a strong match.
    // Example: query = "git", candidate = "git-status".
    if candidate_lower.starts_with(query_lower) {
        return Some(7_500 - candidate.len() as i64 + (query.len() as i64 * 20));
    }

    // Rule 3: subsequence match.
    // This is the "in-order letters" check.
    // We walk through the candidate from left to right and try to match the query letters
    // one by one.
    let mut score: i64 = 0;
    // `qidx` points at the next query character we still need to find.
    let mut qidx = 0usize;
    // Remember the previous candidate position that matched. This lets us reward
    // matching characters that are next to one another.
    let mut last_match: Option<usize> = None;
    // Remember the character immediately before the current candidate character.
    // This lets us detect word and option separators.
    let mut previous_char = None;

    for (idx, c) in candidate_lower.chars().enumerate() {
        // Stop once we have matched every query character.
        if qidx >= qchars.len() {
            break;
        }

        // If the current character matches the next query character, count it as a match.
        if c == qchars[qidx] {
            // Every matched character adds some points.
            score += 100;

            // Bonus if the match starts at the beginning of the candidate.
            // Example: "git" matches "gitlab" and starts at index 0.
            if idx == 0 {
                score += 60;
            }

            // Bonus if the match continues immediately after the previous match.
            // Example: "git" found as "g i t" with no gaps, or "gi" "t" in sequence.
            if let Some(prev) = last_match
                && idx == prev + 1
            {
                score += 50;
            }

            // Bonus if the match happens right after a separator such as space, underscore,
            // dash, slash, or dot.
            // Example: "cal" in "calendar" or "git" in "git-status".
            if let Some(prev_char) = previous_char
                && matches!(prev_char, ' ' | '_' | '-' | '/' | '.')
            {
                score += 40;
            }

            // Save the current match position so the next comparison knows what came before.
            last_match = Some(idx);
            qidx += 1;
        }

        // Save the current character so the next loop iteration can check what came before it.
        previous_char = Some(c);
    }

    // If we did not match every query letter, then this candidate is not a valid match.
    if qidx != qchars.len() {
        return None;
    }

    // Add a bonus based on query length and subtract a little for longer candidate strings.
    // This helps short, relevant matches outrank long, less relevant ones.
    score += (query_lower.len() as i64) * 40;
    score -= (candidate_lower.len() as i64) * 2;

    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppItem, InfoField, ItemAction, ItemInfo, ProviderItem};

    // Build a small valid item for matcher tests. The command and metadata are kept
    // simple because these tests are checking ranking, not provider loading.
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

    // A title that begins with the query should beat a title where the same letters
    // are spread across the text as a weaker subsequence match.
    #[test]
    fn prefix_match_ranks_above_subsequence() {
        let items = vec![
            test_item("alpha", "echo alpha"),
            test_item("beta-alpha", "echo beta-alpha"),
        ];

        let ranked = rank_items("alp", &items);
        assert_eq!(ranked[0].index, 0);
    }

    // Search is not limited to the visible title. Provider authors can put useful
    // alternate names in the subtitle, and the matcher should find those names too.
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
