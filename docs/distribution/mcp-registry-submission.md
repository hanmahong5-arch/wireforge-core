# MCP Registry Submission Runbook — Wireforge

Server name: `io.github.hanmahong5-arch/wireforge`
Bundle format: **mcpb** (MCP Bundle, `.mcpb`)

## Spec sources used

| Document | URL |
|---|---|
| MCPB manifest spec v0.3 | <https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md> |
| MCP Registry package types | <https://modelcontextprotocol.io/registry/package-types> |
| MCP Registry publish quickstart | <https://modelcontextprotocol.io/registry/quickstart> |

---

## Why mcpb and not npm/pypi/nuget

The MCP Registry `server.json` `registryType` field accepts `npm`, `pypi`, `nuget`, `oci`, and `mcpb`. Cargo/crates.io is not a supported type. For a compiled Rust binary the only viable path is **mcpb**: a ZIP archive containing `manifest.json` and the pre-built binary, uploaded as a GitHub Release asset, then referenced by URL in `server.json`.

---

## Phase 1 — Build the .mcpb bundle

### 1.1 Install the release target (if cross-compiling)

```bash
# Example: add Linux x86_64 target from Windows host
C:/Users/Anita/.cargo/bin/rustup target add x86_64-unknown-linux-gnu
```

For local testing on Windows, the native target `x86_64-pc-windows-msvc` requires no additional setup.

### 1.2 Run the build script

```bash
# From repo root — native build
bash mcpb/build-mcpb.sh

# Or with an explicit target
bash mcpb/build-mcpb.sh x86_64-unknown-linux-gnu
```

The script:
1. Runs `cargo build --release -p wf-mcp --target <TARGET>`.
2. Stages `manifest.json` + the binary into `server/<binary>` inside a temp dir.
3. Zips to `mcpb/dist/wireforge.mcpb`.
4. Prints and saves the SHA-256 to `mcpb/dist/wireforge.mcpb.sha256`.

Bundle layout inside the ZIP:

```
wireforge.mcpb (ZIP)
├── manifest.json
└── server/
    └── wf-mcp          (or wf-mcp.exe on Windows)
```

This matches what `manifest.json` declares in `server.entry_point` (`server/wf-mcp`) and `mcp_config.command` (`${__dirname}/server/wf-mcp`, with a `platform_overrides.win32` pointing to `${__dirname}/server/wf-mcp.exe`).

### 1.3 Note on multi-platform bundles

⏳ **verify** — The mcpb spec v0.3 supports `platform_overrides` in `mcp_config` but it is unclear whether a single `.mcpb` ZIP can embed *multiple* platform binaries (e.g., `server/wf-mcp` for Linux/macOS and `server/wf-mcp.exe` for Windows) and have clients pick the right one at install time. The reference examples show single-platform bundles. Safe approach: produce one bundle per platform and publish separate registry entries with platform-specific identifiers, or wait for official multi-platform guidance. Until confirmed, the build script produces one bundle per invocation.

---

## Phase 2 — Create a GitHub Release and upload the bundle

1. Tag the commit:
   ```bash
   git tag -a v0.1.0 -m "Release v0.1.0"
   git push origin v0.1.0
   ```

2. Create a GitHub Release for tag `v0.1.0` via the GitHub UI or CLI:
   ```bash
   gh release create v0.1.0 \
     mcpb/dist/wireforge.mcpb \
     --title "Wireforge v0.1.0" \
     --notes "Initial MCP Registry release."
   ```

3. Note the release asset URL — it will be:
   ```
   https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wireforge.mcpb
   ```

   The URL contains `.mcpb` which satisfies the registry requirement that the identifier URL must contain the string `mcp`.

---

## Phase 3 — Update server.json

Open `server.json` (repo root) and add or update the `packages` array to include an mcpb entry. Replace `<SHA256>` with the hash printed by `build-mcpb.sh`.

```json
{
  "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
  "name": "io.github.hanmahong5-arch/wireforge",
  "title": "Wireforge",
  "description": "ISO 8583 / SWIFT MT/MX / SM-crypto financial-message toolkit (read-only, stdio).",
  "repository": {
    "url": "https://github.com/hanmahong5-arch/wireforge-core",
    "source": "github"
  },
  "version": "0.1.0",
  "packages": [
    {
      "registryType": "mcpb",
      "identifier": "https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wireforge.mcpb",
      "fileSha256": "<SHA256-from-build-mcpb.sh>",
      "transport": {
        "type": "stdio"
      }
    }
  ]
}
```

The `name` in `server.json` must match the `name` in `manifest.json`. Both are set to `io.github.hanmahong5-arch/wireforge`.

⏳ **verify** — The mcpb `server.json` schema does not require `mcpName` in a `Cargo.toml` or any package-manifest side-channel (unlike npm/pypi). Ownership is verified by the registry checking that the GitHub Release asset URL is owned by the authenticated GitHub account. Confirm this is still the case if the registry adds additional mcpb verification steps after this runbook was written.

---

## Phase 4 — Install mcp-publisher

### macOS / Linux

```bash
curl -L "https://github.com/modelcontextprotocol/registry/releases/latest/download/mcp-publisher_$(uname -s | tr '[:upper:]' '[:lower:]')_$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/').tar.gz" \
  | tar xz mcp-publisher
sudo mv mcp-publisher /usr/local/bin/
```

### Windows (PowerShell)

```powershell
$arch = if ([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -eq "Arm64") { "arm64" } else { "amd64" }
Invoke-WebRequest `
  -Uri "https://github.com/modelcontextprotocol/registry/releases/latest/download/mcp-publisher_windows_$arch.tar.gz" `
  -OutFile "mcp-publisher.tar.gz"
tar xf mcp-publisher.tar.gz mcp-publisher.exe
Remove-Item mcp-publisher.tar.gz
# Move mcp-publisher.exe to a directory on your PATH, e.g.:
Move-Item mcp-publisher.exe C:\Users\Anita\.cargo\bin\mcp-publisher.exe
```

Verify:

```bash
mcp-publisher --help
```

---

## Phase 5 — Authenticate with the registry

```bash
mcp-publisher login github
```

Follow the device-code flow printed in the terminal:
1. Visit `https://github.com/login/device`.
2. Enter the code shown (e.g., `ABCD-1234`).
3. Authorise the application.

You must authenticate as the GitHub user that owns `hanmahong5-arch`. The registry namespace `io.github.hanmahong5-arch/…` is validated against the authenticated GitHub account.

---

## Phase 6 — Publish

```bash
mcp-publisher publish
```

Expected output:

```
Publishing to https://registry.modelcontextprotocol.io...
✓ Successfully published
✓ Server io.github.hanmahong5-arch/wireforge version 0.1.0
```

Verify with the registry API:

```bash
curl "https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.hanmahong5-arch/wireforge"
```

---

## Troubleshooting

| Error | Action |
|---|---|
| `Registry validation failed for package` | Confirm `name` in `server.json` matches `name` in `manifest.json`; confirm `fileSha256` is correct. |
| `You do not have permission to publish this server` | Re-authenticate as the GitHub user that owns `hanmahong5-arch`. |
| `Invalid or expired Registry JWT token` | Run `mcp-publisher login github` again. |
| URL does not contain `mcp` | The `identifier` URL must contain the string `mcp`. The `.mcpb` extension satisfies this. If you rename the asset, keep `mcp` in the filename. |

---

## Automating with GitHub Actions

⏳ **verify** — The registry supports CI-based publishing. See <https://modelcontextprotocol.io/registry/github-actions> for the official GitHub Actions workflow. The high-level pattern is: build the binary in CI, pack the `.mcpb`, upload as a release asset, then run `mcp-publisher publish` using a stored registry token. Full details require following that page.

---

## Version update checklist

When releasing a new version (e.g., `0.2.0`):

1. `bash mcpb/build-mcpb.sh` → get new SHA-256.
2. Upload new `wireforge.mcpb` as a GitHub Release asset for the new tag.
3. Update `version` and `identifier` URL (new tag) and `fileSha256` in `server.json`.
4. Update `version` in `mcpb/manifest.json`.
5. `mcp-publisher publish`.
