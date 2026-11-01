# Portable updates and Apple signing

Release builds create one signed update feed for Windows x64, Linux x64 AppImage, and both Mac architectures. macOS uses a universal app archive. Debian packages update through the package manager or a new download; their in-app updater is hidden. The feed follows stable releases. Test releases do not replace the latest stable release.

Updater signatures use the existing `TAURI_SIGNING_PRIVATE_KEY` repository secret, with `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if encrypted. The reusable portable workflow inherits these secrets. Packaging clears generated bundle directories first, and publication requires all four platform entries and their signatures.

Apple Developer ID signing is separate from updater signing. The pipeline supports `APPLE_CERTIFICATE` (base64 P12), `APPLE_CERTIFICATE_PASSWORD`, and `APPLE_SIGNING_IDENTITY`. It imports the certificate into a temporary keychain, verifies the resulting app, and removes the temporary certificate and keychain. With `APPLE_ID`, `APPLE_PASSWORD` (app-specific), and `APPLE_TEAM_ID`, it also notarizes and verifies the stapled ticket. Incomplete configured credentials fail the build instead of silently downgrading it.

No Apple signing credentials were configured during this software pass. Mac test builds therefore remain previews without Developer ID notarization. Build-status JSON records signing and notarization separately. Configuring a real Apple identity and testing installation/TCC permissions are external release requirements.

Sources: [Tauri updater artifacts](https://v2.tauri.app/plugin/updater/) and [Apple signing/notarization setup](https://v2.tauri.app/distribute/sign/macos/).
