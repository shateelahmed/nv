# Plan: `special_secret_keys` for `nv leaks` and `nv fake-secrets`

- **ID:** 011-special-keys
- **Status:** Implemented
- **Author:** shateel
- **Date:** 2026-07-29

## Overview

Extend both the leak-detection and fake-secret-detection pipelines so that, in
addition to the built-in regex, a configurable set of exact key names is also
considered. Keys in `special_secret_keys` are:
- **Detected as leaks** by `nv leaks` (adds to the match set)
- **Excluded from fake secrets** by `nv fake-secrets` (extends the recognized
  secret-key set)

The design mirrors the existing `false_alarms` mechanism: global + per-service
`Vec<String>` on `LeakConfig`, merged at runtime.

## Design decisions

### 1. Config schema — mirror `false_alarms`

Add `special_secret_keys: Vec<String>` to `LeakConfig` next to `false_alarms`.
Using `LeakConfig` for both commands keeps config access consistent — both
already share `is_false_alarm()`.

### 2. Merge strategy — union of global + per-service

`Config::special_secret_keys_for(service)` returns the union of global
`commands.leaks.special_secret_keys` and
`services.<svc>.commands.leaks.special_secret_keys`.

### 3. `nv leaks` detection — dynamic regex

Build a single regex from the escaped special secret keys and run
`captures_iter`. Merge with the built-in regex pass in a `BTreeMap` for dedup.

### 4. `nv fake-secrets` detection — set membership check

Retrieve the merged `special_secret_keys` list per service. In the detection
loop, treat a key as a "secret key" if it matches `secret_key_pattern` OR is
in the `special_secret_keys` list. This naturally excludes it from fake-secret
reporting since the existing logic only reports non-secret keys.

## Data flow

```
nv.yml
  ↓
Config::special_secret_keys_for(service) → Vec<&str>
  ↓
leaks.rs::run()
  ├── regex pass → Vec<(key, value)>
  └── special_secret_keys pass → Vec<(key, value)>
       ↓
      merge + dedup + filter false_alarms
       ↓
      Leak struct / output

fake_secrets.rs::run()
  ├── regex pass → Vec<ParsedPair>
  └── per-pair: is_secret = secret_re.is_match(key) || special_set.contains(key)
       ↓
      skip if is_secret
```

## Modules affected

| Module | Change |
| --- | --- |
| `src/config.rs` | Add `special_secret_keys` to `LeakConfig`. Add `special_secret_keys_for()` method. |
| `src/cli/leaks.rs` | Add special-secret-keys scan pass. Merge and dedup. |
| `src/cli/fake_secrets.rs` | Skip keys in `special_secret_keys` from fake-secret detection. |
| `nv.yml.example` | Document `special_secret_keys`. |

## Assistant configs

No durable rule change — no assistant-config update required.
