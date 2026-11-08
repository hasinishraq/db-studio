# Releasing

Section 9 of `BUILD_GUIDE.md` has the full process. These steps need your accounts and keys, so they
are not done in the repository.

## Enable the updater

1. Generate the key pair and keep the private key offline. If it is lost, installed apps can never
   update again.

   ```bash
   npx tauri signer generate -w ~/.tauri/dbstudio.key
   ```

2. In `src-tauri/tauri.conf.json`, set `plugins.updater.pubkey` to the contents of the `.pub` file,
   set `plugins.updater.endpoints` to
   `["https://github.com/<owner>/<repo>/releases/latest/download/latest.json"]`,
   and add `"createUpdaterArtifacts": true` under `bundle`.
3. Add these GitHub Actions secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
4. Wire the check into the UI. The plugin and its capability are installed, but nothing calls
   `check()` yet, because it has no key or endpoint to use. Follow "Update behaviour" in section 9:
   check 10 seconds after start and every 24 hours, never during a run or an open transaction.

## Sign the installers

- macOS: add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
  `APPLE_PASSWORD` and `APPLE_TEAM_ID` as secrets.
- Windows: configure `bundle.windows` with a certificate thumbprint or a `signCommand`. Unsigned
  builds work but show SmartScreen warnings.
- Linux: no OS signing is needed; the updater signature protects updates.

## Cut a release

```bash
git commit -am "release: v1.0.0-rc.1"
git tag v1.0.0-rc.1
git push origin main --tags
```

The `Release` workflow builds on all platforms and uploads a draft GitHub release. Test the installers,
then publish the draft.

Before tagging, tick the release checklist in section 10 of the build guide. The safety suite must pass
against every engine.
