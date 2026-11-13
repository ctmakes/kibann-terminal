# Releasing Kibann Terminal

A version tag builds the macOS app (Apple silicon) in GitHub Actions,
signs and notarizes it, and attaches it to a **draft** release with the
updater's `latest.json`. Publishing the draft makes it the version
installed copies update to.

## Once: keys and secrets

1. **Updater key.** On your machine (never in the repo):

   ```sh
   npx @tauri-apps/cli signer generate -w ~/.tauri/kibann.key
   ```

   Choose a password without special characters. Put the printed public
   key in `app/src-tauri/tauri.conf.json` at `plugins.updater.pubkey`. Lose
   the private key and installed copies can never update again.

2. **Repository secrets** (Settings → Secrets and variables → Actions, or
   `gh secret set NAME`). GitHub never shows a secret again, so these come
   from the originals:

   | Secret | What |
   |---|---|
   | `APPLE_CERTIFICATE` | The Developer ID Application certificate, `.p12`, base64 (`base64 -i cert.p12 \| gh secret set APPLE_CERTIFICATE`) |
   | `APPLE_CERTIFICATE_PASSWORD` | The `.p12`'s password |
   | `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Name (TEAMID)` |
   | `APPLE_ID` | The Apple ID's email |
   | `APPLE_PASSWORD` | An app-specific password for it (appleid.apple.com) |
   | `APPLE_TEAM_ID` | The 10-character team ID |
   | `TAURI_SIGNING_PRIVATE_KEY` | `gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/kibann.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password |

3. The repository must be **public**: the updater downloads
   `latest.json` from the newest release without credentials.

## Each release

1. **CI green** on `main` (it's paused; run it: `gh workflow enable CI`,
   `gh workflow run CI --ref main`, disable again once it's running).
2. **CHANGELOG.md:** a section `## [0.2.0] - Unreleased`, in plain words.
   It becomes the release notes. Say so when an update ends sessions (the
   protocol changed: `PROTOCOL_VERSION` in `crates/proto/src/wire.rs`).
3. **Bump and tag** on `main`, clean:

   ```sh
   scripts/bump-version.sh 0.2.0
   ```

   It sets the version in `app/package.json`, `app/src-tauri/Cargo.toml`
   and `app/src-tauri/tauri.conf.json` (and the lock files), dates the
   changelog section, commits "Release 0.2.0", tags `v0.2.0` and pushes.
   `--local` changes the files only.
4. **Watch** the Release workflow (`gh run watch`). It checks the tag
   matches the version, runs the Rust and unit tests, builds, signs,
   notarizes (several minutes), uploads, and checks the signature.
5. **Check the draft by hand** (below), then **Publish** it on GitHub.

A release candidate (`scripts/bump-version.sh 0.2.0-rc.1`, from any
branch) makes a draft marked pre-release, with the 0.2.0 notes. Delete the
draft and the tag afterwards (`gh release delete v0.2.0-rc.1
--cleanup-tag`). The updater ignores drafts and pre-releases.

A failed run: fix, delete the draft, `scripts/retrigger-release.sh 0.2.0`.

## By-hand check before publishing

CI can't see these. Use the `.dmg` from the draft.

| | Check | Pass when |
|---|---|---|
| a | Open the `.dmg` on a Mac that never ran Kibann, drag to Applications, open | No Gatekeeper warning |
| b | Splits, tabs, vim, htop, Claude Code | Look and behave right |
| c | Quit, reopen | The shells are still running |
| d | An agent waits in a background tab | The notification says Kibann, with its icon; clicking it shows the pane |
| e | With the previous release installed, wait for (or Check for Updates…) the notice, Restart to update | The new version opens; shells kept, or the protocol prompt if the notes say so |

## How updates reach users

The app checks 20 s after starting and every 6 hours (and on Kibann
Terminal → Check for Updates…), downloads a newer version in the
background, and shows "Kibann Terminal 0.2.0 is ready" with **Restart to
update**. Restarting replaces the app; the daemon keeps the shells and the
new app picks them up, unless the protocol changed, in which case it asks
before ending them (the layout comes back with fresh shells).

## Troubleshooting

- **No update offered:** `https://github.com/ctmakes/kibann-terminal/releases/latest/download/latest.json`
  must load in a browser (public repo, release published, not a draft),
  and its version must be higher than the installed one.
- **Signature verification failed:** the public key in `tauri.conf.json`
  doesn't match the private key in the secret.
- **Notarization fails:** check the Apple secrets; `xcrun notarytool
  history` with the same credentials shows Apple's reason.
