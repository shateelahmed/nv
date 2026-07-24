//! Configurable random secret generation.//!
//! Produces random values in a few shapes: `hex`, URL-safe `base64`, plain
//! alphanumeric, or characters drawn from a custom set. Randomness comes from
//! the operating system via the `rand` crate.
use anyhow::{Result, bail};
use base64::Engine;
use rand::RngExt;

use crate::config::{SecretFormat, SecretPreset};

/// Default alphanumeric charset used for the `alnum` format.
const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Parameters controlling a single secret generation.
#[derive(Debug, Clone)]
pub struct SecretSpec {
    /// Number of output characters (for `alnum`/custom charset) or number of
    /// random bytes to encode (for `hex`/`base64`).
    pub length: usize,
    pub format: SecretFormat,
    /// Optional custom charset; when set, overrides `format` and produces a
    /// string of `length` characters drawn from it.
    pub charset: Option<String>,
}

impl Default for SecretSpec {
    fn default() -> Self {
        SecretSpec {
            length: 32,
            format: SecretFormat::Base64,
            charset: None,
        }
    }
}

impl From<&SecretPreset> for SecretSpec {
    fn from(p: &SecretPreset) -> Self {
        SecretSpec {
            length: p.length,
            format: p.format,
            charset: p.charset.clone(),
        }
    }
}

/// Generate a single secret according to `spec`.
pub fn generate(spec: &SecretSpec) -> Result<String> {
    if spec.length == 0 {
        bail!("secret length must be greater than zero");
    }
    // `rand::rng()` gives a fast, OS-seeded random number generator.
    let mut rng = rand::rng();

    // A custom charset takes priority: pick `length` random characters from it.
    if let Some(charset) = &spec.charset {
        let chars: Vec<char> = charset.chars().collect();
        if chars.is_empty() {
            bail!("custom charset must not be empty");
        }
        return Ok((0..spec.length)
            .map(|_| chars[rng.random_range(0..chars.len())])
            .collect());
    }

    match spec.format {
        // Alphanumeric: choose from A-Z a-z 0-9.
        SecretFormat::Alnum => Ok((0..spec.length)
            .map(|_| ALNUM[rng.random_range(0..ALNUM.len())] as char)
            .collect()),
        // Hex/base64: make `length` random bytes, then encode them as text.
        SecretFormat::Hex => {
            let bytes = random_bytes(&mut rng, spec.length);
            Ok(hex::encode(bytes))
        }
        SecretFormat::Base64 => {
            let bytes = random_bytes(&mut rng, spec.length);
            Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
        }
    }
}

/// Generate `count` secrets that are guaranteed to be distinct from each other.
///
/// Used by `nv gen --unique`. Retries on the rare chance two come out equal,
/// giving up (with an error) if the space is too small to satisfy the request.
pub fn generate_unique(spec: &SecretSpec, count: usize) -> Result<Vec<String>> {
    let mut out = Vec::with_capacity(count);
    let mut attempts = 0usize;
    let max_attempts = count.saturating_mul(64).max(128);
    while out.len() < count {
        let candidate = generate(spec)?;
        if !out.contains(&candidate) {
            out.push(candidate);
        }
        attempts += 1;
        if attempts > max_attempts {
            bail!("could not generate {count} unique secrets; increase length or charset size");
        }
    }
    Ok(out)
}

fn random_bytes(rng: &mut impl RngExt, n: usize) -> Vec<u8> {
    (0..n).map(|_| rng.random::<u8>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alnum_has_exact_length() {
        let spec = SecretSpec {
            length: 40,
            format: SecretFormat::Alnum,
            charset: None,
        };
        let s = generate(&spec).unwrap();
        assert_eq!(s.chars().count(), 40);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn hex_encodes_length_bytes() {
        let spec = SecretSpec {
            length: 16,
            format: SecretFormat::Hex,
            charset: None,
        };
        let s = generate(&spec).unwrap();
        assert_eq!(s.len(), 32); // 2 hex chars per byte
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn custom_charset_respected() {
        let spec = SecretSpec {
            length: 50,
            format: SecretFormat::Base64,
            charset: Some("AB".to_string()),
        };
        let s = generate(&spec).unwrap();
        assert_eq!(s.len(), 50);
        assert!(s.chars().all(|c| c == 'A' || c == 'B'));
    }

    #[test]
    fn unique_values_are_distinct() {
        let spec = SecretSpec {
            length: 32,
            format: SecretFormat::Base64,
            charset: None,
        };
        let secrets = generate_unique(&spec, 5).unwrap();
        assert_eq!(secrets.len(), 5);
        for i in 0..secrets.len() {
            for j in (i + 1)..secrets.len() {
                assert_ne!(secrets[i], secrets[j]);
            }
        }
    }

    #[test]
    fn zero_length_errors() {
        let spec = SecretSpec {
            length: 0,
            format: SecretFormat::Hex,
            charset: None,
        };
        assert!(generate(&spec).is_err());
    }
}
