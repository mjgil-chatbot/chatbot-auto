const COMPLETION_PATTERNS: &[&str] = &[
    "no next",
    "no further",
    "nothing left",
    "nothing else",
    "no more work",
    "the work is complete",
    "all work is complete",
    "no remaining work",
    "no further work",
];
fn normalize_for_matching(content: &str) -> String {
    content
        .chars()
        .map(|ch| match ch {
            '*' | '_' | '`' | '#' => ' ',
            _ => ch.to_ascii_lowercase(),
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn content_indicates_completion(content: &str) -> bool {
    let normalized = normalize_for_matching(content);
    if normalized.contains("design doc fully implemented") {
        let explicitly_incomplete = [
            "design doc fully implemented: no",
            "design doc fully implemented - no",
            "design doc fully implemented no",
        ];
        if explicitly_incomplete
            .iter()
            .any(|pattern| normalized.contains(pattern))
        {
            return false;
        }
        return true;
    }

    COMPLETION_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}
pub fn content_mentions_next_step(content: &str) -> bool {
    let normalized = normalize_for_matching(content);

    // Check for explicit "Next Steps:" heading first — this overrides
    // "Design Doc Fully Implemented" since it explicitly requests next work
    if normalized.contains("next steps:") {
        return true;
    }

    if content_indicates_completion(&normalized) {
        return false;
    }

    let accept_patterns = [
        "next useful step",
        "next steps",
        "next step",
        "next batch",
        "can start",
        "ready for the next",
        "follow-up",
        "follow up",
        "remaining",
        "continue with",
        "still need",
        "left to",
        "next one",
    ];

    accept_patterns
        .iter()
        .any(|pattern| normalized.contains(pattern))
}
