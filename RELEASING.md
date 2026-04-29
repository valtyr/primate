# Releasing primate

primate ships three artifacts on each release:

1. The Rust crate `primate` on **crates.io**.
2. The VS Code extension on the **VS Code Marketplace** and **Open VSX**.
3. The Zed extension on **zed-industries/extensions**.

All three are versioned in lockstep — the version in `Cargo.toml`,
`editors/vscode/package.json`, and `editors/zed/extension.toml` should
always match.

## The release flow

Authoring a release is a PR-based flow driven by **release-plz**:

1. Land changes on `main` using [conventional commits]
   (`feat:` for minor bumps, `fix:` for patches, `feat!:` or
   `BREAKING CHANGE:` for majors).
2. The `Release-plz` workflow opens (or updates) a **"chore: release"**
   PR that bumps `Cargo.toml` and prepends to `CHANGELOG.md`. A
   follow-up commit on the same branch syncs
   `editors/vscode/package.json` and `editors/zed/extension.toml` to
   the new version, so all three artifacts stay in lockstep without
   any manual intervention.
3. Merge the PR. release-plz publishes `primate` to crates.io,
   creates a GitHub Release, and pushes a `vX.Y.Z` tag.
4. The `Publish extensions` workflow fires on the tag and publishes
   the VS Code extension to the Marketplace + Open VSX, and opens a
   PR to `zed-industries/extensions` for the Zed extension.

## Required secrets

Configure these in **Settings → Secrets and variables → Actions** on
the GitHub repo:

| Secret                  | Purpose                                                 | How to get it |
| ----------------------- | ------------------------------------------------------- | ------------- |
| `CARGO_REGISTRY_TOKEN`  | `cargo publish` to crates.io.                           | <https://crates.io/settings/tokens> — needs `publish-update` for `primate`. |
| `VSCE_PAT`              | `vsce publish` to VS Code Marketplace.                  | Azure DevOps PAT, scope **Marketplace → Manage**. <https://aka.ms/vscode-marketplace-manage-publishers> |
| `OVSX_PAT`              | `ovsx publish` to Open VSX.                             | <https://open-vsx.org/user-settings/tokens> |
| `COMMITTER_TOKEN`       | Open the PR to `zed-industries/extensions`.             | GitHub PAT (classic) with **repo** + **workflow** scopes. The PAT's user must have a fork of `zed-industries/extensions` named in the workflow's `push-to:` field. |

## Lockstep

release-plz bumps the Rust crate version; a follow-up step in the
same workflow syncs the editor manifests
(`editors/vscode/package.json` and `editors/zed/extension.toml`) to
match. The `Publish extensions` workflow still verifies the match at
publish time as a safety net — if the manifest version doesn't equal
the tag, the publish fails before reaching the marketplace.

If the editor extensions ever need to ship on a different cadence
than the crate, the simplest path is to skip the auto-sync (delete
the "Sync editor manifest versions" step in `release-plz.yml`) and
bump the manifests by hand only when an editor change actually needs
to ship.

## First-time setup

Before the first automated release fires:

1. Add the four secrets above.
2. On GitHub, create a fork of `zed-industries/extensions` under the
   account that owns `COMMITTER_TOKEN`.
3. Enable **Settings → Actions → General → Allow GitHub Actions to
   create and approve pull requests** so release-plz can open the
   release PR.

[conventional commits]: https://www.conventionalcommits.org/
