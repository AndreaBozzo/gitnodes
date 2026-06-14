# Package-manager release metadata

GitNodes releases generate ready-to-publish metadata for:

- Homebrew: `gitnodes.rb`
- WinGet: the three `AndreaBozzo.GitNodes*.yaml` manifests

The templates live here so package definitions are reviewed with the code. The
release workflow renders them with the release version and archive SHA-256
checksums, then attaches the rendered files and `SHA256SUMS` to the GitHub
release. Download URLs are rendered from the repository running the release
workflow, so the metadata follows the new public upstream without hardcoding
the current development repository.

These files are release preparation, not evidence of a currently published
package. The installer URLs, Homebrew formula, and WinGet manifests become
usable after the public upstream publishes its first binary release.

One-time publication still happens in the external package repositories:

- copy `gitnodes.rb` into the `AndreaBozzo/homebrew-tap` repository;
- submit the WinGet files under
  `manifests/a/AndreaBozzo/GitNodes/<version>/` in `microsoft/winget-pkgs`.

For a local dry run:

```bash
scripts/render-package-manifests.sh \
  0.3.0 \
  <linux-x64-sha256> \
  <macos-x64-sha256> \
  <macos-arm64-sha256> \
  <windows-x64-sha256> \
  dist/package-manifests
```
