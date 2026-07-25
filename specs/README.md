# Spec-Driven Development

This project builds features through four explicit stages. Each stage produces a
written artifact that is reviewed before moving on. This keeps intent, design,
and execution separate and auditable.

```
specify  →  plan  →  tasks  →  implement
  spec.md    plan.md   tasks.md   code + tests
```

## The workflow

1. **Specify** — Capture *what* and *why*, never *how*. Focus on user value,
   behavior, and acceptance criteria. Output: `spec.md`.
2. **Plan** — Decide *how*: architecture, modules, data flow, dependencies, and
   trade-offs. Must trace back to the spec. Output: `plan.md`.
3. **Tasks** — Break the plan into small, ordered, verifiable tasks. Output:
   `tasks.md`.
4. **Implement** — Execute tasks one at a time, keeping tests green and the code
   idiomatic. Update `tasks.md` as work completes.

Each stage has a matching command in both assistants:

| Stage | GitHub Copilot | Claude Code CLI |
| --- | --- | --- |
| Specify | `/specify` | `/specify` |
| Plan | `/plan` | `/plan` |
| Tasks | `/tasks` | `/tasks` |
| Implement | `/implement` | `/implement` |

## Directory layout

Each feature gets a numbered folder:

```
specs/
  templates/
    spec-template.md
    plan-template.md
    tasks-template.md
  001-short-feature-name/
    spec.md
    plan.md
    tasks.md
  002-env-file-glob/
    spec.md
    plan.md
    tasks.md
```

Number features sequentially (`001`, `002`, …) and use a short, kebab-case slug.

## Rules

- **No code before a spec.** Behavior changes start with `spec.md`.
- **Plans cite the spec.** Every design decision maps to a requirement.
- **Tasks are verifiable.** Each task states how it will be checked (test,
  command, or observable behavior).
- **Keep both assistants in sync (spec [001](001-sync-assistant-configs/spec.md)).**
  When a new spec introduces or changes a durable rule, convention, workflow step,
  or guarantee, update **both** the Copilot config
  (`.github/copilot-instructions.md`, `.github/prompts/*`) and the Claude config
  (`CLAUDE.md`, `.claude/commands/*`) in the same change. Syntax may differ;
  behavior must not.
- **Preserve formatting-preserving guarantees.** File edits must never reorder or
  drop unrelated content (see `.github/copilot-instructions.md`).
- **Keep artifacts in sync.** When scope changes, update the spec first, then the
  plan and tasks.
- **Convert instructions to specs before implementing.** When a new instruction
  (e.g., from a CLAUDE.md rule, copilot-instructions entry, or prompt) would
  introduce a durable behavior change or new capability, create a spec for it
  before writing code. Simple clarifications or typo fixes are exempt.
