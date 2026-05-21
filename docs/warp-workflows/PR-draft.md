# PR draft: Add Wireforge ISO 8583 workflows

Target: `warpdotdev/workflows`, `specs/` directory (one YAML per
workflow per repo convention).

## PR title

```
Add Wireforge ISO 8583 workflows (parse, parse-json, build, validate)
```

## PR body

```markdown
## What

Adds four workflows for working with ISO 8583 financial messages from
the Warp terminal:

- `parse-iso8583` — hex → human-readable field tree
- `parse-iso8583-json` — hex → JSON description (pipe into `jq`)
- `build-iso8583` — JSON → hex wire string
- `validate-iso8583` — parse / re-build / diff round-trip check

## Why

ISO 8583 is the dominant card-payment protocol; debugging a message at
the byte level today means hex dumps in a text editor. These
workflows let a Warp user paste a hex message and immediately see the
decoded field tree, or build a message from a JSON spec for testing
acquirer / issuer / processor integrations.

## Dependency

All four workflows shell out to `wf`, the
[Wireforge CLI](https://github.com/wireforge/wireforge-core).
Installation is one command: `cargo install wf-cli`.

`wf-cli` is Apache-2.0 licensed. The codec is pure Rust with no
network or filesystem side effects; it parses bytes and emits bytes.

## Testing

Each workflow was exercised on a known-good ISO 8583-1987 ASCII
message:

```
Input  hex: 303230302000000000000000303030303030
Parsed:    MTI 0200, field 3 = "000000" (Processing Code)
```

Round-trip (`validate-iso8583`) returns exit 0 on canonical input and
non-zero on tampered input.
```

## Submission steps

1. Fork `warpdotdev/workflows`.
2. Copy the four `.yaml` files from this directory into the
   appropriate `specs/<category>/` subdirectory (Warp groups by
   category; "finance" is the closest fit — confirm with maintainers
   if unsure).
3. Open the PR with the title and body above.
4. Respond to CI lint findings (Warp's CI validates the YAML schema).

## Notes

- The validate-iso8583 workflow uses bash process substitution
  (`<(echo ...)`); confirm this is acceptable for Warp's
  cross-platform shells, otherwise rewrite with a temp file.
- The `source_url` / `author_url` fields point at the wireforge GitHub
  org URL — adjust if the org name changes.
