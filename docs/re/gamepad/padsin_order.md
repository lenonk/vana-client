| pos | slot (pad bit) | role | key-fn ids using it | Steven padsin000 | Steven padexsin000 |
|---|---|---|---|---|---|
| 0 | 8 (0x100) |  | 0x12, 0x12(analog), 0x15, 0x15(analog), 0x1c, 0x1c(analog), 0x1f, 0x1f(analog), 0x99 | 8 (button 8; XInput: LB) | — |
| 1 | 9 (0x200) |  | 0x16, 0x16(analog), 0x21, 0x21(analog), 0x86, 0x89, 0x9b, 0xa8, 0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xb5, 0xb6 | 9 (button 9; XInput: LT) | — |
| 2 | 14 (0x4000) |  | 0x10, 0x10(analog), 0x7d, 0x98, 0x9b, 0xa7, 0xa7(analog) | 13 (button 13; XInput: R-stick click) | — |
| 3 | 13 (0x2000) |  | 0x17, 0x17(analog), 0x20, 0x20(analog), 0x85, 0x8a, 0x9a, 0x9b, 0xa8, 0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xb5, 0xb6 | 12 (button 12; XInput: RT) | — |
| 4 | 10 (0x400) |  | 0x30, 0x30(analog), 0x44, 0x7e, 0x99, 0x9b, 0xa6 | 10 (button 10; XInput: L-stick click) | — |
| 5 | 3 (0x8) |  | 0x0f, 0x0f(analog), 0x90, 0x90(analog), 0x92, 0x94, 0x95, 0x9c | 0 (button 0; XInput: B) | — |
| 6 | 1 (0x2) |  | 0x18, 0x18(analog), 0x19, 0x19(analog), 0x25, 0x25(analog), 0x35, 0x35(analog), 0x8d | 1 (button 1; XInput: X) | — |
| 7 | 0 (0x1) |  | 0x0d, 0x0d(analog), 0x0e, 0x0e(analog), 0x8f, 0x8f(analog), 0x91, 0x93 | 3 (button 3; XInput: A) | — |
| 8 | 2 (0x4) |  | 0x11, 0x11(analog), 0x22, 0x22(analog), 0x78 | 2 (button 2; XInput: Y) | — |
| 9 | 16 (0x10000) |  | 0x1b(analog), 0x26, 0x26(analog), 0x2f, 0x2f(analog), 0x56 | 15 (button 15; XInput: Back) | — |
| 10 | 25 (0x2000000) | while held: left stick drives D-pad instead of movement |  | — | — |
| 11 | 26 (0x4000000) | while held: left stick drives camera instead of movement |  | — | — |
| 12 | 15 (0x8000) |  | 0x13, 0x13(analog), 0x14, 0x14(analog) | 14 (button 14; XInput: Start) | — |
| 13 | 23 (0x800000) | move forward (+0x253 -127) | 0x04 | -33 (axis lY inverted) | -33 (axis lY inverted) |
| 14 | 24 (0x1000000) | move back (+0x253 +127) | 0x04 | -33 (axis lY inverted) | -33 (axis lY inverted) |
| 15 | 22 (0x400000) | move left (+0x252 -127) | 0x05 | 32 (axis lX) | 32 (axis lX) |
| 16 | 21 (0x200000) | move right (+0x252 +127) | 0x05 | 32 (axis lX) | 32 (axis lX) |
| 17 | 19 (0x80000) | camera up (+0x251 -127) |  | -36 (axis lRy inverted) | -36 (axis lRy inverted) |
| 18 | 20 (0x100000) | camera down (+0x251 +127) |  | -36 (axis lRy inverted) | -36 (axis lRy inverted) |
| 19 | 18 (0x40000) | camera left (+0x250 -127) |  | 35 (axis lRx) | 35 (axis lRx) |
| 20 | 17 (0x20000) | camera right (button->+127 on +0x250) |  | 35 (axis lRx) | 35 (axis lRx) |
| 21 | 6 (0x40) | D-pad up | 0x00, 0x00(analog), 0x27, 0x27(analog), 0x2b, 0x2b(analog), 0x31, 0x31(analog), 0xb0 | 6 (button 6; XInput: DPad Up) | 6 (button 6; XInput: DPad Up) |
| 22 | 7 (0x80) | D-pad down | 0x01, 0x01(analog), 0x28, 0x28(analog), 0x2c, 0x2c(analog), 0x32, 0x32(analog), 0xb1 | 7 (button 7; XInput: DPad Down) | 7 (button 7; XInput: DPad Down) |
| 23 | 5 (0x20) | D-pad left | 0x03, 0x03(analog), 0x2a, 0x2a(analog), 0x2e, 0x2e(analog), 0x34, 0x34(analog), 0xb3, 0xb6 | 5 (button 5; XInput: DPad Left) | 5 (button 5; XInput: DPad Left) |
| 24 | 4 (0x10) | D-pad right (also +X of dpad axis 17c) | 0x02, 0x02(analog), 0x29, 0x29(analog), 0x2d, 0x2d(analog), 0x33, 0x33(analog), 0xb2, 0xb5 | 4 (button 4; XInput: DPad Right) | 4 (button 4; XInput: DPad Right) |
| 25 | 12 (0x1000) |  | 0x1a, 0x1a(analog), 0x1e, 0x1e(analog), 0x24, 0x24(analog), 0x98 | 11 (button 11; XInput: RB) | — |
| 26 | 31 (0x80000000) | edge: cycle stick mode DAT_10455c3e 0->1->2 |  | — | — |
| 27 | 32 (0x100000000 hi) |  | 0xb7 | — | — |
| 28 | 33 (0x200000000 hi) |  | 0xb8 | — | — |
| 29 | 34 (0x400000000 hi) | while held: use padexsin000 (alternate button map) |  | — | — |
| 30 | 35 (0x800000000 hi) |  | 0xb9 | — | — |
| 31 | 36 (0x1000000000 hi) |  | 0xba | — | — |
