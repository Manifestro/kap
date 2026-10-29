use std::sync::LazyLock;

use regex::Regex;

/// Shapes that are almost always credentials, redacted before anything is stored.
///
/// The list is deliberately conservative: a false positive loses a little context, a false
/// negative writes a key to disk and then to a git remote.
static TEXT_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"(?i)\b(sk|rk)-[a-z0-9_-]{16,}").unwrap(),
            "[REDACTED]",
        ),
        (Regex::new(r"\bhf_[A-Za-z0-9]{20,}\b").unwrap(), "[REDACTED]"),
        (
            Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b").unwrap(),
            "[REDACTED]",
        ),
        (Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(), "[REDACTED]"),
        (
            Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b").unwrap(),
            "[REDACTED]",
        ),
        (
            Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
                .unwrap(),
            "[REDACTED PRIVATE KEY]",
        ),
        (
            Regex::new(
                r#"(?i)\b(api[_-]?key|access[_-]?token|secret[_-]?key|auth[_-]?token|hf[_-]?token|password|passwd|authorization)\b\s*[:=]\s*["']?[^\s"']{8,}"#,
            )
            .unwrap(),
            "[REDACTED]",
        ),
    ]
});

/// Redact credentials in text. Returns the text and whether anything was removed.
pub fn text(input: &str) -> (String, bool) {
    let mut output = input.to_string();
    let mut changed = false;

    for (pattern, replacement) in TEXT_PATTERNS.iter() {
        if pattern.is_match(&output) {
            output = pattern.replace_all(&output, *replacement).into_owned();
            changed = true;
        }
    }

    (output, changed)
}

/// Redact credentials in bytes, when the bytes are text.
///
/// Binary payloads are left alone: they are not where a token is printed, and decoding them would
/// corrupt the artifact.
pub fn bytes(input: &[u8]) -> (Vec<u8>, bool) {
    let Ok(content) = std::str::from_utf8(input) else {
        return (input.to_vec(), false);
    };

    let (redacted, changed) = text(content);
    if changed {
        (redacted.into_bytes(), true)
    } else {
        (input.to_vec(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_tokens() {
        for sample in [
            "HF_TOKEN=hf_abcdefghijklmnopqrstuvwxyz",
            "export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz",
            "token: ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "aws AKIAIOSFODNN7EXAMPLE",
            "runpod rpa_abcdefghijklmnopqrstuvwxyz",
        ] {
            let (out, changed) = text(sample);
            if sample.contains("rpa_") {
                // Not a shape we claim to know.
                assert!(!changed);
            } else {
                assert!(changed, "not redacted: {sample}");
                assert!(out.contains("[REDACTED]"), "not redacted: {sample}");
            }
        }
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let (out, changed) = text("cargo test --all\nrunning 39 tests");
        assert!(!changed);
        assert_eq!(out, "cargo test --all\nrunning 39 tests");
    }

    #[test]
    fn leaves_binary_alone() {
        let raw = [0u8, 159, 146, 150];
        let (out, changed) = bytes(&raw);
        assert!(!changed);
        assert_eq!(out, raw);
    }
}
