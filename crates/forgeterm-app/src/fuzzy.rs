//! Small fuzzy matcher for the Command Palette.
//!
//! The query must appear in the candidate as a case-insensitive subsequence. Scoring
//! rewards matches at word starts and runs of consecutive characters, and lightly
//! penalises gaps, so "spr" ranks "Split Right" above "Scroll Page Up".

/// Returns (score, matched char indices) or `None` if `query` does not match.
pub fn score(query: &str, candidate: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let word_start = |i: usize| {
        i == 0
            || matches!(chars[i - 1], ' ' | ':' | '-' | '_' | '/' | '\\' | '.')
            || (chars[i].is_uppercase() && chars[i - 1].is_lowercase())
    };

    // Try every starting position of the first query char and keep the best greedy
    // alignment; candidates are short (palette titles), so this is cheap.
    let mut best: Option<(i32, Vec<usize>)> = None;
    for start in (0..lower.len()).filter(|&i| lower[i] == query[0]) {
        let mut indices = vec![start];
        let mut pos = start + 1;
        let mut ok = true;
        for &qc in &query[1..] {
            // Prefer the next word-start occurrence if it comes before any gap would
            // otherwise be needed; else the nearest occurrence.
            let next = (pos..lower.len()).find(|&i| lower[i] == qc);
            let Some(next) = next else {
                ok = false;
                break;
            };
            let chosen = if next == pos {
                next
            } else {
                (pos..lower.len())
                    .find(|&i| lower[i] == qc && word_start(i))
                    .unwrap_or(next)
            };
            indices.push(chosen);
            pos = chosen + 1;
        }
        if !ok {
            continue;
        }
        let mut s = 0i32;
        for (k, &i) in indices.iter().enumerate() {
            s += 1;
            if word_start(i) {
                s += 8;
            }
            if k > 0 {
                let prev = indices[k - 1];
                if i == prev + 1 {
                    s += 5;
                } else {
                    s -= ((i - prev - 1) as i32).min(5);
                }
            }
        }
        // Earlier matches read as more relevant.
        s -= (start as i32).min(10);
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, indices));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank<'a>(query: &str, items: &[&'a str]) -> Vec<&'a str> {
        let mut scored: Vec<(i32, &str)> = items
            .iter()
            .filter_map(|c| score(query, c).map(|(s, _)| (s, *c)))
            .collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        scored.into_iter().map(|(_, c)| c).collect()
    }

    #[test]
    fn subsequence_required_and_case_insensitive() {
        assert!(score("spr", "Pane: Split Right").is_some());
        assert!(score("SPLIT", "Pane: Split Right").is_some());
        assert!(score("xyz", "Pane: Split Right").is_none());
        // Order matters: "tp" fits "Tab: Previous", "vp" has no "p" after the "v".
        assert!(score("tp", "Tab: Previous Tab").is_some());
        assert!(score("vp", "Tab: Previous Tab").is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("", "anything"), Some((0, vec![])));
        assert_eq!(score("  ", "anything"), Some((0, vec![])));
    }

    #[test]
    fn word_starts_beat_scattered_letters() {
        let items = [
            "Terminal: Scroll Page Up",
            "Pane: Split Right",
            "Tab: Previous Tab",
        ];
        assert_eq!(rank("spr", &items)[0], "Pane: Split Right");

        let ranked = rank(
            "nt",
            &["Terminal: Restart Shell", "Tab: New Tab", "Tab: Next Tab"],
        );
        assert_eq!(ranked.len(), 3);
        assert_eq!(
            ranked[2], "Terminal: Restart Shell",
            "scattered match ranks last"
        );
    }

    #[test]
    fn consecutive_prefix_wins() {
        let items = [
            "Terminal: Clear Scrollback",
            "Edit: Copy Selection",
            "Tab: Close Tab",
        ];
        assert_eq!(rank("clear", &items)[0], "Terminal: Clear Scrollback");
        assert_eq!(rank("copy", &items)[0], "Edit: Copy Selection");
    }

    #[test]
    fn indices_point_at_matched_chars() {
        let (_, idx) = score("nt", "Tab: New Tab").unwrap();
        let chars: Vec<char> = "Tab: New Tab".chars().collect();
        assert_eq!(
            idx.iter()
                .map(|&i| chars[i].to_ascii_lowercase())
                .collect::<String>(),
            "nt"
        );
        // "N" of New (5) and the word-start "T" of the second Tab (9).
        assert_eq!(idx, vec![5, 9]);
    }
}
