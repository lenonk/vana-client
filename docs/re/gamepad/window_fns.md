# What the pad buttons do in retail's windows and in the field

Source: `FFXiMain.unpacked.dll`, the same build as findings.md. Read-only.
**[C]** = confirmed from code. **[I]** = inferred.
Button names assume Steven's padsin000: A=slot0, X=slot1, Y=slot2, B=slot3, LB=slot8, LT=slot9, L3=slot10, RB=slot12, RT=slot13, R3=slot14, Start=slot15, Back=slot16.
The coordinator's "FUN_1011da00" is **FUN_10123a70** in this build: `test(ctx, id, mode, which)`, where mode 0 = repeat, 1 = pressed, 4 = held, 5 = released.

## How a key function reaches a window [C]

1. **FUN_101ab820** runs every frame while a window has focus. It turns key functions into numbered window events with `FUN_1015dd00(event)`:

   | key fn (button) | event | | key fn (button) | event |
   |---|---|---|---|---|
   | 0x2b/0x2c/0x2d/0x2e (D-pad U/D/R/L) | 2/1/3/4 | | 0x86 (LT) | 0x0a |
   | 0x93 (A) pressed / repeat / released | 5 / 0x1d / 0x20 | | 0x85 (RT) | 0x0b |
   | 0x94 (B) pressed | 6 | | 0x22 (Y) | 0x0c |
   | 0x94 held-released / mouse R | 0x1e / 0x1f | | 0x24 (RB) | 0x0e |
   | 0x18 (X) | 7 | | 0x25 (X) | 0x0f |
   | 0x11 (Y) | 8 | | 0x26 (Back) | 0x10 |
   | 0x1e (RB) | 0x11 | | 0x2f (Back) | 0x13 |
   | 0x1f (LB) | 0x15 | | 0x14 (Start) | 0x12 |
2. **FUN_10118260** (the generic list window) handles cursor (1–4), confirm (5/0x1d), cancel (6), 9 and 0x14 itself. **Every event is then passed to the window's handler object, `vtable+0x18` (OnKey(event, cursor))**.
3. Window names come from the registry table at **0x103722d8** (370 entries `{name[16], ..., handler global @+0x20}`; `FUN_1015dc70`). Opening a window by name is `FUN_1015e350(name,1,0)`.
   I mapped name → global → constructor → vtable → OnKey (`tools/name_classes.py`, `menu_classes.json`). Some constructors install several vtables, so a few names are ambiguous; the table marks those [I].
4. Scanning all 239 OnKey handlers for the pad events above (`tools/onkey_scan.py`): **only 18 windows react to them**. Every other window ignores LT/RT/Y/X/LB/RB/Back/Start, and the generic list code only uses cursor, confirm and cancel. [C]

## Requested ids

| id | button | where tested | window / place | what it does | conf. |
|---|---|---|---|---|---|
| **0x1e** | RB (slot 12) | FUN_101ab820 → event 0x11 | **software keyboard** (`keypad`, `keypadus`; OnKey FUN_101af190) | Sends Enter (DIK 0x1c). In the alternate keyboard mode it types a space. | C |
| | | | `keyselec` / `keypad2` / `keypad3` (FUN_10191c60, FUN_10192f60, FUN_101924e0) | Commit (FUN_10190c70(8)) | C (meaning I) |
| | | | `msgline` (FUN_101ff3e0) | Commit the line and send it (FUN_101feec0(1), then the chat send path) | C/I |
| | | | `comment` (FUN_101dde60, the bazaar/search comment editor) | Commit the current line, move to the next of 3 lines | C |
| | | | `fep` (text input frame, FUN_1018f080) | Acts as confirm (re-dispatches as event 5) when no IME is active | C |
| | | | `link11`, `scresult`, `mcres20`, `mapframe`, dialog FUN_10206780 | Commit/OK in that window. FUN_10206780 runs its callback and closes. | C (FUN_10206780), I (others) |
| **0x24** | RB (slot 12) | FUN_101ab820 → event 0x0e | no window OnKey handles event 0x0e | nothing inside windows | C |
| | | **FUN_101579c0** (field, every frame) | field, no window focused (`FUN_1015dce0()==0`) | **Opens the chat input line**: chat object vtbl[0] = FUN_101b13f0 opens `menu    fep` | C |
| | | **FUN_101d0c70** | field while **hide-UI mode** is on (flag `+0xb4` of window mgr 0x10621838, toggled by key fn 0x56 = Back/ScrollLock, cleared by cancel 0x0f) | **Takes a screenshot** (FUN_101d0ab0 → FUN_101d0ea0 writes a `%s.jpg`). Also fires while RT (0x8a) is held after a shot. | C |
| **0x1f** | LB (slot 8) | FUN_101ab820 → event 0x15 | software keyboard (`keypad*`) | Backspace (DIK 0x0e) | C |
| | | | `keyselec`/`keypad2/3` | FUN_10190c70(0xc), backspace-like | C (meaning I) |
| **0x12** | LB (slot 8) | — | **not tested anywhere** (no constant call to FUN_10123a70, not in the field table 0x10371ee0) | none in this build | C (by search) |
| **0x15** | LB (slot 8) | — | not tested anywhere | none | C (by search) |
| **0x25** | X (slot 1) | event 0x0f | software keyboard (`keypad*`, FUN_101af190) | FUN_101afbb0 (switch keyboard layout/page), then text cmd 2 (Tab) | C (meaning I) |
| | | | `keypad2` variant (FUN_101924e0) | same family | I |
| **0x35** | X (slot 1) / LShift, **held** (mode 4) | text-input objects: FUN_101b2810, FUN_1018f080, FUN_10190c70, FUN_10191c60, FUN_101924e0, FUN_10192f60, FUN_101af190 | any text box (chat line, keypad, fep) | **Shift modifier**: Left/Right become Shift+Left/Right (text selection) | C |
| **0x26** | Back (slot 16) | event 0x10 | no OnKey handles 0x10 | nothing in windows | C |
| **0x2f** | Back (slot 16) | event 0x13 | software keyboard | Text cmd 8 = Esc: cancels input and the IME (FUN_101b29f0(0x1b), ImmNotifyIME CPS_CANCEL) | C |
| | | | `keyselec`/`keypad2/3` | FUN_10190c70(0xd) (close) | C (meaning I) |
| **0x85** | RT (slot 13) | event 0x0b | text windows (`msgline`, `comment`, `fep`, `macro`, `faqmain`) | Text cmd 7 = caret right (DIK 0xcd) | C |
| | | | `keypad*` | Right arrow key into the text | C |
| | | | `keyselec` | FUN_10190c70(0x12) | C |
| **0x86** | LT (slot 9) | event 0x0a | same windows | Text cmd 6 = caret left (DIK 0xcb) / Left arrow | C |
| **0xb5** | LT+RT held + Right | asm 0x101db814 (item-list class, vtable ~0x10338cc0, globals `DAT_10663468` = `bank`, `DAT_10663740`) | **item / storage lists** | **Next bag** (FUN_101dbb20 → FUN_101dba30). Wraps through the bag table 0x1037ef98 = 0,1,9,2,4,5,6,7,8,10..16 and skips unavailable bags. Not active while list kind 0x11. If 0xb5 isn't held and 0xa8 isn't held, moves the cursor instead. | C (bag names I) |
| **0xb6** | LT+RT held + Left | asm 0x101db77f | same | **Previous bag** (FUN_101dbba0) | C |
| **0x22** | Y (slot 2) | event 0x0c | text windows | Text cmd 3 = IME Convert (DIK 0x79), or Tab (auto-translate) when the IME is off | C |
| | | | `keypad*` | Convert / predictive (cmd 3), or confirm when no IME | C |
| | | | **`chmkname`** (character creation name, FUN_10238500) | **Random name** (rand + FUN_102386b0 builds a name) | C |
| | | | `macro` (FUN_101a5690), `comment`, `msgline` | FUN_101c6920(3): forward cmd 3 to the active text box | C |

Text-input command codes (chat object vtable 0x10337a98, slot 2 = **FUN_101b2810**) [C]:
1 = submit (Enter / commit IME), 2 = Tab, 3 = Convert (0x79) or Tab, 4/5 = Shift+Left/Right, 6/7 = Left/Right, 8 = Esc + cancel IME.

Other field uses found on the way [C]:
- **0x13 / 0x14 (Start, slot 15)**: in the field (no chat, not hide-UI, no event) opens a yes/no prompt; **Yes = `/logout`** (FUN_101609f0 → callback FUN_101608d0).
- **0xb7 (slot 32)**: same kind of prompt, **Yes = `/shutdown`** (FUN_10160960 → FUN_101608b0).
- **0x56 (Back, slot 16 / ScrollLock)**: toggles hide-UI mode (FUN_1015ef60 → FUN_10160710). Cancel (0x0f) leaves it.

## Field D-pad and targeting (table 0x10371ee0, FUN_101585f0 → FUN_101587b0 → FUN_101587f0) [C]

Field command table entries `{ctx 0x3b/0x3f, key fn, mode, command}`:

| key fn | mode | cmd | action |
|---|---|---|---|
| 0x27 Up, 0x28 Down (repeat) | 0 | 2 | **Step through party/alliance**: FUN_10158b60 → FUN_10203920 moves index `DAT_106643dc` over **18 slots (0..0x11) with wrap-around**, skipping empty ones (FUN_102034e0). Up = −1, Down = +1. Alliance slots only if the action allows alliance (`flags & 8`). If the action can't target self (`flags & 0x200` clear) and the step lands on self, it steps again. If nothing valid and self is allowed, it targets self. |
| 0x2a Left (repeat) | 0 | 3 | **Previous nearby target**: FUN_10158c40(0) → FUN_10083b40(current, flags) |
| 0x29 Right (repeat) | 0 | 4 | **Next nearby target**: FUN_10083d00. **No wrap** (Tab does wrap, see below). |
| 0xb0 / 0xb1 / 0xb3 / 0xb2 (LT or RT held + U/D/L/R) | 0 | 0x18–0x1b | **Moves a free ground-position cursor** [C]: FUN_10159970 builds a direction (U +z, D −z, R +x, L −x). FUN_10159a20 starts the cursor at the current target's position, moves it by `dir × config 0xc4 (cursor speed) × frame delta`, and snaps its height to the terrain (FUN_10177770). Only active in position-target mode (field `+0x58` set and `+0x59` or `+0xc4` set). Full trace: [ground_cursor.md](ground_cursor.md). |
| 0x91 (A) | 1 | 1 | Confirm the target / act. With no target it opens `menu    playermo` (or `mp_pmode`). |
| 0x92 (B) | 1 | 0x11 | Cancel targeting |
| 0x46 F1 | 1 | 6 | Target self (FUN_10081550), if allowed |
| 0x47–0x4b F2–F6 | 1 | 7–0xb | Party member 1–5 (FUN_102034a0(n)) |
| 0x53 F11 / 0x54 F12 | 1 | 0xd / 0xe | Alliance party 2 / 3 leader (FUN_102034a0(6 / 12)) |
| 0x84 Tab / 0x97 Ctrl+Tab | 1 | 0xf / 0x10 | Next / previous target **with wrap** (FUN_10158ca0) |
| 0x4d F8, **0xb8 (slot 33)** | 1 | 0x1c | **Nearest valid target** [C]: FUN_10083f20(0x10,0,1) scans entities within 50 yalms (dist² < 2500) in front of the camera (view-space depth ≥ 0). Mode 1 keeps only non-player ids (top byte ≠ 0), filtered by FUN_10083430 with type flags 0x10, and picks the closest. If none is found it falls back to the action's current filter. |
| 0x52 F10, 0xab, 0xac, 0x4e | 1 | 0x12–0x15 | other nearest-target variants |

- NPCs vs monsters [I]: left/right/Tab walk the nearby-entity list filtered by the **current action's allowed-target flags** (`field+0x60/+0x64`). Which entity types appear depends on the action, not on the button.
- The D-pad ids are context 0x3b (field). Inside windows the separate ids 0x2b–0x2e / 0x31–0x34 drive the cursor.

## Macros (FUN_101a2e80, palettes `mcr1pall`/`mcr2pall`, long forms `mcr1long`/`mcr2long`) [C unless marked]

- **LT (key fn 0x89, slot 9)**: a pressed edge toggles the **Ctrl macro palette** (`mcr1pall`, state 1). Pressing it again closes it.
  Holding it opens the palette after one tick (FUN_101039e0 changes) and marks it "opened by hold" (`+0x18`).
- **RT (0x8a, slot 13)**: the same for the **Alt palette** (`mcr2pall`, state 2). Pressing RT while the Ctrl palette is open switches to the Alt palette, and vice versa. That is what LT+RT does together; there is no third palette.
- Keyboard: holding Ctrl/Alt (0x76/0x77) opens the matching palette after a tick and closes it on release.
  Ctrl+1..0 (0x6c–0x75) and Alt+1..0 (0x62–0x6b) run macro n through FUN_101a3500 → FUN_101a3300(set, n) → FUN_101a1c60(n or n+10).
- **Palette window OnKey FUN_101a3960**: Up/Down (events 2/1) = previous/next **macro set** (FUN_101a2a70/2a50, only if `+0x28`); **A (confirm, event 5) = run the macro under the cursor** (FUN_101a3300(1, cursor−1)); B = close (FUN_101a3390). Left/Right move the cursor over the 10 macros through the generic list code.
- **Pad macro selection is cursor-based in retail.** No code maps a face button or D-pad direction straight to macro 1–10. The fixed ids 0xb0–0xb3 are used only by the field targeting table above. [C by search, I that nothing else exists]
- **0xa8** (LT or RT or Alt or Ctrl held, mode 4) at 0x101a5f7f is in the **`mcrbedit`** window (macro book edit; vtable 0x103378b8, global 0x106627b8), not the palette. While it is held a frame counter runs, and after **31 frames** (~0.5 s) the window switches its 3 rows to an alternate label set (`+0x1d=1`) [C]. The palette's long form (`mcr*long`) uses the same handler as `mcr*pall`.
  0xa8 is also checked in the item lists (0x101db7d4/0x101db879: suppresses cursor moves while LT/RT is held) and in FUN_101f6c70, 1022e710, 10231a90, 10232b40, 102342c0 (not traced).
- **0xaf** (same combo, held): one use, in **`fulllog`** (full chat log, OnKey 0x10138230 via vtable 0x10333b68). Without the modifier, Up/Down move the selected line (`+0x1c` within `+0x58`). With LT/RT held, they scroll the log (FUN_10138180/101380e0). [C/I]

## Slots 32, 33, 35, 36 and slot 10

| id | slot | context | action | conf. |
|---|---|---|---|---|
| 0xb7 | 32 | field | yes/no prompt → `/shutdown` | C |
| 0xb8 | 33 | field | same as F8: nearest valid non-player target within 50 yalms in front of the camera (field cmd 0x1c) | C |
| 0xb9 | 35 | field (FUN_101ab770) | sends **`/jump`** | C |
| 0xba | 36 | field (FUN_101ab770) | sends **`/map`** | C |
| 0x44 | 10 (L3) | field (FUN_10157fa0) | **`/heal`** (rest). On a chair/seat object it sends `/sitchair`; on type 0x2f, `/sit` | C |
| 0x30 | 10 (L3) | field (FUN_101581e0) | **Lock-on toggle**: FUN_10157660 flips `+0x5c&3` and fires field callback 0/1 (registered by FUN_10159400, LAB_100c5440/100c5410). These call the player entity's vtable+0x334 with 0 (lock: face the current target, camera follows it) or 1 (free). Needs a target and refuses to lock onto yourself. | C |
| 0x7e | 10 (L3) | map viewer (class vtable 0x1033a000, created by FUN_101d11c0; per-frame slot FUN_101ef0a0; ctx 0x26) | **Centres/pans the map on the map cursor**: FUN_101ef990(x, y) sets the scroll target to the cursor point, clamped to the map bounds | C |

## 0x87 (pressed edge of any face/shoulder button)

**Not referenced anywhere** in this build: no constant call to FUN_10123a70 and no table entry. Its tester FUN_10122190 has no other callers. [C by search]

## Also resolved

- **Config id 0xce (chat filter)** [C]: FUN_100da5e0 (called from the incoming-chat handler @0x100e392c) checks the message against `prohibited.txt`/`allowed.txt` (FUN_101bceb0) only when 0xce ≠ 0. On a match: **1 = drop the line** (handler returns without displaying it), **2 = replace the text with system message (9, 0x159)**. 0 = filter off. Values 1/2 are only kept if the word list loads.
- **Repeat timing units** [C]: the frame delta (`*(DAT_104568fc+0x28)`, returned by FUN_10014cf0) is `60 / measured fps`, averaged over 4 frames, clamped to the frame-limit setting and at most 20, so it is in **1/60 s ticks**. Pad auto-repeat therefore starts after **16 ticks = 0.267 s** and then fires every **2 ticks (~30 Hz)**, or every frame when running at 30 fps or slower.

## Still open (low impact)

- The commit actions in `link11`, `scresult`, `mcres20` and `mapframe` on RB(0x1e) were only traced to their helper calls (FUN_101e5130, FUN_1020e290, ...), not fully named.
- 0xa8's other users (FUN_101f6c70, 1022e710, 10231a90, 10232b40, 102342c0) were not traced. They are all "modifier held" checks in other windows.
