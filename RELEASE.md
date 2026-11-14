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
3. **Bump and tag**, with a clean tree, on `main` or a branch that
   `main` can fast-forward to:

   ```sh
   scripts/bump-version.sh 0.2.0
   ```

   It sets the version in `app/package.json`, `app/src-tauri/Cargo.toml`
   and `app/src-tauri/tauri.conf.json` (and the lock files), dates the
   changelog section, commits "Release 0.2.0", pushes it to `main` (and
   your branch), then tags `v0.2.0` and pushes the tag. `--local` changes
   the files only.
4. **Watch** the Release workflow (`gh run watch`). It checks the tag
   matches the version, runs the Rust and unit tests, builds, signs,
   notarizes (several minutes), uploads, and checks the signature.
5. **Check the draft by hand** (below), then **Publish** it on GitHub.

Only a tag on a commit that is on `main` builds: the workflow checks
that first, then that every secret is set.

A failed run: fix and commit, delete the draft (`gh release delete
v0.2.0`), then `scripts/retrigger-release.sh 0.2.0` (it pushes the fix
to `main` and moves the tag).

## By-hand check before publishing

CI can't see these. Use the `.dmg` from the draft.

| | Check | Pass when |
|---|---|---|
| a | Open the `.dmg` on a Mac that never ran Kibann, drag to Applications, open | No Gatekeeper warning |
| b | Splits, tabs, vim, htop, Claude Code | Look and behave right |
| c | Quit, reopen | The shells are still running |
| d | An agent waits in a background tab | The notification says Kibann, with its icon; clicking it shows the pane |
| d1 | OpenCode in a background tab: ask it to run a command, with `"permission": {"bash": "ask"}` in its config | The pane's dot turns amber and "OpenCode is waiting for you" appears; after Allow, green, then blue when it finishes |
| d2 | Close the window; an agent waits | The menu-bar icon gets a dot and "1 agent is waiting"; that item, the icon's Show and the Dock icon bring the window back |
| d3 | Close the last pane, then the icon's Show | The window comes back with a fresh shell |
| d4 | The icon's Quit with shells open | It asks; Quit ends the shells, and the next start reopens the tabs with fresh shells |
| d4b | Cmd+Q, then the icon's Show | The window hid and the icon stayed; Show brings it back with the same shells |
| d4c | Double-click the title bar, twice | The window zooms to fill the screen, then goes back (or minimizes, if System Settings says so) |
| d5 | The icon in a light and a dark menu bar; Help → What's New | The K is clear in both; Settings opens on Updates, with this version marked |
| d6 | Cmd+, → General: turn a switch off, then Open in Editor | The config file opens in the text editor with the line in it |
| d7 | Settings → Appearance: pick a font from the list, then − / + | The panes change font and size within a second |
| e | With the previous release installed, wait for (or Check for Updates…) the notice, Restart to update | The new version opens; shells kept, or the protocol prompt if the notes say so |

## How updates reach users

The app checks 20 s after starting and every hour (and on Kibann
Terminal → Check for Updates…, or the menu-bar icon's), downloads a newer
version in the background, and shows "Kibann Terminal 0.2.0 is ready" with
**Restart to update** (also in the menu-bar icon's menu). The first start
of the new version offers its notes (Help → What's New). Restarting replaces the app; the daemon keeps the shells and the
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
