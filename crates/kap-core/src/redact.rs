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
        (
            Regex::new(r"\bhf_[A-Za-z0-9]{20,}\b").unwrap(),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\brpa_[A-Za-z0-9]{20,}\b").unwrap(),
            "[REDACTED]",
        ),
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
            Regex::new(
                r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .unwrap(),
            "[REDACTED PRIVATE KEY]",
        ),
        // `user:secret@host` in a URL. The scheme and user stay readable.
        (
            Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://[^/\s:@]+:)[^@\s/]+@").unwrap(),
            "${1}[REDACTED]@",
        ),
        (
            Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}").unwrap(),
            "${1} [REDACTED]",
        ),
        // A credential named by its key: `RUNPOD_API_KEY=…`, `token: …`, `"api_key": "…"`. The
        // name must end in the credential word, so `num_tokens=12000000` is left alone, while a
        // prefix joined by `_` still counts — `\b` alone does not see `API_KEY` inside it.
        (
            Regex::new(&format!(
                r#"(?i)\b([A-Za-z0-9_.-]*{SECRET_NAME})(["']?\s*[:=]\s*["']?)[^\s"',;]{{8,}}"#
            ))
            .unwrap(),
            "${1}${2}[REDACTED]",
        ),
        // The same credential passed as a flag: `--password …`, `--hf-token=…`.
        (
            Regex::new(&format!(
                r#"(?i)((?:^|\s)--?[a-z0-9-]*{SECRET_NAME})([=\s]+)[^\s"']{{4,}}"#
            ))
            .unwrap(),
            "${1}${2}[REDACTED]",
        ),
    ]
});

/// Key names that hold a credential, matched at the end of a longer name.
const SECRET_NAME: &str = r"(?:api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|token|secret|password|passwd|authorization|credentials?)\b";

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
        for (sample, secret) in [
            ("HF_TOKEN=hf_abcdefghijklmnopqrstuvwxyz", "abcdefghijklmnop"),
            (
                "export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz",
                "abcdefghijklmnop",
            ),
            (
                "token: ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                "abcdefghijklmnop",
            ),
            ("aws AKIAIOSFODNN7EXAMPLE", "IOSFODNN7EXAMPLE"),
            ("runpod rpa_abcdefghijklmnopqrstuvwxyz", "abcdefghijklmnop"),
            ("export RUNPOD_API_KEY=Zq81mXk20Lp3nV7c", "Zq81mXk20Lp3nV7c"),
            (
                "WANDB_API_KEY: 0123456789abcdef0123",
                "0123456789abcdef0123",
            ),
            (r#"{"api_key": "Zq81mXk20Lp3nV7c"}"#, "Zq81mXk20Lp3nV7c"),
            ("Authorization: Bearer Zq81mXk20Lp3nV7c", "Zq81mXk20Lp3nV7c"),
            (
                "git clone https://user:Zq81mXk20Lp3nV7c@github.com/x/y",
                "Zq81mXk20Lp3nV7c",
            ),
            ("token: Zq81mXk20Lp3nV7c", "Zq81mXk20Lp3nV7c"),
            ("login --password Zq81mXk2", "Zq81mXk2"),
            ("--password Zq81mXk2", "Zq81mXk2"),
            ("run --hf-token=Zq81mXk20Lp3nV7c", "Zq81mXk20Lp3nV7c"),
        ] {
            let (out, changed) = text(sample);
            assert!(changed, "not redacted: {sample}");
            assert!(!out.contains(secret), "secret survived: {out}");
            assert!(out.contains("[REDACTED]"), "not marked: {out}");
        }
    }

    #[test]
    fn keeps_the_name_of_what_was_redacted() {
        let (out, _) = text("export RUNPOD_API_KEY=Zq81mXk20Lp3nV7c");
        assert_eq!(out, "export RUNPOD_API_KEY=[REDACTED]");
        let (out, _) = text("git clone https://user:Zq81mXk20Lp3nV7c@github.com/x/y");
        assert_eq!(out, "git clone https://user:[REDACTED]@github.com/x/y");
    }

    #[test]
    fn leaves_training_logs_alone() {
        for sample in [
            "step 1200  loss 2.314  num_tokens=12000000",
            "tokens: 123456789 seen",
            "max_new_tokens=2048 tokenizer=mimi_tokenizer_v2",
            "eos_token: </s>",
            "https://huggingface.co/datasets/x/y",
            "secret_sauce=dropout0.1",
        ] {
            let (out, changed) = text(sample);
            assert!(!changed, "redacted ordinary text: {sample} -> {out}");
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
