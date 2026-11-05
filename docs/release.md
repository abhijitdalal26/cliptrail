# Cutting a release

1. Bump the version in `Cargo.toml` (`workspace.package.version`),
   `apps/desktop/src-tauri/tauri.conf.json` and `apps/desktop/package.json`.
   The Android versionCode is derived from it by the Tauri CLI.
2. Add the changelog entry in `site/_src/pages.py` (`CHANGELOG`) and run
   `python site/_src/build.py`.
3. Merge to `main`, then tag: `git tag v0.1.0 && git push origin v0.1.0`.
4. The Release workflow builds Windows / Linux / macOS installers, the
   Android APK and the CLI archives into a **draft** release. Check it,
   then publish. Untick "pre-release" for versions installed desktop apps
   should be offered as an update.
5. The download page (`site/assets/download.js`) picks up the newest
   published release by itself.

## Signing keys (keep backups)

| What | Where (maintainer machine) | GitHub secret |
| --- | --- | --- |
| Desktop updater key | `%USERPROFILE%\.tauri\cliptrail-updater.key` (+ `.password`) | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| Android release keystore | `%USERPROFILE%\.cliptrail-release\cliptrail-release.jks` (+ `android-keystore.password`), alias `cliptrail` | `ANDROID_KEYSTORE_B64` (base64 of the .jks), `ANDROID_KEYSTORE_PASSWORD` |

The APK is not on the Play Store; people install it directly. Android only
installs an update signed with the **same** key, so losing the keystore
means every user has to uninstall (losing their phone history) to move to a
new one. Keep an offline backup of both files.

Local signed build: `apps/desktop/src-tauri/gen/android/keystore.properties`
(gitignored) with `storeFile`, `storePassword`, `keyAlias`, `keyPassword`;
then `pnpm tauri android build --apk --target aarch64` in `apps/desktop`.
