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
        // A JSON Web Token: three base64url parts, the first two starting with `eyJ`.
        (
            Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}").unwrap(),
            "[REDACTED]",
        ),
    ]
});

/// A credential named by its key, as `(name)(separator)(value)`. Only a literal value is
/// redacted — see [`is_literal`] — so code that passes a key around stays readable.
static NAMED_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // `RUNPOD_API_KEY=…`, `token: …`, `"api_key": "…"`. The name must end in the credential
        // word, so `num_tokens=12000000` is left alone, while a prefix joined by `_` still
        // counts — `\b` alone does not see `API_KEY` inside it.
        Regex::new(&format!(
            r#"(?i)\b([A-Za-z0-9_.-]*{SECRET_NAME})(["']?\s*[:=]\s*["']?)([^\s"',;]{{8,}})"#
        ))
        .unwrap(),
        // netrc style: `password <value>` with no `=` or `:`.
        Regex::new(r#"(?i)\b(password|passwd)(\s+)([^\s"']{8,})"#).unwrap(),
        // A flag: `--password …`, `--hf-token=…`.
        Regex::new(&format!(
            r#"(?i)((?:^|\s)--?[a-z0-9-]*{SECRET_NAME})([=\s]+)([^\s"']{{4,}})"#
        ))
        .unwrap(),
    ]
});

/// Whether a value after a credential name is the credential itself rather than code.
///
/// `api_key=args.api_key`, `pad_token = tokenizer.eos_token`, `password=$GH_TOKEN` and
/// `token = os.environ.get(...)` are how code handles a key, not a key. Real keys carry digits;
/// a letters-only password (`password=correcthorse`) is the accepted miss.
pub fn is_literal(value: &str) -> bool {
    if value.starts_with(['$', '{', '<', '%', '(', '[', '@', '`']) || value.contains('(') {
        return false;
    }
    if !value.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let dotted_name = value.contains('.')
        && value.split('.').all(|part| {
            part.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    !dotted_name
}

/// Key names that hold a credential, matched at the end of a longer name.
const SECRET_NAME: &str = r"(?:api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|token|secret|password|passwd|authorization|credentials?)\b";

/// Runs of base62-ish characters long enough to be a key. `-`, `_`, `.` and `/` end a run, so
/// item ids like `c0062-f09-af_nova-en_US-amy-medium` and paths are never one token.
static RANDOM_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9+=]{32,}").unwrap());

/// Bits per character above which a run reads as random. Measured on the dauys repository (66 MB
/// of code, logs and reports): no ordinary run reaches it, while 99.6% of random 32-character
/// base62 keys do.
const RANDOM_ENTROPY_BITS: f64 = 4.0;

/// Whether a run looks like a generated key rather than a word, a hash, or a number.
///
/// Mixed case plus digits rules out hex digests (git, BLAKE3) and plain identifiers; entropy rules
/// out long words. A hex key without a telling name is indistinguishable from a commit hash and
/// is deliberately not caught.
pub fn looks_random(run: &str) -> bool {
    let has = |test: fn(&char) -> bool| run.chars().any(|c| test(&c));
    if !(has(char::is_ascii_uppercase)
        && has(char::is_ascii_lowercase)
        && has(char::is_ascii_digit))
    {
        return false;
    }

    let mut counts = std::collections::HashMap::new();
    for c in run.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let length = run.chars().count() as f64;
    let entropy: f64 = counts
        .values()
        .map(|&n| {
            let p = n as f64 / length;
            -p * p.log2()
        })
        .sum();
    entropy >= RANDOM_ENTROPY_BITS
}

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

    for pattern in NAMED_PATTERNS.iter() {
        if pattern
            .captures_iter(&output)
            .any(|caps| is_literal(&caps[3]))
        {
            output = pattern
                .replace_all(&output, |caps: &regex::Captures<'_>| {
                    if is_literal(&caps[3]) {
                        format!("{}{}[REDACTED]", &caps[1], &caps[2])
                    } else {
                        caps[0].to_string()
                    }
                })
                .into_owned();
            changed = true;
        }
    }

    // A key with no telling name or prefix is caught by how random it looks.
    if RANDOM_RUN
        .find_iter(&output)
        .any(|run| looks_random(run.as_str()))
    {
        output = RANDOM_RUN
            .replace_all(&output, |caps: &regex::Captures<'_>| {
                let run = &caps[0];
                if looks_random(run) {
                    "[REDACTED]".to_string()
                } else {
                    run.to_string()
                }
            })
            .into_owned();
        changed = true;
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
    fn redacts_keys_without_a_name() {
        for (sample, secret) in [
            ("key: q7Lr2VxN9mKp4TzB8wYc1HdF6jGs3EaU", "q7Lr2VxN9mKp4TzB8wYc1HdF6jGs3EaU"),
            ("q7Lr2VxN9mKp4TzB8wYc1HdF6jGs3EaU", "q7Lr2VxN9mKp4TzB8wYc1HdF6jGs3EaU"),
            (
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk",
                "dBjftJeZ4CVP",
            ),
            (
                "machine api.wandb.ai login user password 0123456789abcdef0123456789abcdef01234567",
                "0123456789abcdef",
            ),
        ] {
            let (out, changed) = text(sample);
            assert!(changed, "not redacted: {sample}");
            assert!(!out.contains(secret), "secret survived: {out}");
        }
    }

    #[test]
    fn leaves_hashes_ids_and_paths_alone() {
        for sample in [
            "commit 6ba810789f2a1c3d4e5f60718293a4b5c6d7e8f9",
            "blake3:bd34aa745853dc530f70cfbb7a83276aa72f1189ebdf8c36216cd1f47d0c6d41",
            "c0062-f09-af_nova-en_GB-jenny_dioco-medium",
            "attempt-01m64grbfj4q8gh0nkhg9whcqe",
            "/Users/karl4th/Documents/manifestro/research/dauys/checkpoints/E3",
            "S1-E005-DIAGNOSTICS-codec-and-sampling",
        ] {
            let (out, changed) = text(sample);
            assert!(!changed, "redacted ordinary text: {sample} -> {out}");
        }
    }

    #[test]
    fn leaves_code_that_handles_keys_alone() {
        for sample in [
            "client = AsyncOpenAI(base_url=args.base_url, api_key=args.api_key)",
            "tokenizer.pad_token = tokenizer.eos_token",
            r#"echo "password=$GH_TOKEN""#,
            r#"token = os.environ.get("WANDB_API_KEY")"#,
            "authorization = base64.b64encode(token.encode()).decode()",
            r#"parser.add_argument("--api-key", default="EMPTY")"#,
        ] {
            let (out, changed) = text(sample);
            assert!(!changed, "redacted code: {sample} -> {out}");
        }
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
