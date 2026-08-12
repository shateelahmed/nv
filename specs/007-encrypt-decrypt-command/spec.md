# Spec: `nv encrypt` and `nv decrypt` commands

- **ID:** 007-encrypt-decrypt-command
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-26

## Summary

Two new commands `nv encrypt` and `nv decrypt` that encrypt and decrypt the
contents of `.env.example` files using a user-provided key. This allows teams
to store example environment files with encrypted secret values in version
control, while still being able to decrypt them for local development.

## Problem / Motivation

Teams often need to share `.env.example` files that contain placeholder or
example secret values. However, even these example values can sometimes be
sensitive (e.g., connection strings with embedded passwords, API endpoints
with tokens). Storing these in plain text in version control can be a security
risk.

By encrypting the values in `.env.example` files, teams can:
- Safely commit example files to version control
- Share them with team members who have the decryption key
- Prevent accidental exposure of sensitive example values

## Goals

- `nv encrypt` encrypts all values in a selected `.env.example` file using a
  user-provided key.
- `nv decrypt` decrypts all values in a selected `.env.example` file using a
  user-provided key.
- Both commands support selecting the target service via `--service`.
- Both commands support selecting the target file via `--file`.
- Both commands display a preview before applying changes.
- Both commands use `--yes` to skip confirmation.
- Encryption is symmetric (same key encrypts and decrypts).
- Keys (variable names) are preserved; only values are encrypted/decrypted.
- Comments and formatting are preserved.

## Non-goals

- Encrypting/decrypting `.env` files (only `.env.example` files).
- Encrypting/decrypting YAML files (configmap, secrets).
- Key management or rotation.
- Asymmetric encryption.
- Encrypting entire files (we encrypt values in-place).

## User stories

- As a developer, I want to encrypt secret values in `.env.example` so that
  I can safely commit the file to version control.
- As a developer, I want to decrypt a `.env.example` file so that I can
  read the example values for local development.
- As a platform engineer, I want to share encrypted example files with my team
  so that they can decrypt them with a shared key.

## Behavior & requirements

### `nv encrypt`

- `nv encrypt` MUST encrypt all values in a selected `.env.example` file.
- The command MUST require a `--key <KEY>` flag for the encryption key.
- The command MUST support `--service` / `-s` to select the target service.
  If multiple services have `.env.example` files, the command MUST prompt
  the user to select one.
- The command MUST support `--file` to select a specific `.env.example` file
  when a service has multiple.
- The command MUST display a preview of the encrypted file before applying.
- The command MUST use `--yes` / `-y` to skip the confirmation prompt.
- The command MUST use `--dry-run` to show the preview without writing.
- Keys (variable names) MUST NOT be encrypted — only values.
- Comments and blank lines MUST be preserved.
- Empty values MUST remain empty (not encrypted).
- Lines with `export` prefix MUST be handled correctly.

### `nv decrypt`

- `nv decrypt` MUST decrypt all values in a selected `.env.example` file.
- The command MUST require a `--key <KEY>` flag for the decryption key.
- The command MUST support `--service` / `-s` to select the target service.
- The command MUST support `--file` to select a specific `.env.example` file.
- The command MUST display a preview of the decrypted file before applying.
- The command MUST use `--yes` / `-y` to skip the confirmation prompt.
- The command MUST use `--dry-run` to show the preview without writing.
- If a value is not encrypted, the command MUST leave it unchanged.
- If the key is wrong, the command MUST produce garbled output (no error —
  symmetric encryption with wrong key produces invalid plaintext).

### Encryption algorithm

- The encryption MUST use AES-256-GCM (or similar authenticated encryption).
- The key MUST be derived from the user-provided string using a key derivation
  function (e.g., PBKDF2, scrypt, or Argon2).
- A random nonce/IV MUST be generated for each value encrypted.
- The encrypted value MUST be encoded in a format that can be stored in a
  dotenv file (e.g., base64).
- The encrypted value MUST be prefixed with `ENC[` and suffixed with `]` to
  indicate it is encrypted (e.g., `ENC[base64encodeddata]`).

### CLI surface

```
nv encrypt [OPTIONS] --key <KEY>

Options:
  --key <KEY>       Encryption key (required)
```

```
nv decrypt [OPTIONS] --key <KEY>

Options:
  --key <KEY>       Decryption key (required)
```

Both commands use global flags: `--service`, `--file`, `--dry-run`, `--yes`,
`--no-config`, `--root`, `--all`.

## Acceptance criteria

- [ ] Given a service with `.env.example` containing `DB_PASSWORD=secret123`,
      when `nv encrypt --key mykey` is run, then the value is encrypted and
      stored as `DB_PASSWORD=ENC[...]`.
- [ ] Given a service with `.env.example` containing `DB_PASSWORD=ENC[...]`,
      when `nv decrypt --key mykey` is run, then the value is decrypted back
      to `DB_PASSWORD=secret123`.
- [ ] Given a service with `.env.example` containing `# Database config`,
      when `nv encrypt` is run, then the comment is preserved.
- [ ] Given a service with `.env.example` containing `EMPTY_VAR=`,
      when `nv encrypt` is run, then the value remains empty.
- [ ] Given a service with `.env.example` containing `export API_KEY=xxx`,
      when `nv encrypt` is run, then `export` prefix is preserved and only
      the value is encrypted.
- [ ] Given `--dry-run`, when `nv encrypt` is run, then the preview is shown
      but no file is written.
- [ ] Given `--yes`, when `nv encrypt` is run, then the file is written
      without confirmation.
- [ ] Given a wrong key, when `nv decrypt` is run, then garbled output is
      produced (no error).
- [ ] Given an already decrypted value, when `nv decrypt` is run, then the
      value is unchanged.

## Edge cases

- Empty `.env.example` files are handled (no-op).
- Files with only comments are handled (no-op).
- Multiple `.env.example` files in a service: use `--file` to select.
- Very long values: encryption should work without truncation.
- Values containing special characters (newlines, equals signs): handled
  correctly by the encryption/decryption logic.
- `--service` with a service that has no `.env.example`: error message.
- `--file` with a file that doesn't exist: error message.

## Open questions

- Should we support a `--list` flag to show which `.env.example` files are
  encrypted vs decrypted?
- Should we support batch encryption/decryption of all `.env.example` files?
- Should the encrypted format include a version number for future algorithm
  changes?
- Should we support a `--force` flag to encrypt/decrypt already
  encrypted/decrypted files?

## Post-approval revisions

The shipped implementation differs from the original proposal in two ways,
and later gained one formatting behavior:

1. **Whole-file encryption.** The commands encrypt/decrypt the entire file as
   a single `ENC[base64(nonce+ciphertext)]` blob, not per-value in place.
2. **Trailing-newline tolerance.** A `.env.example` may end with a newline
   after the `ENC[...]` token (e.g. added by an editor); `nv decrypt` strips
   surrounding whitespace to find the token, so the file decrypts regardless.
3. **Unix file format.** `nv encrypt` always writes a trailing newline after
   the `ENC[...]` token. `nv decrypt` preserves the file's trailing whitespace
   but never duplicates the terminating newline, so encrypt→decrypt is a byte
   -for-byte round-trip for content that ends with a newline (and yields a
   single trailing newline otherwise).

## Assistant-config sync

No assistant-config change is required. This revision is a file-format detail
of one command and introduces no cross-cutting rule, convention, workflow
step, or project guarantee.
