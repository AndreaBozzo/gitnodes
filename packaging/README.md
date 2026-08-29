# Package-manager release metadata

GitNodes is published to two package managers:

| Channel | Install | Where it lives |
|---|---|---|
| Homebrew | `brew install andreabozzo/tap/gitnodes` | [`AndreaBozzo/homebrew-tap`](https://github.com/AndreaBozzo/homebrew-tap) → `Formula/gitnodes.rb` |
| WinGet | `winget install AndreaBozzo.GitNodes` | [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) → `manifests/a/AndreaBozzo/GitNodes/<version>/` |

Both are live. WinGet accepted the package in
[winget-pkgs#396497](https://github.com/microsoft/winget-pkgs/pull/396497) and
currently carries 0.1.0 and 0.1.1.

The templates in this directory are the source for that metadata, kept beside
the code so package definitions are reviewed with it. The release workflow
renders them with the release version and archive SHA-256 checksums, then
attaches the rendered files and `SHA256SUMS` to the GitHub release. Download URLs
are rendered from the repository running the release workflow, so the metadata
follows the public upstream without hardcoding a repository name.

The release installers live in [`../scripts/`](../scripts/): `install.sh` for
macOS/Linux and `install.ps1` for Windows. Keeping executable release tooling
there leaves this directory focused on package-manager metadata.

## Publishing a new version

Rendering is automatic; **publishing is still manual.** After a stable release
finishes, take the rendered files from its assets and:

- **Homebrew** — commit `gitnodes.rb` to `AndreaBozzo/homebrew-tap` at
  `Formula/gitnodes.rb`.
- **WinGet** — add the three `AndreaBozzo.GitNodes*.yaml` manifests under
  `manifests/a/AndreaBozzo/GitNodes/<version>/` in a fork of
  `microsoft/winget-pkgs` and open a PR. Microsoft's bot validates and merges;
  the owner must sign the CLA on the first PR.

Automating both steps is tracked in
[#35](https://github.com/AndreaBozzo/gitnodes/issues/35) and needs a packaging
PAT with write access to the tap and the winget-pkgs fork.

## Local dry run

```bash
scripts/render-package-manifests.sh \
  0.3.0 \
  <linux-x64-sha256> \
  <macos-x64-sha256> \
  <macos-arm64-sha256> \
  <windows-x64-sha256> \
  dist/package-manifests
```
