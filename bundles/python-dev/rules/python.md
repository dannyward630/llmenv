---
paths:
  - "**/*.py"
  - "**/pyproject.toml"
---

# Python Conventions

## Toolchain

**Runtime:** 3.14 via `uv venv`.

| Purpose | Tool |
|---------|------|
| deps | `uv` |
| lint/format | `ruff check` / `ruff format` |
| types | `ty check` |
| tests | `pytest` |

`AGENTS.md` sets the toolchain rules. Bare `python3` bypasses the venv.

## Types & Errors

- Type hints MUST be on every public function signature; `ty check` MUST pass clean.
- MUST validate at trust boundaries (CLI args, file input, network); MAY assume typed
  data internally.
- MUST raise specific exceptions with context (operation, input, fix). MUST NOT use bare
  `except:` or silent `pass`.
- PEP 758 (3.14+): `except ValueError, TypeError:` without parentheses is valid.
  On projects pinned to 3.14+, `ruff` removes the parentheses — MUST accept that change.

## Testing

- `tests/` SHOULD mirror the package structure.
- SHOULD prefer `pytest` fixtures over setup/teardown methods; SHOULD prefer `parametrize` over copy-pasted
  cases; SHOULD use `hypothesis` where property-based testing fits (parsing, serialization,
  normalization).

## Supply Chain

- MUST pin to patch version in `pyproject.toml` (`~=X.Y.Z`, PEP 440 compatible-release —
  allows patch bumps, blocks minor/major); MUST commit `uv.lock` (hash verification built in).
- MUST audit dependencies before deploy (`uv pip list` + advisory check).
