//! `nv encrypt` / `nv decrypt` — encrypt or decrypt an entire file using
//! AES-256-GCM with a user-provided key.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

use super::Cli;
use crate::color;
use crate::edit::ChangeSet;

/// The prefix that indicates encrypted content.
const ENCRYPTED_PREFIX: &str = "ENC[";
/// The suffix that indicates encrypted content.
const ENCRYPTED_SUFFIX: &str = "]";
/// PBKDF2 iteration count for key derivation.
const PBKDF2_ITERATIONS: u32 = 100_000;
/// Salt for key derivation.
const SALT: &[u8] = b"nv-encrypt-salt-v1";

/// Derive an AES-256 key from a password string using PBKDF2.
fn derive_key(password: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), SALT, PBKDF2_ITERATIONS, &mut key);
    key
}

/// Encrypt plaintext, returning `ENC[base64(nonce+ciphertext)]` followed by a
/// trailing newline so the written file follows the Unix text-file format.
fn encrypt_content(plaintext: &str, password: &str) -> Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }

    let key = derive_key(password);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("failed to create cipher: {e}"))?;

    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);

    Ok(format!(
        "{}{}{}\n",
        ENCRYPTED_PREFIX,
        BASE64.encode(&combined),
        ENCRYPTED_SUFFIX
    ))
}

/// Decrypt `ENC[...]` content back to plaintext.
fn decrypt_content(value: &str, password: &str) -> Result<String> {
    if value.is_empty() {
        return Ok(String::new());
    }

    // The file may end with a trailing newline; strip surrounding whitespace
    // so the `ENC[...]` token is found, then re-append it to the plaintext so
    // the file keeps its original formatting.
    let trimmed = value.trim_end_matches(char::is_whitespace);
    let trailing_ws = &value[trimmed.len()..];

    let Some(encoded) = trimmed
        .strip_prefix(ENCRYPTED_PREFIX)
        .and_then(|s| s.strip_suffix(ENCRYPTED_SUFFIX))
    else {
        return Ok(value.to_string());
    };

    let combined = BASE64
        .decode(encoded)
        .map_err(|e| anyhow::anyhow!("invalid base64 in encrypted content: {e}"))?;

    if combined.len() < 12 {
        bail!("encrypted content too short (missing nonce)");
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);

    let key = derive_key(password);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("failed to create cipher: {e}"))?;

    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed (wrong key?): {e}"))?;

    let plaintext = String::from_utf8(plaintext)
        .map_err(|e| anyhow::anyhow!("decrypted content is not valid UTF-8: {e}"))?;

    // Re-append the file's trailing whitespace unless the decrypted content
    // already ends with a newline. Encrypt always writes a trailing newline,
    // and re-adding it here would double it on round-trip.
    if trailing_ws.is_empty() || plaintext.ends_with('\n') {
        Ok(plaintext)
    } else {
        Ok(plaintext + trailing_ws)
    }
}

/// Shared runner for encrypt/decrypt — resolves the target file, reads it,
/// calls `transform_fn`, and shows a preview.
fn run_transform(
    cli: &Cli,
    password: &str,
    service_name: &str,
    file_path: &str,
    transform_fn: fn(&str, &str) -> Result<String>,
) -> Result<()> {
    let ctx = super::context::resolve(cli)?;
    super::context::print_banner(&ctx);

    let target_file = crate::model::find_target(&ctx.services, service_name, file_path)?;
    let old_content = std::fs::read_to_string(&target_file.path).unwrap_or_default();

    let new_content = transform_fn(&old_content, password)?;

    let changes = ChangeSet {
        changes: vec![crate::edit::FileChange {
            service: service_name.to_string(),
            display: target_file.display.clone(),
            path: target_file.path.clone(),
            kind: target_file.kind,
            key: String::new(),
            value: String::new(),
            old_content,
            new_content,
        }],
    };

    let use_color = color::should_use_color();
    let colors = ctx.colors();
    super::context::preview_and_apply(cli, &changes, &colors, use_color)
}

/// Handle `nv encrypt`: encrypt an entire file.
pub fn run_encrypt(cli: &Cli, password: &str, service_name: &str, file_path: &str) -> Result<()> {
    run_transform(cli, password, service_name, file_path, encrypt_content)
}

/// Handle `nv decrypt`: decrypt an entire file.
pub fn run_decrypt(cli: &Cli, password: &str, service_name: &str, file_path: &str) -> Result<()> {
    run_transform(cli, password, service_name, file_path, decrypt_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let password = "test-password-123";
        let original = "hello world\n";
        let encrypted = encrypt_content(original, password).unwrap();
        assert!(encrypted.starts_with(ENCRYPTED_PREFIX));
        assert!(encrypted.trim_end().ends_with(ENCRYPTED_SUFFIX));
        assert!(encrypted.ends_with('\n'));
        assert_ne!(encrypted, original);

        let decrypted = decrypt_content(&encrypted, password).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn encrypt_always_appends_trailing_newline() {
        let encrypted = encrypt_content("hello world", "key").unwrap();
        assert!(encrypted.trim_end().ends_with(ENCRYPTED_SUFFIX));
        assert!(encrypted.ends_with('\n'));
    }

    #[test]
    fn decrypt_roundtrip_content_without_newline_gains_one() {
        // Encrypt always writes a trailing newline, so decrypting content that
        // originally had none yields it with a single trailing newline (Unix
        // text-file format).
        let encrypted = encrypt_content("hello world", "key").unwrap();
        let decrypted = decrypt_content(&encrypted, "key").unwrap();
        assert_eq!(decrypted, "hello world\n");
    }

    #[test]
    fn encrypt_empty_content() {
        let result = encrypt_content("", "key").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn decrypt_empty_content() {
        let result = decrypt_content("", "key").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn decrypt_non_encrypted_passthrough() {
        let result = decrypt_content("plain-text", "key").unwrap();
        assert_eq!(result, "plain-text");
    }

    #[test]
    fn wrong_key_fails() {
        let encrypted = encrypt_content("secret", "correct-key").unwrap();
        let result = decrypt_content(&encrypted, "wrong-key");
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_preserves_multiline_content() {
        let content = "# comment\nFOO=bar\nBAZ=qux\n";
        let encrypted = encrypt_content(content, "key").unwrap();
        let decrypted = decrypt_content(&encrypted, "key").unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn encrypt_newlines_in_content() {
        let content = "line1\nline2\nline3\n";
        let encrypted = encrypt_content(content, "key").unwrap();
        let decrypted = decrypt_content(&encrypted, "key").unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn encrypt_special_characters() {
        let content = "KEY=\"value with spaces\"\nOTHER=foo#bar\n";
        let encrypted = encrypt_content(content, "key").unwrap();
        let decrypted = decrypt_content(&encrypted, "key").unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn decrypt_tolerates_extra_trailing_blank_line() {
        // An editor may append a blank line after the ENC token; the token is
        // still found and the original content keeps its own terminating
        // newline without it being duplicated.
        let encrypted = encrypt_content("hello world\n", "key").unwrap();
        let file = format!("{encrypted}\n");
        let decrypted = decrypt_content(&file, "key").unwrap();
        assert_eq!(decrypted, "hello world\n");
    }

    #[test]
    fn decrypt_preserves_file_newlines_when_content_has_none() {
        // When the original content had no trailing newline, the file's
        // trailing newlines are formatting and are preserved as-is.
        let encrypted = encrypt_content("hello world", "key").unwrap();
        let file = format!("{encrypted}\n");
        let decrypted = decrypt_content(&file, "key").unwrap();
        assert_eq!(decrypted, "hello world\n\n");
    }

    #[test]
    fn decrypt_tolerates_crlf_ending() {
        let encrypted = encrypt_content("hello world", "key").unwrap();
        let crlf_file = format!("{}\r\n", encrypted.trim_end_matches('\n'));
        let decrypted = decrypt_content(&crlf_file, "key").unwrap();
        assert_eq!(decrypted, "hello world\r\n");
    }

    #[test]
    fn decrypt_non_encrypted_passthrough_keeps_trailing_newline() {
        let decrypted = decrypt_content("plain-text\n", "key").unwrap();
        assert_eq!(decrypted, "plain-text\n");
    }
}
