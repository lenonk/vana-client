# winit patch

winit **0.30.13**, vendored so `[patch.crates-io]` can replace the crates.io crate. The only
source change is `src/platform_impl/android/mod.rs`. Other platforms build the same code as stock.

## Changes (`src/platform_impl/android/mod.rs`)

NativeActivity delivers input on the NDK `AInputQueue` only. Stock winit turns every
`MotionEvent` into `Touch` and every pad `KeyEvent` into `KeyboardInput`, so:

1. **Gamepad / joystick** (`SOURCE_CLASS_JOYSTICK` / `SOURCE_GAMEPAD`) → call
   `vana_android_gamepad_motion` / `vana_android_gamepad_key` (XInput-style state in
   `vana-client`), and do **not** emit Touch / KeyboardInput for those events.
2. **Mouse** → `CursorMoved` + `MouseInput` from `button_state` (hover clears buttons so a
   missed UP cannot stick `left_down` / camera-drag); no Touch (avoids double handling with
   the client’s touch→mouse bridge).

3. **Lifecycle**: `InitWindow` marks the loop running and asks for a redraw; `Resume` sends `Resumed` and a redraw, so
   the client rebuilds its surface after another activity (Settings, the browser, the folder picker).

Rebuild the APK after changing this tree (`[patch.crates-io] winit` in the workspace
`Cargo.toml`).
