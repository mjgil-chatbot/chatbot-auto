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

#[cfg(test)]
mod tests {
    use super::{content_indicates_completion, content_mentions_next_step};

    #[test]
    fn accepts_next_steps_language() {
        assert!(content_mentions_next_step(
            "Tasks Accomplished\n\nNext Steps:\n1. Continue with parser cleanup."
        ));
        assert!(content_mentions_next_step(
            "There is remaining integration work."
        ));
    }

    #[test]
    fn rejects_completion_language() {
        assert!(!content_mentions_next_step("There is no more work left."));
        assert!(!content_mentions_next_step("All work is complete."));
        assert!(!content_mentions_next_step("Design Doc Fully Implemented"));
        assert!(content_indicates_completion("There is no more work left."));
        assert!(content_indicates_completion("Design Doc Fully Implemented"));
    }

    #[test]
    fn accepts_design_doc_not_fully_implemented_status_with_next_steps() {
        let content = "## Next Steps:\n\
Task 210 remains blocked on zigmera-lowering.\n\
Design Doc Fully Implemented: NO - Tasks 210-211 remain.";
        assert!(content_mentions_next_step(content));
        assert!(!content_indicates_completion(content));
    }

    #[test]
    fn accepts_markdown_emphasized_incomplete_status() {
        let content = "## Next Steps:\n\
Task 210 remains blocked on zigmera-lowering.\n\
Design Doc Fully Implemented: **NO** - Tasks 210-211 remain.";
        assert!(content_mentions_next_step(content));
        assert!(!content_indicates_completion(content));
    }

    #[test]
    fn rejects_irrelevant_text() {
        assert!(!content_mentions_next_step(
            "Implemented the change and added tests."
        ));
    }

    #[test]
    fn explicit_next_steps_overrides_completion() {
        // When content has an explicit "Next Steps:" section heading,
        // it should be treated as having next steps even if it also
        // contains "Design Doc Fully Implemented"
        let content =
            "**Next Steps:**\n1. **B.16 (Formal Lean Proofs)**: Implement Chimera/Beam.\n\
             **Design Doc Fully Implemented** — Phase 5.6 (B.1-B.14) is complete.";
        assert!(
            content_mentions_next_step(content),
            "Explicit Next Steps: heading should override Design Doc Fully Implemented"
        );
    }
}
