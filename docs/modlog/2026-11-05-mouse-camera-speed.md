## Mouse Camera Speed setting (2026-11-05)

Ours, not retail. Config `mouse_camera_speed` (%, default 100 = the speed drags had before) scales the motion a mouse
look drag (left, middle or right button) feeds `App::look`, before the Mouse/Cam. axis inversions and the per-mode
factors (0.008 / 0.02 / 0.03 / 0.004). Shown as "Mouse Camera Speed" in Settings (25% to 200%). The controller's
camera (client/gamepad.rs) does not read it.
