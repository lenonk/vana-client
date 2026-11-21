# vana-client on Android

The APK does **not** ship FINAL FANTASY XI game data. Square Enix’s install is many gigabytes and must live on the device’s storage separately (for example under `Download`, `Documents`, or an SD card). The client only stores a path in `config.json` (`game_dir`) that points at that tree.

## Layout the client expects

`game_dir` is the folder that **contains** `FINAL FANTASY XI` (the same as a Windows install’s `PlayOnline\SquareEnix` or similar). Inside that folder the client looks for `FINAL FANTASY XI/VTABLE.DAT` and the rest of the retail files.

The first start has two steps:

1. **All files access**: *Open settings* and allow vana-client. The game files and installer volumes are read as plain paths on shared storage, so nothing loads without it.
2. **Game files**, from one of:
   - **Square Enix**: opens PlayOnline's download page. Download all five US volumes (`FFXIFullSetup_US.part1.exe`, `part2.rar` … `part5.rar`) into one folder under **Download**.
   - **Local installer**: installs from those volumes (found automatically under Download, or the *Archives* folder) into *Install to*, by default `/storage/emulated/0/Game/FFXI`. The volumes are left in place.
   - **Existing install**: pick a folder holding `FINAL FANTASY XI/VTABLE.DAT` (or a folder above it).

**Config → Game data** on the title screen changes the folder later. Config, mods, logs and caches stay in app storage.

## Mods, addons, and Download

**Settings → Mods** scans **`/storage/emulated/0/Download`** (and subfolders) for zip/7z/rar packs and opens **Install from File** there. That needs **All files access** (same as playing in zones), or the folder stays empty in the app.

Drop LuAshitacast exports, DAT packs, or mod archives in Download, grant access, then use **Found in the Downloads folder** or **Install from File**.

## Full screen

The activity runs in **immersive** mode (status and navigation bars hidden; swipe from the edge to peek).

## Launcher icon

Mipmaps come from `crates/vana-client/assets/icon-256.png` (same as desktop). Regenerate with:

```bash
android/gen-icons.sh
```

## Release signing (sideload)

Every APK must be signed. For installs you keep updating on your own devices, use one **release keystore** (not the debug key):

```bash
android/gen-signing-key.sh
```

That creates `~/.config/vana-client-android/release.keystore` and writes `android/local.properties` (gitignored). Back up the keystore and passwords; changing the key requires uninstalling the app before installing a new APK.

Release builds pick up signing automatically. Override paths with `VANA_ANDROID_KEYSTORE` and related env vars (see `android/local.properties.example`).

CI (`.github/workflows/android.yml`) builds a debug-signed APK unless the repository has these Actions secrets, then a release-signed one:

- `VANA_ANDROID_KEYSTORE_BASE64`: the keystore file, `base64 -w0 release.keystore`
- `VANA_ANDROID_KEYSTORE_PASSWORD`, `VANA_ANDROID_KEY_PASSWORD`
- `VANA_ANDROID_KEY_ALIAS` (`vana-client` from `gen-signing-key.sh`)

## Building

`release/build-android-apk.sh` cross-compiles `libvana_client.so` (NDK 27, `cargo-ndk`) and packages the APK with Gradle as `target/assets/vana-client-android-aarch64.apk`; `VANA_ANDROID_APK_VARIANT=release` builds the signed release. Game files are never part of the APK.