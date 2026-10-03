//! `nv encode` / `nv decode` — hide sensitive key names in `.env.example`
//! files behind a reversible, recognizable `ENC.` prefix.
//!
//! `encode` rewrites a sensitive key name to `ENC.<base64>`; `decode` restores
//! it. The two are exact inverses. Values are never touched, and only
//! `.env.example` files are in scope — renaming a key in a deployed configmap
//! would break the running deployment.

use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::{Cli, context, leaks};
use crate::color;
use crate::edit::{self, ChangeSet, FileChange};
use crate::model::FileKind;
use crate::parser::dotenv;

/// Marker that identifies an encoded key so it is easy to spot in a file and
/// easy to skip on a second `encode` run.
///
/// The separator is `.` rather than `[`/`]` (used by `nv encrypt`) because `.`
/// is a legal dotenv key character after the first one, whereas `=` padding
/// would terminate the key.
const ENC_PREFIX: &str = "ENC.";

/// Encode a key name to its `ENC.`-prefixed form.
///
/// URL-safe unpadded base64 is mandatory: padded output emits `=`, which dotenv
/// treats as the assignment separator, corrupting the line into
/// `ENC.abc==` + value. URL-safe also avoids `+` and `/`, neither of which is a
/// legal dotenv key character.
fn encode_key(key: &str) -> String {
    format!("{ENC_PREFIX}{}", URL_SAFE_NO_PAD.encode(key))
}

/// Split an encoded key back into its payload, or `None` if it lacks the prefix.
fn encoded_payload(key: &str) -> Option<&str> {
    key.strip_prefix(ENC_PREFIX)
}

/// Validate the `--file` filter and collect the `.env.example` targets.
///
/// The target kind is fixed, so a `--file` naming any other kind is an error
/// rather than something to silently ignore.
fn collect_example_targets(cli: &Cli, ctx: &context::Context) -> Result<Vec<edit::Target>> {
    // An empty filter (or `--all`) means "no restriction", so only validate
    // when the user actually named kinds.
    let kinds = ctx.kind_filter(cli)?;
    if !kinds.is_empty() && !kinds.iter().all(|k| *k == FileKind::DotenvExample) {
        bail!("encode/decode only operate on .env.example files (--file dotenv_example)");
    }

    let service_filter = ctx.service_filter(cli);
    Ok(edit::collect_targets(
        &ctx.services,
        &service_filter,
        &[FileKind::DotenvExample],
    ))
}

/// A decoded key name that could not be used, with the reason why.
struct DecodeProblem {
    service: String,
    display: String,
    key: String,
    reason: String,
}

/// Handle `nv encode`: rewrite sensitive key names in `.env.example` files.
pub fn run_encode(cli: &Cli) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    let targets = collect_example_targets(cli, &ctx)?;
    if targets.is_empty() {
        bail!("no .env.example files found");
    }

    let mut changes = ChangeSet::default();
    let mut encoded_total = 0usize;

    for target in &targets {
        let old_content = edit::read_or_empty(&target.file.path)?;
        let mut new_content = old_content.clone();

        // Merge the global and per-service special key lists, mirroring what
        // `nv leaks` treats as sensitive.
        let special_keys = ctx
            .config
            .as_ref()
            .map(|cfg| cfg.special_secret_keys_for(&target.service))
            .unwrap_or_default();

        for pair in crate::parser::parse(&old_content, target.file.kind) {
            let key = pair.key.as_str();

            // Already encoded: skip so a second run is a no-op.
            if encoded_payload(key).is_some() {
                continue;
            }
            // The user declared this a false alarm, so it is not a secret.
            if ctx
                .config
                .as_ref()
                .is_some_and(|cfg| cfg.is_false_alarm(&target.service, key))
            {
                continue;
            }
            if !leaks::is_sensitive_key_name(key, &special_keys) {
                continue;
            }

            new_content = dotenv::rename_key(&new_content, key, &encode_key(key));
            encoded_total += 1;
        }

        if new_content != old_content {
            changes.changes.push(FileChange {
                service: target.service.clone(),
                display: target.file.display.clone(),
                path: target.file.path.clone(),
                kind: target.file.kind,
                key: String::new(),
                value: String::new(),
                old_content,
                new_content,
            });
        }
    }

    if encoded_total == 0 {
        eprintln!("No sensitive keys to encode.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    eprintln!("{encoded_total} key(s) to encode.");
    context::preview_and_apply(cli, &changes, &colors, use_color)
}

/// Handle `nv decode`: restore encoded key names in `.env.example` files.
///
/// Every file is decoded and validated before anything is written, so a
/// malformed key anywhere leaves the whole run without side effects.
pub fn run_decode(cli: &Cli) -> Result<()> {
    let ctx = context::resolve(cli)?;
    context::print_banner(&ctx);

    let targets = collect_example_targets(cli, &ctx)?;
    if targets.is_empty() {
        bail!("no .env.example files found");
    }

    let mut changes = ChangeSet::default();
    let mut decoded_total = 0usize;
    let mut problems: Vec<DecodeProblem> = Vec::new();

    for target in &targets {
        let old_content = edit::read_or_empty(&target.file.path)?;
        let mut new_content = old_content.clone();

        // Keys present in this file, used to detect collisions. Rebuilt as we
        // go so two encoded keys decoding to the same name also collide.
        let mut present: Vec<String> = crate::parser::parse(&old_content, target.file.kind)
            .into_iter()
            .map(|p| p.key)
            .collect();

        for pair in crate::parser::parse(&old_content, target.file.kind) {
            let key = pair.key.as_str();
            let Some(payload) = encoded_payload(key) else {
                continue;
            };

            let problem = |reason: String| DecodeProblem {
                service: target.service.clone(),
                display: target.file.display.clone(),
                key: key.to_string(),
                reason,
            };

            let bytes = match URL_SAFE_NO_PAD.decode(payload) {
                Ok(b) => b,
                Err(e) => {
                    problems.push(problem(format!("invalid base64 payload ({e})")));
                    continue;
                }
            };

            let decoded = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    problems.push(problem("payload is not valid UTF-8".to_string()));
                    continue;
                }
            };

            // A decoded name that cannot be a key would produce a broken file.
            if !is_valid_dotenv_key(&decoded) {
                problems.push(problem(format!("'{decoded}' is not a valid key name")));
                continue;
            }

            if present.iter().any(|k| k == &decoded) {
                problems.push(problem(format!("'{decoded}' already exists in this file")));
                continue;
            }

            new_content = dotenv::rename_key(&new_content, key, &decoded);
            present.push(decoded);
            decoded_total += 1;
        }

        if new_content != old_content {
            changes.changes.push(FileChange {
                service: target.service.clone(),
                display: target.file.display.clone(),
                path: target.file.path.clone(),
                kind: target.file.kind,
                key: String::new(),
                value: String::new(),
                old_content,
                new_content,
            });
        }
    }

    // Refuse to write anything if any key could not be decoded, so a bad
    // payload can never leave a half-converted tree behind.
    if !problems.is_empty() {
        eprintln!("Cannot decode {} key(s):", problems.len());
        for p in &problems {
            eprintln!("  {}/{}: {} — {}", p.service, p.display, p.key, p.reason);
        }
        eprintln!("No files were changed. Fix the keys above and retry.");
        bail!("decode failed");
    }

    if decoded_total == 0 {
        eprintln!("No encoded keys to decode.");
        return Ok(());
    }

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    eprintln!("{decoded_total} key(s) to decode.");
    context::preview_and_apply(cli, &changes, &colors, use_color)
}

/// Whether `name` is a legal dotenv key: a leading letter or `_`, then
/// letters, digits, `_`, or `.`.
///
/// Mirrors the parser's own rule so decode never writes a line the parser
/// cannot read back.
fn is_valid_dotenv_key(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_with_the_enc_prefix() {
        assert_eq!(encode_key("JWT_SECRET"), "ENC.SldUX1NFQ1JFVA");
        assert_eq!(encode_key("DB_PASSWORD"), "ENC.REJfUEFTU1dPUkQ");
        assert_eq!(encode_key("API_KEY"), "ENC.QVBJX0tFWQ");
    }

    #[test]
    fn encoding_never_emits_padding_or_url_unsafe_chars() {
        // `=` would terminate the key; `+` and `/` are illegal in dotenv keys.
        for key in [
            "JWT_SECRET",
            "DB_PASSWORD",
            "API_KEY",
            "ZOOM_ACCOUNT_ID",
            "A_KEY",
            "MY_USERNAME",
        ] {
            let encoded = encode_key(key);
            let payload = encoded_payload(&encoded).unwrap();
            assert!(
                payload
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{encoded} contains an unsafe character"
            );
        }
    }

    #[test]
    fn encoded_key_is_a_valid_dotenv_key() {
        assert!(is_valid_dotenv_key(&encode_key("JWT_SECRET")));
        assert!(is_valid_dotenv_key(&encode_key("ZOOM_ACCOUNT_ID")));
    }

    #[test]
    fn round_trips_through_decode() {
        for key in [
            "JWT_SECRET",
            "DB_PASSWORD",
            "API_KEY",
            "ZOOM_ACCOUNT_ID",
            "MY_USERNAME",
        ] {
            let encoded = encode_key(key);
            let payload = encoded_payload(&encoded).unwrap();
            let decoded = String::from_utf8(URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
            assert_eq!(decoded, key);
        }
    }

    #[test]
    fn encoded_payload_requires_the_prefix() {
        assert_eq!(encoded_payload("ENC.abc"), Some("abc"));
        assert_eq!(encoded_payload("JWT_SECRET"), None);
        // A near-miss prefix must not be treated as encoded.
        assert_eq!(encoded_payload("ENCabc"), None);
        assert_eq!(encoded_payload("enc.abc"), None);
    }

    #[test]
    fn rename_then_decode_restores_the_original_line() {
        let original = "# doc\nJWT_SECRET=changeme # inline\nAPP_NAME=demo\n";
        let encoded = dotenv::rename_key(original, "JWT_SECRET", &encode_key("JWT_SECRET"));
        assert_eq!(
            encoded,
            "# doc\nENC.SldUX1NFQ1JFVA=changeme # inline\nAPP_NAME=demo\n"
        );
        let restored = dotenv::rename_key(&encoded, "ENC.SldUX1NFQ1JFVA", "JWT_SECRET");
        assert_eq!(restored, original);
    }

    #[test]
    fn rejects_invalid_decoded_names() {
        assert!(is_valid_dotenv_key("JWT_SECRET"));
        assert!(is_valid_dotenv_key("_LEADING"));
        // Must start with a letter or underscore.
        assert!(!is_valid_dotenv_key("1JWT"));
        assert!(!is_valid_dotenv_key(""));
        // No `=`, spaces, or padding characters may appear in a key.
        assert!(!is_valid_dotenv_key("JWT=SECRET"));
        assert!(!is_valid_dotenv_key("JWT SECRET"));
    }

    #[test]
    fn invalid_base64_is_reported_not_guessed() {
        // `ENC.` payload that is not decodable must surface as an error rather
        // than silently producing a wrong key.
        let result = URL_SAFE_NO_PAD.decode("not valid base64!!");
        assert!(result.is_err());
    }

    #[test]
    fn sensitivity_predicate_drives_what_gets_encoded() {
        assert!(leaks::is_sensitive_key_name("JWT_SECRET", &[]));
        assert!(!leaks::is_sensitive_key_name("APP_NAME", &[]));
        // An already-encoded key must not be re-encoded.
        assert!(!leaks::is_sensitive_key_name(
            &encode_key("JWT_SECRET"),
            &[]
        ));
    }
}
