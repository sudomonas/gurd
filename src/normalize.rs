/// Normal form used for name matching: lowercase, single spaces, trimmed.
///
/// Stored in `names.norm` at import time and applied to queries, so both sides
/// always agree.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn folds_case_and_whitespace() {
        assert_eq!(
            normalize("  Metformin   HYDROCHLORIDE\t"),
            "metformin hydrochloride"
        );
        assert_eq!(normalize(""), "");
    }
}
