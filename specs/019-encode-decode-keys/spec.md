# Spec: `nv encode` and `nv decode` commands

- **ID:** 019-encode-decode-keys
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-10-04

## Summary

`nv encode` rewrites sensitive environment-variable **key names** in `.env.example`
files into a base64 form prefixed with `ENC.`, so that scanners such as
`nv leaks` no longer match them. `nv decode` reverses the transformation,
restoring the original key names. The two commands are exact inverses.

## Problem / Motivation

- `.env.example` files are committed to version control, but they must still
  document the *shape* of a service's configuration. Naming a key
  `JWT_SECRET=` or `DB_PASSWORD=` in such a file is a false alarm for secret
  scanners: `nv leaks` matches on **key name**, so the example file is reported
  even though no real secret is present.
- Today the only remedies are destructive or manual: `nv leaks --clean` empties
  the value (leaving a still-sensitive name behind) or deletes the line entirely
  (losing the documentation), and hand-editing every example file does not
  scale across a monorepo.
- There is no way to tell a genuine hardcoded secret apart from a documented
  example key by looking at the file alone.

## Goals

- Provide a reversible, auditable way to hide sensitive **key names** in
  `.env.example` files from name-based scanners.
- Make the transformation obvious on sight: every encoded key is recognizable by
  its `ENC.` prefix.
- Guarantee that `decode(encode(x)) == x` for the key name.
- Reuse the exact criteria `nv leaks` uses, so `encode` produces exactly the
  set of keys that would otherwise be reported.

## Non-goals

- **Not** encoding values. `nv encrypt` / `nv decrypt` (spec 007) already cover
  value encryption with the `ENC[…]` wrapper. `encode` / `decode` are a distinct,
  complementary pair that touch key names only.
- **Not** touching configmap or secrets files. Renaming a deployed key would
  break the running deployment contract; `nv leaks --clean` remains the remedy
  there.
- **Not** touching `.env` files (not scanned by `nv leaks`).
- **Not** editing text inside comments. Comments stay byte-identical; see
  *Known limitations*.
- **Not** a security control. The encoding is obfuscation, not encryption —
  anyone can run `nv decode`.
- TUI changes.

## User stories

- As a developer, I want `nv encode` to hide sensitive key names in my committed
  `.env.example` files so that CI secret scanning stops failing on template
  files that contain no real secrets.
- As a reviewer, I want encoded keys to be instantly recognizable via the
  `ENC.` prefix so I can tell at a glance that a name was deliberately
  obfuscated rather than accidentally mangled.
- As a developer, I want `nv decode` to restore the original names so I can
  update a real `.env` or configmap with the correct key.

## Behavior & requirements

### Sensitive key criteria

A key name is **sensitive** when it satisfies **any** of the following, and
**none** of the exclusions:

- It matches the `nv leaks` key-name pattern (case-sensitive):
  `[A-Za-z0-9_][A-Z0-9_]+_(?:KEY|PASSWORD|SECRET|TOKEN|ID|USERNAME)`.
- It is listed in the global `commands.leaks.special_secret_keys` in `nv.yml`.
- It is listed in the service's `commands.leaks.special_secret_keys`.

Exclusions (a matching key is **not** treated as sensitive):

- It already begins with `ENC.` (makes `encode` idempotent).
- It is listed in `commands.leaks.false_alarms` (global or per-service). A
  false alarm is a key the user has explicitly declared *not* a secret, so it
  MUST NOT be obfuscated.

The key's **value is irrelevant**: a key is encoded whether its value is empty,
a placeholder, or populated. Leaks ignores empty-valued keys, but the *name* is
what this feature hides, so the criterion is name-only and stable over time.

### Encoding format

An encoded key MUST have the form:

```
ENC.<base64-url-safe-unpadded-of-the-original-key-name>
```

- The encoding MUST be **URL-safe and unpadded** base64 (alphabet
  `A–Z a–z 0–9 - _`, no `=` padding).
- **Rationale (verified empirically against this codebase):** dotenv keys are
  split at the first `=`, so padded base64 corrupts the line —
  `ENC.SldUX1NFQ1JFVA==x` parses as key `ENC.SldUX1NFQ1JFVA` with value `=x`.
  URL-safe unpadded output never emits `=`, `+`, or `/`.
- The `ENC.` prefix is valid under the dotenv key grammar (first character
  alphabetic; remaining characters alphanumeric, `_`, or `.`).
- Values MUST be left untouched.

Worked examples:

| Original key | Encoded key |
| --- | --- |
| `JWT_SECRET` | `ENC.SldUX1NFQ1JFVA` |
| `DB_PASSWORD` | `ENC.REJfUEFTU1dPUkQ` |
| `API_KEY` | `ENC.QVBJX0tFWQ` |
| `APP_NAME` | *not encoded* (not sensitive) |

### Target files

- Both commands MUST operate only on files whose kind is `DotenvExample`
  (`.env.example`, `.env.testing.example`, …).
- Configmap, secrets, and `.env` files MUST be left byte-identical.
- Service selection uses the global `-s/--service` (repeatable) and `--all`
  flags, consistent with `nv leaks`. With no `-s`, every discovered service is
  processed.

### CLI surface

```
nv encode [OPTIONS]
nv decode [OPTIONS]

Options:
  -s, --service <NAME>   Service(s) to process (repeatable). Omit for all.
  --all                  Process every service, ignoring filters.
```

Both commands inherit the global flags (`--no-config`, `--root`, `--file`,
`--dry-run`, `-y/--yes`) and MUST honor `--dry-run` and `-y/--yes` through the
same preview/apply path used by `nv leaks --clean`.

`--file` is not meaningful here because the target kind is fixed to
`.env.example`; specifying another kind MUST be rejected with a clear error
rather than silently ignored.

### Encode behavior

1. For every selected `.env.example` file, parse key/value pairs.
2. Select keys that are sensitive per the criteria above.
3. Replace the key name on its own line with the encoded form.
4. Leave the value, the line's position, indentation, `export ` prefix, inline
   comments, surrounding comment lines, blank lines, ordering, and line endings
   byte-identical (golden rule 1).
5. Report the change through the standard preview/apply flow.

`encode` MUST be idempotent: running it twice MUST produce the same file as
running it once.

### Decode behavior

1. For every selected `.env.example` file, find keys beginning with `ENC.`.
2. Strip the prefix and base64-url-safe-unpadded-decode the remainder.
3. Replace the key name with the decoded original.
4. Preserve everything else byte-identically, exactly as `encode` does.
5. Report the change through the standard preview/apply flow.

A decoded key name MUST be a valid key for the file kind; if it is not, the
command MUST refuse to write (see *Error handling*).

`decode` MUST be idempotent: keys not beginning with `ENC.` are never touched,
so a second run is a no-op.

### Error handling

- **Malformed encoded key** — a key beginning with `ENC.` whose remainder is
  not valid URL-safe unpadded base64, or which decodes to a name that is not a
  valid dotenv key. The command MUST report **every** such occurrence (service,
  file, key, reason), write **nothing** to disk, and exit non-zero. It MUST NOT
  partially rewrite a file.
- **Decode collision** — if the decoded key name already exists in the same
  file, the command MUST report it and write nothing, rather than silently
  producing a duplicate key.
- **Encode collision** — if a file already contains the target encoded key,
  that entry is left as-is (it is already encoded) and is not an error.

### Output

- The standard `Config source:` banner is printed to stderr (golden rule 5).
- Changes are shown in the uniform hierarchical colorized tree format with
  `+`/`-` indicators and `added`/`removed` colors (golden rule 6).
- Each reported row shows the original and resulting key so the transformation
  is reviewable, e.g. `JWT_SECRET` → `ENC.SldUX1NFQ1JFVA`.
- A summary is printed to stderr: `N key(s) encoded.` / `N key(s) decoded.`
- When no keys are eligible, the command prints `No sensitive keys to encode.`
  (respectively `No encoded keys to decode.`) and exits successfully without
  modifying any file.

## Acceptance criteria

- [ ] Given a `.env.example` containing `JWT_SECRET=changeme`, when `nv encode`
      runs, then the line becomes `ENC.SldUX1NFQ1JFVA=changeme`.
- [ ] Given the encoded file above, when `nv decode` runs, then the line is
      restored to exactly `JWT_SECRET=changeme`.
- [ ] Given a `.env.example` with `JWT_SECRET=changeme`, when `nv encode -s auth`
      runs and `nv leaks -s auth` is then run, then `JWT_SECRET` is no longer
      reported as a potential leak.
- [ ] Given a `.env.example` containing a non-sensitive key such as `APP_NAME`,
      when `nv encode` runs, then the file is unchanged.
- [ ] Given a `.env.example` containing `JWT_SECRET=` with an empty value, when
      `nv encode` runs, then the key is encoded (name-only criterion).
- [ ] Given a `.env.example` containing a key listed in `false_alarms`, when
      `nv encode` runs, then that key is left unchanged.
- [ ] Given a `.env.example` containing a key listed in
      `special_secret_keys` that does not match the built-in pattern, when
      `nv encode` runs, then that key is encoded.
- [ ] Given an already-encoded `.env.example`, when `nv encode` runs again, then
      the file is byte-identical (idempotence).
- [ ] Given a `.env.example` with comments, blank lines, `export ` prefixes, and
      CRLF line endings surrounding a sensitive key, when `nv encode` runs, then
      every unrelated byte is preserved.
- [ ] Given a configmap file containing a sensitive key, when `nv encode --all`
      runs, then the configmap file is byte-identical.
- [ ] Given a key beginning with `ENC.` whose payload is not valid base64, when
      `nv decode` runs, then the command reports it, writes nothing, and exits
      non-zero.
- [ ] Given a file containing both `JWT_SECRET=` and `ENC.SldUX1NFQ1JFVA=`, when
      `nv decode` runs, then the collision is reported and nothing is written.
- [ ] Given `--dry-run`, when `nv encode` runs, then the preview is shown and no
      file is modified.
- [ ] Given `-y`, when `nv encode` runs, then the confirmation prompt is
      skipped and the change is applied.

## Edge cases

- **A `.env.example` with no sensitive keys** → `No sensitive keys to encode.`,
  no writes, exit 0.
- **A key whose base64 payload contains `-` or `_`** → valid; URL-safe base64 is
  explicitly allowed to emit these.
- **A service with no `.env.example` files** → skipped silently.
- **Duplicate sensitive keys within one file** (e.g. `JWT_SECRET` twice) → both
  lines are encoded; the summary counts both.
- **A key already starting with `ENC.` that is genuinely a real key** (not
  base64) → `encode` leaves it; `decode` reports it as malformed and writes
  nothing. This is the one genuinely lossy case and is called out deliberately.
- **Nested example files** such as `docker/app/.env.example` → included; paths
  are displayed relative to the service root.
- **Windows/CRLF files** → line endings preserved.

## Known limitations

- **Comments are not rewritten.** A line such as `# JWT_SECRET - signing key`
  keeps naming the key, so the name is still visible in the file. This is
  deliberate: golden rule 1 requires comments to stay byte-identical. Users who
  need the name fully hidden must also edit the comment prose by hand.
- **Obfuscation, not secrecy.** `nv decode` restores any name; the encoding
  defeats naive scanners, not a determined reader.
- The `ENC.` prefix reserves that namespace inside `.env.example` files.

## Open questions

- (none)

## Assistant-config sync

This spec **does** introduce a durable convention and therefore requires an
assistant-config change during implementation, applied to **both** assistant
configs in the same change set (spec 001):

- `CLAUDE.md` and `.github/copilot-instructions.md` — add a golden rule
  describing the `ENC.` convention: sensitive **key names** in `.env.example`
  files may be stored URL-safe unpadded base64 prefixed with `ENC.`, written
  only via `nv encode` and reversed via `nv decode`.
- `.claude/commands/implement.md` and `.github/prompts/implement.prompt.md` —
  qualify the existing "secrets are raw strings (no base64)" constraint so it
  reads as applying to Kubernetes `data:` values, not to this key-name
  convention. Without this, future work is likely to misread the constraint and
  refuse to implement the feature.