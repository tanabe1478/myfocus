use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// fzf-style fuzzy match over (id, text) pairs. Returns ids ranked by score.
pub fn fuzzy_rank(corpus: &[(i64, String)], query: &str, limit: usize) -> Vec<i64> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut buf = Vec::new();
    let mut scored: Vec<(u32, i64)> = corpus
        .iter()
        .filter_map(|(id, text)| {
            let haystack = Utf32Str::new(text, &mut buf);
            pattern.score(haystack, &mut matcher).map(|s| (s, *id))
        })
        .collect();

    scored.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(limit).map(|(_, id)| id).collect()
}
