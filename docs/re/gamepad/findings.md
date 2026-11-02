# FFXiMain.dll gamepad input — findings

Target: `/run/media/SSD/FFXI/vana-client/game/FINAL FANTASY XI/FFXiMain.dll`
(from `game_dir` in `~/.config/vana-client/config.json`), 2,901,584 bytes,
md5 `ac6d09d9cdb9dc2acb38317a5f57f3bd`, PE32 i386, image base 0x10000000, linker timestamp 2026-10-05.
The original file was only read. Working files are in `~/ffxi-pad-re/` on Steven's machine.

Legend: **[C]** confirmed from code, **[I]** inferred.

---

## 0. The DLL is packed. Read this before using any address

- The on-disk `.text` section has **0 raw bytes**. All code is LZ-compressed inside section `POL1`
  (0x109ff000). The entry stub at 0x10be4a10 calls a decompressor at 0x10be4aab
  (src=0x109ff000, len 0x1e5a10, dst=0x10001000, out len 0x32762e), applies relocs if rebased,
  then jumps to the real DllMain at **0x1031676f**. [C]
- Format: a flag byte, MSB first. 1 = literal byte. 0 = 2-byte back-reference
  (`off = ((b0<<8)|b1) & 0xfff`, `len = (b0>>4)+3`). An offset of 0 ends the stream. [C]
- `tools/unpack_pol1.py` rebuilds a static copy, **`FFXiMain.unpacked.dll`**. It produces exactly 0x32762e bytes,
  and the code at the OEP is a valid DllMain prologue. All addresses in this document refer to that file.
  Analysis databases: `unpacked.rzdb` (Rizin, `aaa`) and `ghidra/ffxi.gpr` (Ghidra headless).
  Helpers: `tools/rz.sh '<cmds>'` and `tools/gd.sh <addr>...` (Ghidra decompile into `gdecomp/`).
- **The coordinator's addresses only partly match this build.** FUN_101bcb60 matches.
  FUN_101bced0 is a real function, but it does something else (see §2).
  FUN_1011fe30 decompiles to mid-instruction garbage. 0x10361e28, 0x103619f0 and 0x103612e8 hold strings
  or high-entropy data here. The equivalents in this build:

  | coordinator name | this build (30261006_0 file above) |
  |---|---|
  | input object with pad words +0x220/+0x228/+0x238 | `*(0x104e1d44)` (same offsets, confirmed) |
  | FUN_1011fbc0 (compact keyboard -> pad word) | **FUN_10123700**, table 0x1036e0f0 |
  | FUN_1011fe30 (axes) | stick bytes are written in **FUN_10005a40**; read by FUN_10120c10/c40/c70/ca0 |
  | PTR_FUN_10361e28 (callbacks per key fn) | **0x1036db18** (2 ptrs per id, stride 8) |
  | binding table 0x103619f0 | **0x1036d6b0** (3 x {DIK scancode, flag byte} per id, stride 6) |
  | masks 0x103612e8 | **0x1036cf60** (u16 context mask per id, ANDed with current context `DAT_1036e3a0`) |
  | (new) analog callbacks per id | **0x1036d0d8** (2 float-returning ptrs per id) |

## a. Imports and loaders

| API | how | xrefs [C] |
|---|---|---|
| `DINPUT8!DirectInput8Create` | static import (IAT 0x10329030), thunk 0x1030df72 | FUN_100058f0 @0x1000590e (pad/kb/mouse init, version 0x800, IID at 0x10341f1c) |
| XInput | **no static import**. FUN_10008d90 builds `<install dir>\xinputdll.dll` (Square's wrapper DLL in the game folder) and test-loads it. FUN_10008e10 does `LoadLibraryA` + `GetProcAddress("XInputGetState_")`→`DAT_10456550`, `"XInputSetState_"`→`DAT_10456554`, `"XInputEnable_"`→`DAT_10456558`. FUN_10008e70 frees it. | FUN_100083d0 (only if padmode[5] `0x10455c38` set) |
| XInputGetState wrapper | FUN_10008ea0(userIndex, XINPUT_STATE*) | FUN_10005a40 |
| XInputEnable wrapper | FUN_10008f00(bool) | FUN_10005a40 (called with "app active" unless background mode) |
| WMI XInput detection | strings `Win32_PNPEntity`, `DeviceID`, `VID_%4X`/`PID_%4X` (0x1034fab4..). FUN_10008ab0 (from enum callback FUN_100057d0) is the standard "IsXInputDevice" check [I: by string pattern] | FUN_100057d0 |
| `winmm!joyGetPosEx` | **not imported**, no string | — |
| `RegQueryValueExA` (0x1032900c) | | FUN_100160c0, FUN_10018f80, **FUN_10018fc0** (string value reader used for padmode/padsin/padexsin), **FUN_10019060** (binary reader, padguid), **FUN_101bccb0** (pad tool launcher), FUN_102e73c5, FUN_102e7446, FUN_102ecc67, 0x1030fe14 |
| `RegOpenKeyExA` | | FUN_1000ed20, FUN_1000ed70, FUN_100160c0, FUN_101bccb0, 0x1030fdfc |
| `CreateProcessA` | | FUN_101bccb0 (FFXiPadConfig.exe), 0x1030fbbc |

DirectInput device setup (FUN_100083d0) [C]:
- EnumDevices(DI8DEVCLASS_GAMECTRL=4, cb FUN_100057d0, flags 0x101 = ATTACHEDONLY|FORCEFEEDBACK). It runs first when padmode[1] `0x10455c39` is set and then again with flags 1 (ATTACHEDONLY).
- SetDataFormat(`0x1034a0fc` = **c_dfDIJoystick2**: dwSize 0x18, flags DIDF_ABSAXIS, dataSize 0x110).
- SetProperty BUFFERSIZE 0x10. Cooperative level `EXCLUSIVE | (padmode[4] ? BACKGROUND : FOREGROUND)`.
- DIPROP_RANGE −127..127 on offsets 0,4,8,0xC,0x10,0x14,0x18,0x1C (lX,lY,lZ,lRx,lRy,lRz,slider0,slider1).
- If force feedback is supported and padmode[1] is set: creates a GUID_ConstantForce effect (`{13541C20-8E33-11D0-9AD0-00A0C9A06E35}` @0x10341d4c) into `DAT_10455c28` (vibration).
- The device lives at `*(0x10455c20)+0x90`. Keyboard is at +0x88 and mouse at +0x8c. The device needs Poll() when `+0xc98` is set.
- Device choice (FUN_100057d0): if padguid000 is the zero GUID, take the first device. When XInput is enabled and the device is an XInput device, set `+0xc99=1` and use XInput user index `+0xc9c`. Otherwise CreateDevice into +0x90.

## b. FUN_101bced0 and FUN_101bcb60

- **FUN_101bced0 is NOT a gamepad count.** [C] It returns −1 if `DAT_10662dc0` (a 0x4e34-byte object built in FUN_101a8b50 by FUN_101bcef0) is null. Otherwise it returns FUN_101bd820, which loads **`prohibited.txt`** and **`allowed.txt`** word lists (UTF-8, ≤100 entries, regex-like `\(`, `[`, `^`, flags `(i)`, `(s)`, `(n)`) via FUN_101bd880. It returns 1 on success and −1 on failure. [I: it is the chat word filter.]
- **FUN_101bcb60** returns 1 if `DAT_10662dbc` is null, otherwise FUN_101bcc30. [C] That function:
  `FindWindowA(NULL,"FINAL FANTASY XI GAMEPAD Config")`, then if none is running, spawns a thread (FUN_10315c7c → 0x101bcca0 → **FUN_101bccb0**).
  FUN_101bccb0 reads `HKLM\SOFTWARE\PlayOnline{,US,EU}\InstallFolder` value `"0001"` (region index `*(DAT_10456a28+4)`),
  formats `"%s\%s" -game` with `Tools{,US,EU}\FFXiPadConfig.exe` (table 0x1037ca14), runs `CreateProcessA`,
  then calls `FUN_10005600(1)` (suspends pad input / forces state while the tool runs [I]).
  FUN_101bcb80 reports whether the tool is active (`*(DAT_10662dbc+4)==1`).
- Callers: FUN_101bcb60 is called from FUN_1019db70 @0x1019dc13 and 0x1019689a. FUN_101bced0 is called only from FUN_1019bec0 @0x1019beea.

## c. Registry values

Key: `HKLM\SOFTWARE\[Wow6432Node\]PlayOnline{,US,EU}\SquareEnix\FinalFantasyXI` (string values, read once in **FUN_10002840**, called from FUN_10002820 and FUN_10016820+0x512). The pad manager is `DAT_10455c20`. [C]

| value | parser | effect |
|---|---|---|
| `padmode000` | FUN_10005490 (strtok " ,", atoi==1 → flag) | 6 byte flags, in order: **[0] 0x10455c34** gamepad enabled (padsin/padexsin are applied only if set; device is (re)created only if set). **[1] 0x10455c39** enumerate force-feedback devices first and create the vibration effect. **[2] 0x10455c36** allow axis codes 7/8 (slider0/1). **[3] 0x10455c35** allow axis codes 9/10 (POV hat as an axis). **[4] 0x10455c37** background input (DISCL_BACKGROUND; keeps XInputEnable on). **[5] 0x10455c38** use XInput through xinputdll.dll. A change sets `0x10455c40` (re-create the device next frame). |
| `padsin000` | **FUN_10005130** | button/axis map, details below |
| `padexsin000` | FUN_10005320 | same format. Fills the *alternate* map (0x10456048 / raw 0x10455ba0), used while slot 34 is held |
| `padguid000` | FUN_10019060 binary → FUN_100055a0 | 16-byte instance GUID of the chosen device → `0x10455b10`. A change triggers re-creation. Log string prints it as `%08x-%04x-...` |

**padsin000 parsing (FUN_10005130) [C]:**
```c
int slotval[37] = {-1...}; char raw[32] = {-1...};      // raw @0x10455b20
tok = strtok(s, " ,");
for (p = 0; perm[p] != -1 && tok; p++) {                // perm @0x1034f930 (position -> slot)
    raw[p] = atoi(tok); slotval[perm[p]] = (char)raw[p]; tok = strtok(NULL, " ,");
}
uint64 btnmap[32] = {all -1};                           // @0x10455c48, index = DI button number
int64  axismap[12] = {0};                               // @0x10456448
for (slot = 0; slot < 37; slot++) {
    v = slotval[slot]; if (v == -1) continue;
    if (abs(v) < 32)  btnmap[abs(v)] = (v<0 ? ~0ull : 1ull) << slot;   // button -> pad bit "slot"
    else if (slot >= 17 && slot <= 24) axismap[slot-17] = v>0 ? v-31 : v+31;   // signed axis code
    else if (slot >= 4  && slot <= 7)  axismap[8+slot-4] = v>0 ? v-31 : v+31;  // d-pad from an axis
}
// flags: 0x10455c3c = |slot17|==|arg2|, 0x10455c3b = |slot4|==|arg2| (only when slot31 is set); else 0x10455c3e = 0
```
- **Button values 0..31** = DIJOYSTATE2 `rgbButtons[n]`. In XInput mode they are *virtual* button numbers (table 0x10329810):
  0=B, 1=X, 2=Y, 3=A, 4=DPad Right, 5=DPad Left, 6=DPad Up, 7=DPad Down, 8=LB, 9=LT, 10=L-stick click, 11=RB, 12=RT, 13=R-stick click, 14=Start, 15=Back.
- **Axis values ≥32**: code = |v|−31 → 1=lX, 2=lY, 3=lZ, 4=lRx, 5=lRy, 6=lRz, 7=slider0*, 8=slider1*, 9=POV x**, 10=POV y** (*needs padmode[2], **needs padmode[3]). Negative = inverted.
  In XInput mode lX/lY/lRx/lRy = sThumbLX/LY/RX/RY·127/32768.
  POV→x: 2250..15750 → +127, 20250..33750 → −127. POV→y: <6750 or >29250 → −127, 11250..24750 → +127 (FUN_100059c0 / FUN_10005a00).
- **No default padsin string exists in the DLL.** With no value, FUN_10005130 is called with NULL and returns, so the maps stay empty. Defaults come from FFXiPadConfig.exe. [C for the DLL side]
- Full position → slot → key-function table, including Steven's current values: **`padsin_order.md`**.
- Steven's live values (Steam Wine prefix `compatdata/2473131497/pfx/system.reg`, saved in `registry_steam_prefix.txt`):
  `padmode000="1,1,0,0,0,1"` (enabled, FF/vibration, XInput), `padsin000="8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1"`,
  `padguid000={55DDF31E-8F9C-11F1-8001-000044455354}`.

Position → slot permutation (0x1034f930) [C]:
```
pos : 0 1  2  3  4 5 6 7 8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
slot: 8 9 14 13 10 3 1 0 2 16 25 26 15 23 24 22 21 19 20 18 17  6  7  5  4 12 31 32 33 34 35 36
```

## d. Per-frame poll — FUN_10005a40 (called from FUN_10011a00 @0x1001287b)

Input object `I = *(0x104e1d44)` [C]:

| offset | meaning |
|---|---|
| +0x220/+0x224 | **held** pad word, 64-bit, bit n = slot n (+0x224 = "second word", slots 32..63) |
| +0x228/+0x22c | **pressed edge** |
| +0x230/+0x234 | **released edge** |
| +0x238/+0x23c | **repeat** (edge, plus auto-repeat once held longer than `mgr+0x94`, timer +0x248) |
| +0x250 | camera X byte (slots 17/18 axis), 0x80 = centre |
| +0x251 | camera Y byte (slots 19/20) |
| +0x252 | move X byte (slots 21/22) |
| +0x253 | move Y byte (slots 23/24) |

Each frame [C]:
1. Saves the previous held word, then clears +0x220..+0x23c and sets the 4 stick bytes to 0x80.
2. **DirectInput path:** optional `Poll()`, `GetDeviceState(0x110 = DIJOYSTATE2)` into `mgr+0xa44`. For each button i<32 with `rgbButtons[i]&0x80`: `held |= btnmap[i]`. If any pressed button maps to bit 34, the alternate map 0x10456048 is used for the other buttons. Buttons mapped to slots 4–7 / 17–24 also set the matching direction to ±127.
   Then for each of the 12 `axismap` entries a jump table (0x10007788, case = code+10) copies `±(short)DIJOYSTATE2.field` into the destination variable:
   entries 0,1 → camX (slots 17,18), 2,3 → camY (19,20), 4,5 → moveX (21,22), 6,7 → moveY (23,24), 8,9 → dpadX (slots 4,5), 10,11 → dpadY (slots 6,7).
   **XInput path** (when `mgr+0xc99`): `XInputGetState(mgr+0xc9c)`. Each of 24 entries in table 0x10329810 `{u64 padbit; u32 virtBtnBit; u32 xinputMask}` (0x10000=LT, 0x20000=RT, else a wButtons mask) is translated through `raw[]` and table 0x10329688/0x10329790 to the **same slot bits** as the DI path. Thumbs are scaled into the DIJOYSTATE2 buffer and then go through the same axis switch (jump table 0x100077fc).
3. Stick modes: holding slot 25 (or `DAT_10455c3e==1`) routes the movement stick to the D-pad; holding slot 26 (or mode 2) routes it to the camera. If the flags `0x10455c3b`/`0x10455c3c` are set (d-pad or camera bound to the same axis as slot 31), those outputs are zeroed. A pressed edge on **slot 31** cycles `DAT_10455c3e` 0→1→2.
4. Dead zone: |v| < 42 → 0 for every axis. The movement vector is renormalised when x²+y² > 9677. Values are clamped to ±127.
   **`I+0x252 = moveX−0x80`, `+0x253 = moveY−0x80`, `+0x250 = camX−0x80`, `+0x251 = camY−0x80`.**
   The game reads them as `(b−0x80)/128` (FUN_10120c10 → +0x252, FUN_10120c40 → +0x253, FUN_10120c70 → +0x250, FUN_10120ca0 → +0x251; constant 0x3c000000 = 1/128).
5. D-pad axis to bits: dpadX ≥ 64 → bit 0x10 (slot 4), ≤ −64 → 0x20 (slot 5). dpadY ≥ 64 → 0x80 (slot 7), ≤ −64 → 0x40 (slot 6). These also raise edge and repeat when newly set.
6. Edges: `edge = (held ^ prev) & held`, `released = edge ^ held ^ prev` (= prev & ~held), `repeat = edge` when nonzero. Buffered DI data (`GetDeviceData`, 0x14 bytes each) also ORs press/release edges using btnmap (or the alternate map while bit 34 is held).
7. Keyboard: **FUN_10123700** returns a 64-bit word from the compact-keyboard table 0x1036e0f0 (entries `{scancode, modreq, modeMask, ..., u64 padbit}`, see `compact_keyboard_table.md`), which is ORed into held. It also forces stick bytes: W→slot23 (+0x253=0), S→slot24 (0xff), A→slot22 (+0x252=0), D→slot21 (0xff), J/L→slots 18/17 (+0x250), I/K→slots 19/20 (+0x251).
   This is why slot 23 = "forward", 21 = "right", 17 = "camera right", 19 = "camera up" [I from the keyboard table; the signs match the button code above].

Bit of each physical input is not fixed. It is `slot = perm[position]` where `padsin[position] = button`. For Steven's XInput config this gives:
A→slot0 (0x1), X→slot1 (0x2), Y→slot2 (0x4), B→slot3 (0x8), DPad R/L/U/D→slots 4/5/6/7, LB→slot8 (0x100), LT→slot9 (0x200), L3→slot10 (0x400), RB→slot12 (0x1000), RT→slot13 (0x2000), R3→slot14 (0x4000), Start→slot15 (0x8000), Back→slot16 (0x10000), left stick → moveX/Y (lX, −lY), right stick → cam (lRx, −lRy).
[I] The −33/−36 inversion fits XInput, where LY up = +32767 → −127 → "forward" (0x253 low). A DirectInput pad (lY up negative) would need +33 for the same result.

## e. Key-function id → pad bit table

`FUN_10123a70(?, id, mode, which)` is the key-function test [C]:
- returns false if the id's context mask `0x1036cf60[id] & DAT_1036e3a0` is 0.
- calls the pad tester `0x1036db18[id*2]` with `mode` (0 = repeat, 1 = pressed edge, 2 = repeat+consume, 3 = edge+consume, 4 = held, 5 = released).
- if that returns false, checks up to 3 keyboard bindings `0x1036d6b0[id]` (`{DIK scancode, flags}`; flags 0x01 Shift, 0x02 Ctrl, 0x04 Alt, 0x08 Ctrl+Alt, 0x10 (Ctrl or Alt) and `DAT_1036cf5c`, 0x20 not Shift, 0x80 no Ctrl/Alt, 0x40 in high byte = not while chat input is active [I]).
- `FUN_10123970(?, id)` is the analog version through `0x1036d0d8[id*2..+1]` (two float sources; the larger magnitude wins).

The tester functions each test one bit of I+0x220 (or +0x224) in the 6 modes. Tester → slot [C]:
`10120e60`=0, `10121000`=1, `10120d70`=2, `10120f30`=3, `10121ea0`=4, `101220a0`=5, `10121db0`=6, `10121f90`=7, `101210f0`=8, `10121200`=9, `10121310`=10, `10121420`=12, `10121530`=13, `10121640`=14, `10121ca0`=15, `10121860`=16, `101226d0`=21, `101227e0`=22, `10122480`=23, `10122590`=24, `10121970`=27, `10121a80`=28, `10121b90`=29, `10121750`=30, **`10122a30`=32, `10122b20`=33, `10122c10`=35, `10122d00`=36 (second word, +0x224 bits 0,1,3,4)**.
Composites: `10122450` = 23|24, `101226a0` = 21|22, `101228f0`/`10122920`/`10122980`/`10122950` = (9 or 13 held) and 6/7/4/5, `101229b0`/`101229f0` = (9 and 13 held) and 4/5, `101234c0` = 8&10, `101234f0` = 12&14, `101235f0` = 9&10&13&14, `10123520` = 9|13|Alt|Ctrl, `10122190` = pressed edge of any of {0,1,2,3,8,9,10,12,13,14,15,16}. `10120980`/`10120a20`/`10120ad0` = mouse buttons (obj 0x104e1d4c), `10123110` = key 0x30.
The second callback column (`id*2+1`) is unused for every id except 0x9f (keyboard), and FUN_10123a70 never calls column 1.

**Full id table (0x00..0xba), with tester, analog sources and default keys: `keyfn_table.md`** (TSV: `keyfn_table.tsv`). Highlights:
- 0x00–0x03 = slots 6/7/4/5 (D-pad up/down/right/left; keys Up/Down/Right/Left). 0x04/0x05 = movement stick (analog +0x252/+0x253, digital slots 23|24 / 21|22). 0x06/0x07 = camera analog (+0x250/+0x251). 0x08–0x0b = D-pad as an analog axis.
- 0x0e (Enter/Y) → slot0, 0x0f (Esc/N) → slot3, 0x11 (F/Num+) → slot2, 0x18/0x19 (−/Num−) → slot1, 0x10 (V/Num5) → slot14, 0x13/0x14 (Pause) → slot15, 0x12/0x15/0x1c (Num7) → slot8, 0x16/0x21 → slot9, 0x17/0x20 → slot13, 0x1a/0x1e/0x24 → slot12, 0x26/0x2f/0x56 → slot16, 0x30/0x44/0x7e (Num*) → slot10, 0x4f/0xa4 (PgUp/Num9) → slot27, 0x50/0xa5 (PgDn/Num3) → slot28, 0x45 (Z, Num/) → slot29, 0x55 ([, SysRq) → slot30.
- **0xb7 → slot32, 0xb8 → slot33, 0xb9 → slot35, 0xba → slot36** (second word), i.e. padsin positions 27, 28, 30, 31.
- Slots 25, 26, 31 and 34 are not tested by any key-function id. They are consumed inside FUN_10005a40 (stick routing, mode cycle, alternate map).

## f. Config id 0xce

FUN_1019bec0 (config setter): for id 0xce, values other than 1/2 are stored directly into `this+0x24`. Values 1 or 2 are kept only if `FUN_101bced0() > 0`, meaning `prohibited.txt` loaded. Otherwise it calls `FUN_10193830(0xce, 0)` (reset to 0). [C]
Since FUN_101bced0 is the chat word-list loader, **0xce is a chat-filter option, not a gamepad option.** [I: strong] Nothing in the pad code (FUN_10005a40/10005130/100083d0) reads it.

## Not done / open

- I did not resolve what consumes `this+0x24` for config 0xce.
- The 2 "mode" bytes in the compact keyboard table (modreq 0/4/7/8, modeMask 4/5/6) are only partly decoded. Entries 27–34 have scancode 0 (probably filled at runtime).
- Slot meanings beyond confirm/cancel/movement/camera/D-pad are inferred from default keyboard bindings only.

## Files

In this repo (`docs/re/gamepad/`): this file, `keyfn_table.md` (id → pad bit table), `padsin_order.md` (position order with Steven's values), and `tools/unpack_pol1.py`.
On Steven's machine (`~/ffxi-pad-re/`, not committed: binaries and large dumps): `FFXiMain.unpacked.dll`, Rizin/Ghidra databases, `full_text.asm`, `gdecomp/*.c` pseudo-C, raw table dumps, `tools/gen_table.py`, `compact_keyboard_table.md`.
Rebuild the unpacked DLL with: `python3 tools/unpack_pol1.py FFXiMain.dll FFXiMain.unpacked.dll`.

---

## Reference: pad input details (FUN_10005a40 and friends)

### padsin position → slot permutation (table 0x1034f930, all 32 entries) [C]

The table holds 32 position entries, 4 more slot ids (33..36 are reachable only via positions 27..31), and ends with -1.

| pos | slot | | pos | slot | | pos | slot | | pos | slot |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 8 | | 8 | 2 | | 16 | 21 | | 24 | 4 |
| 1 | 9 | | 9 | 16 | | 17 | 19 | | 25 | 12 |
| 2 | 14 | | 10 | 25 | | 18 | 20 | | 26 | 31 |
| 3 | 13 | | 11 | 26 | | 19 | 18 | | 27 | 32 |
| 4 | 10 | | 12 | 15 | | 20 | 17 | | 28 | 33 |
| 5 | 3 | | 13 | 23 | | 21 | 6 | | 29 | 34 |
| 6 | 1 | | 14 | 24 | | 22 | 7 | | 30 | 35 |
| 7 | 0 | | 15 | 22 | | 23 | 5 | | 31 | 36 |

Raw dwords: `8 9 14 13 10 3 1 0 2 16 25 26 15 23 24 22 21 19 20 18 17 6 7 5 4 12 31 32 33 34 35 36 -1`.
For each position p with value v: if |v| < 32, then `btnmap[|v|] = 1ull << perm[p]`. Only one slot per physical button; the last one written wins.

### XInput virtual button numbering (table 0x10329810, 24 × 16 bytes {u64 padbit; u32 virtBtnBit; u32 xinputMask}) [C]

| XInput | mask | virtual button # (as used in padsin) |
|---|---|---|
| B | 0x2000 | 0 |
| X | 0x4000 | 1 |
| Y | 0x8000 | 2 |
| A | 0x1000 | 3 |
| DPad Right | 0x0008 | 4 |
| DPad Left | 0x0004 | 5 |
| DPad Up | 0x0001 | 6 |
| DPad Down | 0x0002 | 7 |
| LB | 0x0100 | 8 |
| LT (trigger byte != 0) | 0x10000 (pseudo) | 9 |
| Left stick click | 0x0040 | 10 |
| RB | 0x0200 | 11 |
| RT (trigger byte != 0) | 0x20000 (pseudo) | 12 |
| Right stick click | 0x0080 | 13 |
| Start | 0x0010 | 14 |
| Back | 0x0020 | 15 |

Entries 16..23 of the table carry no XInput mask. They hold the pad bits for slots 25, 26, 31, 32..36, so a padsin that points those positions at a virtual button still works.
Only virtual numbers < 17 are matched in XInput mode. The XInput path does **not** use `btnmap` directly: pressed button → virtual number → padsin position holding that number → canonical bit for that position (0x10329688) → table entry → pad bit. The result equals `1 << perm[pos]`, the same as DirectInput.
XInput thumbs are scaled `(s16 * 127) >> 15` into the DIJOYSTATE2-shaped buffer: sThumbLX→lX(+0), LY→lY(+4), RX→lRx(+0xC), RY→lRy(+0x10). XInput "up" is positive, so padsin `-33`/`-36` (inverted Y) makes stick-up = forward / camera-up.

### Stick byte layout and dead zone [C]

Input object `I = *(u32*)0x104e1d44`:

| byte | fed by padsin slots | button-as-axis (slot held → value) | keyboard (FUN_10123700) |
|---|---|---|---|
| `I+0x250` camera X | 17/18 (pos 20/19) | slot17 → +127, slot18 → −127 | L → 0xff, J → 0x00 |
| `I+0x251` camera Y | 19/20 (pos 17/18) | slot19 → −127, slot20 → +127 | I → 0x00, K → 0xff |
| `I+0x252` move X | 21/22 (pos 16/15) | slot21 → +127, slot22 → −127 | D → 0xff, A → 0x00 |
| `I+0x253` move Y | 23/24 (pos 13/14) | slot23 → −127, slot24 → +127 | W → 0x00, S → 0xff |

- Stored as `(u8)(value - 0x80)`, so 0x80 = centre, 0x01 = full negative, 0xff = full positive. All four are reset to 0x80 at the start of every poll.
- Readers: FUN_10120c70 (+0x250), FUN_10120ca0 (+0x251), FUN_10120c10 (+0x252), FUN_10120c40 (+0x253). Each returns `(float)(b - 0x80) * (1/128)` (constant 0x3c000000 @0x1032a778).
- Dead zone: each axis value with `|v| < 42` (0x2a) becomes 0, after routing and before writing.
- Movement only: if `x² + y² > 9677` (0x25cd, ≈ 98.4²) the vector is rescaled (two `__ftol` calls; exact factor not decoded), then each component is clamped to ±127.
- D-pad from an axis (padsin positions 21–24 holding an axis code): dpadX ≥ 64 → slot4 bit 0x10, ≤ −64 → slot5 0x20. dpadY ≥ 64 → slot7 0x80, ≤ −64 → slot6 0x40.
- Routing before the dead zone: slot25 held or `DAT_10455c3e==1` → move stick drives the D-pad. Slot26 held or mode 2 → move stick drives the camera. A pressed edge on slot31 cycles mode 0→1→2, skipping a mode whose same-axis flag (`0x10455c3b` d-pad / `0x10455c3c` camera) is clear.

### Second pad word (`I+0x224`, slots 32..63) [C]

The pad words are 64-bit: low dword at +0x220, high dword at +0x224 (likewise +0x228/+0x22c, +0x230/+0x234, +0x238/+0x23c).

| hi bit | slot | padsin pos | tested by | key fn id |
|---|---|---|---|---|
| 0x01 | 32 | 27 | FUN_10122a30 | 0xb7 |
| 0x02 | 33 | 28 | FUN_10122b20 | 0xb8 |
| 0x04 | 34 | 29 | (none; internal) | — while held, buttons use the padexsin000 map (0x10456048) |
| 0x08 | 35 | 30 | FUN_10122c10 | 0xb9 |
| 0x10 | 36 | 31 | FUN_10122d00 | 0xba |
| 0x20 / 0x40 | 37 / 38 | — | compact keyboard Q / E only (table 0x1036e0f0) | — |

The modifier check is `(I+0x224 & 4)` for buffered DI data. In the immediate DI state loop it checks whether any pressed button's `btnmap` value is exactly `{lo 0, hi 4}`.

### How held / pressed / released / repeat are computed (FUN_10005a40) [C]

Per frame:
```c
prev = I->held64;                         // saved first
I->held = I->pressed = I->released = I->repeat = 0;  sticks = 0x80;
// device path (DI immediate state or XInput) ORs bits into I->held,
// and sets pressed|repeat for d-pad-from-axis bits that were not in prev
if (held != prev) {                       // device bits only
    I->pressed  = (held ^ prev) & held;            // newly down
    I->released = I->pressed ^ held ^ prev;        // = prev & ~held
    if (I->pressed) I->repeat = I->pressed;
}
// DI buffered events (GetDeviceData, 16 entries): press (0x80) → pressed|repeat|held |= map,
//   release (0) → released |= map, held ^= map
kb = FUN_10123700();  I->held |= kb;      // compact keyboard bits
for each bit b in 0..63:                  // reconcile with keyboard bits(simplified)
    if (kb==0) { if (b in prev && !(b in held)) released |= b; }
    else if (b in kb && !(b in prev)) { pressed |= b; repeat |= b; }
    else if (b in kb && b in released) released ^= b;
if (pressed & (0x80|0x40)) pressed &= ~0x30;   // D-pad: vertical wins over horizontal
if (repeat  & (0x80|0x40)) repeat  &= ~0x30;
// auto-repeat
if (pressed == 0 && released == 0) {
    if (held) {
        I->timer(+0x248) += FUN_10014cf0();       // frame delta, clamped to >= 1.0
        if (!mgr->armed(+0xdac)) mgr->armed = 1;  // skip the first frame after an edge
        else if (I->timer > (float)mgr->repeatDelay(+0x94)) {
            I->repeat |= I->held;                 // whole held word repeats
            I->timer = (float)mgr->repeatDelay - 2.0f;   // 2.0f @0x10329990
        }
    }
} else { I->timer = 0; mgr->armed = 0; }
if (I->pressed & 0x80000000) cycle DAT_10455c3e;   // slot31
```
- `repeatDelay` (`mgr+0x94`) is initialised to **16** in FUN_10005660 (pad manager reset). This is the only constant writer I found; a settings path may change it at runtime.
- `FUN_10014cf0()` returns `max(1.0, *(float*)(DAT_104568fc+0x28))`, the engine's frame delta in ticks. [I] Ticks are 1/60 s, which gives a first repeat after ≈16 ticks (~0.27 s) and then one every 2 ticks (~30 Hz).
- `mgr+0xdac` delays arming by one frame after any edge. The timer resets to 0 on any pressed or released edge, so pressing a second button restarts the repeat delay.
- Testers read `repeat` for mode 0, `pressed` for mode 1, and `held` for mode 4. Modes 2/3 read repeat/pressed and then clear all pressed bits except their own (consume).

### Steven's current values (Steam Wine prefix, verbatim)

From `~/.local/share/Steam/steamapps/compatdata/2473131497/pfx/system.reg`, key `[Software\\Wow6432Node\\PlayOnlineUS\\SquareEnix\\FinalFantasyXI]`:
```
"padexsin000"="-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-33,-33,32,32,-36,-36,35,35,6,7,5,4,-1,-1,-1,-1,-1,-1,-1"
"padguid000"="{55DDF31E-8F9C-11F1-8001-000044455354}"
"padmode000"="1,1,0,0,0,1"
"padsin000"="8,9,13,12,10,0,1,3,2,15,-1,-1,14,-33,-33,32,32,-36,-36,35,35,6,7,5,4,11,-1,-1,-1,-1,-1,-1"
```
padmode000 = enabled, force feedback, no sliders, no POV-axis, foreground only, XInput.
