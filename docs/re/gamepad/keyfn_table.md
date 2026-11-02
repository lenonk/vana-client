| id | ctx mask | pad tester (cb0) | cb0 addr | cb1 | analog cbs (0x1036d0d8) | default keys (0x1036d6b0) |
|---|---|---|---|---|---|---|
| 0x00 | 0xffff | slot6(pos21) [lo 0x40] | 10121db0 |  | slot6 held ->1.0 | Up |
| 0x01 | 0xffff | slot7(pos22) [lo 0x80] | 10121f90 |  | slot7 held ->1.0 | Down |
| 0x02 | 0xffff | slot4(pos24) [lo 0x10] | 10121ea0 |  | slot4 held ->1.0 | Right |
| 0x03 | 0xffff | slot5(pos23) [lo 0x20] | 101220a0 |  | slot5 held ->1.0 | Left |
| 0x04 | 0x0008 | slot23(pos13) OR slot24(pos14) | 10122450 |  | stick +0x252 (slots21/22 axis) / FUN_10122e30(1) (mouse/other) |  |
| 0x05 | 0x0008 | slot21(pos16) OR slot22(pos15) | 101226a0 |  | stick +0x253 (slots23/24 axis) / FUN_10122e30(2) (mouse/other) |  |
| 0x06 | 0x0010 |  |  |  | stick +0x250 (slots17/18 axis) / ?101232e0 |  |
| 0x07 | 0x0010 |  |  |  | stick +0x251 (slots19/20 axis) / ?10123450 |  |
| 0x08 | 0x0004 |  |  |  | dpad slot6/7 as -1/+1 |  |
| 0x09 | 0x0004 |  |  |  | dpad slot4/5 as -1/+1 |  |
| 0x0a | 0x0001 |  |  |  | dpad slot6/7 as -1/+1 |  |
| 0x0b | 0x0001 |  |  |  | dpad slot4/5 as -1/+1 |  |
| 0x0c | 0x0001 |  |  |  |  |  |
| 0x0d | 0xffff | slot0(pos7) [lo 0x1] | 10120e60 |  | slot0 held ->1.0 |  |
| 0x0e | 0xffff | slot0(pos7) [lo 0x1] | 10120e60 |  | slot0 held ->1.0 | Y[0x10:(Ctrl or Alt)&DAT_1036cf5c], Enter, NumEnter |
| 0x0f | 0xffff | slot3(pos5) [lo 0x8] | 10120f30 |  | slot3 held ->1.0 | N[0x10:(Ctrl or Alt)&DAT_1036cf5c], Esc[0x80:noCtrl/Alt] |
| 0x10 | 0x0010 | slot14(pos2) [lo 0x4000] | 10121640 |  | slot14 held ->1.0 | V[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num5[0xc0:notInChat?+noCtrl/Alt] |
| 0x11 | 0xffff | slot2(pos8) [lo 0x4] | 10120d70 |  | slot2 held ->1.0 | F[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num+[0x80:noCtrl/Alt] |
| 0x12 | 0x0004 | slot8(pos0) [lo 0x100] | 101210f0 |  | slot8 held ->1.0 |  |
| 0x13 | 0x0004 | slot15(pos12) [lo 0x8000] | 10121ca0 |  | slot15 held ->1.0 | Pause[0x80:noCtrl/Alt] |
| 0x14 | 0x0002 | slot15(pos12) [lo 0x8000] | 10121ca0 |  | slot15 held ->1.0 | Pause[0x80:noCtrl/Alt] |
| 0x15 | 0x0002 | slot8(pos0) [lo 0x100] | 101210f0 |  | slot8 held ->1.0 |  |
| 0x16 | 0x0002 | slot9(pos1) [lo 0x200] | 10121200 |  | slot9 held ->1.0 |  |
| 0x17 | 0xffff | slot13(pos3) [lo 0x2000] | 10121530 |  | slot13 held ->1.0 |  |
| 0x18 | 0x0004 | slot1(pos6) [lo 0x2] | 10121000 |  | slot1 held ->1.0 | -[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num-[0x80:noCtrl/Alt] |
| 0x19 | 0x0004 | slot1(pos6) [lo 0x2] | 10121000 |  | slot1 held ->1.0 | -[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num-[0x80:noCtrl/Alt] |
| 0x1a | 0xffff | slot12(pos25) [lo 0x1000] | 10121420 |  | slot12 held ->1.0 |  |
| 0x1b | 0x0008 |  |  |  | slot16 held ->1.0 | ][0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x1c | 0x0008 | slot8(pos0) [lo 0x100] | 101210f0 |  | slot8 held ->1.0 | Num7[0x80:noCtrl/Alt] |
| 0x1d | 0xffff |  |  |  |  |  |
| 0x1e | 0x003e | slot12(pos25) [lo 0x1000] | 10121420 |  | slot12 held ->1.0 |  |
| 0x1f | 0x003e | slot8(pos0) [lo 0x100] | 101210f0 |  | slot8 held ->1.0 |  |
| 0x20 | 0xffff | slot13(pos3) [lo 0x2000] | 10121530 |  | slot13 held ->1.0 |  |
| 0x21 | 0xffff | slot9(pos1) [lo 0x200] | 10121200 |  | slot9 held ->1.0 |  |
| 0x22 | 0x0002 | slot2(pos8) [lo 0x4] | 10120d70 |  | slot2 held ->1.0 |  |
| 0x23 | 0x0000 |  |  |  |  |  |
| 0x24 | 0x003e | slot12(pos25) [lo 0x1000] | 10121420 |  | slot12 held ->1.0 |  |
| 0x25 | 0x003e | slot1(pos6) [lo 0x2] | 10121000 |  | slot1 held ->1.0 |  |
| 0x26 | 0x003e | slot16(pos9) [lo 0x10000] | 10121860 |  | slot16 held ->1.0 |  |
| 0x27 | 0x0001 | slot6(pos21) [lo 0x40] | 10121db0 |  | slot6 held ->1.0 |  |
| 0x28 | 0x0001 | slot7(pos22) [lo 0x80] | 10121f90 |  | slot7 held ->1.0 |  |
| 0x29 | 0x0001 | slot4(pos24) [lo 0x10] | 10121ea0 |  | slot4 held ->1.0 |  |
| 0x2a | 0x0001 | slot5(pos23) [lo 0x20] | 101220a0 |  | slot5 held ->1.0 |  |
| 0x2b | 0x003e | slot6(pos21) [lo 0x40] | 10121db0 |  | slot6 held ->1.0 | Up[0x20:noShift] |
| 0x2c | 0x003e | slot7(pos22) [lo 0x80] | 10121f90 |  | slot7 held ->1.0 | Down[0x20:noShift] |
| 0x2d | 0x003e | slot4(pos24) [lo 0x10] | 10121ea0 |  | slot4 held ->1.0 | Right[0x20:noShift] |
| 0x2e | 0x003e | slot5(pos23) [lo 0x20] | 101220a0 |  | slot5 held ->1.0 | Left[0x20:noShift] |
| 0x2f | 0x003e | slot16(pos9) [lo 0x10000] | 10121860 |  | slot16 held ->1.0 |  |
| 0x30 | 0x0008 | slot10(pos4) [lo 0x400] | 10121310 |  | slot10 held ->1.0 | Num*[0xc0:notInChat?+noCtrl/Alt] |
| 0x31 | 0x0040 | slot6(pos21) [lo 0x40] | 10121db0 |  | slot6 held ->1.0 | Up[0x80:noCtrl/Alt] |
| 0x32 | 0x0040 | slot7(pos22) [lo 0x80] | 10121f90 |  | slot7 held ->1.0 | Down[0x80:noCtrl/Alt] |
| 0x33 | 0x0040 | slot4(pos24) [lo 0x10] | 10121ea0 |  | slot4 held ->1.0 | Right[0x80:noCtrl/Alt] |
| 0x34 | 0x0040 | slot5(pos23) [lo 0x20] | 101220a0 |  | slot5 held ->1.0 | Left[0x80:noCtrl/Alt] |
| 0x35 | 0x003e | slot1(pos6) [lo 0x2] | 10121000 |  | slot1 held ->1.0 | LShift[0x80:noCtrl/Alt] |
| 0x36 | 0xffff |  |  |  |  | E[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x37 | 0x003e |  |  |  |  | R[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x38 | 0x003e |  |  |  |  | T[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x39 | 0xffff |  |  |  |  | I[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3a | 0x003e |  |  |  |  | P[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3b | 0xffff |  |  |  |  | A[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3c | 0x003e |  |  |  |  | S[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3d | 0xffff |  |  |  |  | H[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3e | 0x003e |  |  |  |  | L[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x3f | 0xffff |  |  |  |  | C[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x40 | 0xffff |  |  |  |  | V[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x41 | 0xffff |  |  |  |  | M[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x42 | 0xffff |  |  |  |  | Y[0x10:(Ctrl or Alt)&DAT_1036cf5c], Enter[0x10:(Ctrl or Alt)&DAT_1036cf5c], NumEnter[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x43 | 0xffff |  |  |  |  | NumEnter[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x44 | 0x0008 | slot10(pos4) [lo 0x400] | 10121310 |  |  | Num*[0xc0:notInChat?+noCtrl/Alt] |
| 0x45 | 0x0008 | slot29(no pos) [lo 0x20000000] | 10121b90 |  |  | Z[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num/[0xc0:notInChat?+noCtrl/Alt] |
| 0x46 | 0x0001 |  |  |  |  | F1[0x80:noCtrl/Alt] |
| 0x47 | 0x0001 |  |  |  |  | F2[0x80:noCtrl/Alt] |
| 0x48 | 0x0001 |  |  |  |  | F3[0x80:noCtrl/Alt] |
| 0x49 | 0x0001 |  |  |  |  | F4[0x80:noCtrl/Alt] |
| 0x4a | 0x0001 |  |  |  |  | F5[0x80:noCtrl/Alt] |
| 0x4b | 0x0001 |  |  |  |  | F6[0x80:noCtrl/Alt] |
| 0x4c | 0x0001 |  |  |  |  | F7[0x80:noCtrl/Alt] |
| 0x4d | 0x0001 |  |  |  |  | F8[0x80:noCtrl/Alt] |
| 0x4e | 0x0001 |  |  |  |  | F9[0x80:noCtrl/Alt] |
| 0x4f | 0x0010 | slot27(no pos) [lo 0x8000000] | 10121970 |  | key fn 0xa4 held ->1.0 | .[0x10:(Ctrl or Alt)&DAT_1036cf5c], PgUp[0xc0:notInChat?+noCtrl/Alt], Num9[0xc0:notInChat?+noCtrl/Alt] |
| 0x50 | 0x0010 | slot28(no pos) [lo 0x10000000] | 10121a80 |  | key fn 0xa4 held ->1.0 | ,[0x10:(Ctrl or Alt)&DAT_1036cf5c], PgDn[0xc0:notInChat?+noCtrl/Alt], Num3[0xc0:notInChat?+noCtrl/Alt] |
| 0x51 | 0xffff |  |  |  |  | Home[0x80:noCtrl/Alt] |
| 0x52 | 0x0001 |  |  |  |  | F10[0x80:noCtrl/Alt] |
| 0x53 | 0xffff |  |  |  |  | F11[0x80:noCtrl/Alt] |
| 0x54 | 0xffff |  |  |  |  | F12[0x80:noCtrl/Alt] |
| 0x55 | 0x003e | slot30(no pos) [lo 0x40000000] | 10121750 |  |  | [[0x10:(Ctrl or Alt)&DAT_1036cf5c], SysRq[0x80:noCtrl/Alt], SysRq[0x80:noCtrl/Alt] |
| 0x56 | 0x003e | slot16(pos9) [lo 0x10000] | 10121860 |  |  | ][0x10:(Ctrl or Alt)&DAT_1036cf5c], Scroll[0x80:noCtrl/Alt] |
| 0x57 | 0xffff |  |  |  |  |  |
| 0x58 | 0xffff |  |  |  |  | 0[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x59 | 0xffff |  |  |  |  | 1[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5a | 0xffff |  |  |  |  | 2[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5b | 0xffff |  |  |  |  | 3[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5c | 0xffff |  |  |  |  | 4[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5d | 0xffff |  |  |  |  | 5[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5e | 0xffff |  |  |  |  | 6[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x5f | 0xffff |  |  |  |  | 7[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x60 | 0xffff |  |  |  |  | 8[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x61 | 0xffff |  |  |  |  | 9[0x11:Shift+(Ctrl or Alt)&DAT_1036cf5c] |
| 0x62 | 0xffff |  |  |  |  | 0[0x04:Alt] |
| 0x63 | 0xffff |  |  |  |  | 1[0x04:Alt] |
| 0x64 | 0xffff |  |  |  |  | 2[0x04:Alt] |
| 0x65 | 0xffff |  |  |  |  | 3[0x04:Alt] |
| 0x66 | 0xffff |  |  |  |  | 4[0x04:Alt] |
| 0x67 | 0xffff |  |  |  |  | 5[0x04:Alt] |
| 0x68 | 0xffff |  |  |  |  | 6[0x04:Alt] |
| 0x69 | 0xffff |  |  |  |  | 7[0x04:Alt] |
| 0x6a | 0xffff |  |  |  |  | 8[0x04:Alt] |
| 0x6b | 0xffff |  |  |  |  | 9[0x04:Alt] |
| 0x6c | 0xffff |  |  |  |  | 0[0x02:Ctrl] |
| 0x6d | 0xffff |  |  |  |  | 1[0x02:Ctrl] |
| 0x6e | 0xffff |  |  |  |  | 2[0x02:Ctrl] |
| 0x6f | 0xffff |  |  |  |  | 3[0x02:Ctrl] |
| 0x70 | 0xffff |  |  |  |  | 4[0x02:Ctrl] |
| 0x71 | 0xffff |  |  |  |  | 5[0x02:Ctrl] |
| 0x72 | 0xffff |  |  |  |  | 6[0x02:Ctrl] |
| 0x73 | 0xffff |  |  |  |  | 7[0x02:Ctrl] |
| 0x74 | 0xffff |  |  |  |  | 8[0x02:Ctrl] |
| 0x75 | 0xffff |  |  |  |  | 9[0x02:Ctrl] |
| 0x76 | 0xffff |  |  |  |  | LCtrl[0x02:Ctrl] |
| 0x77 | 0xffff |  |  |  |  | LAlt[0x04:Alt] |
| 0x78 | 0xffff | slot2(pos8) [lo 0x4] | 10120d70 |  |  | F[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num+[0x80:noCtrl/Alt] |
| 0x79 | 0x0004 |  |  |  | stick +0x252 (slots21/22 axis) / FUN_10122e30(1) (mouse/other) |  |
| 0x7a | 0x0004 |  |  |  | stick +0x253 (slots23/24 axis) / FUN_10122e30(2) (mouse/other) |  |
| 0x7b | 0x0004 |  |  |  | stick +0x250 (slots17/18 axis) / ?10123060 |  |
| 0x7c | 0x0004 |  |  |  | stick +0x251 (slots19/20 axis) / -(key fn 0xa4) |  |
| 0x7d | 0x0004 | slot14(pos2) [lo 0x4000] | 10121640 |  |  | V[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num5[0x80:noCtrl/Alt] |
| 0x7e | 0x0004 | slot10(pos4) [lo 0x400] | 10121310 |  |  | Num*[0x80:noCtrl/Alt] |
| 0x7f | 0xffff |  |  |  |  | J[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x80 | 0xffff |  |  |  |  | W[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0x81 | 0xffff |  |  |  |  |  |
| 0x82 | 0xffff |  |  |  |  |  |
| 0x83 | 0xffff |  |  |  | FUN_10122e30(5) (mouse/other) | LShift[0x80:noCtrl/Alt] |
| 0x84 | 0x0001 |  |  |  |  | Tab[0x20:noShift], Num0[0x20:noShift] |
| 0x85 | 0x003e | slot13(pos3) [lo 0x2000] | 10121530 |  |  |  |
| 0x86 | 0x003e | slot9(pos1) [lo 0x200] | 10121200 |  |  |  |
| 0x87 | 0xffff | any of 0,1,2,3,8,9,10,12,13,14,15,16 (edge only, mode 1) | 10122190 |  |  |  |
| 0x88 | 0xffff |  |  |  |  | Num/[0x80:noCtrl/Alt] |
| 0x89 | 0xffff | slot9(pos1) [lo 0x200] | 10121200 |  |  |  |
| 0x8a | 0xffff | slot13(pos3) [lo 0x2000] | 10121530 |  |  |  |
| 0x8b | 0x003e | mouse button bit0 (obj 0x104e1d4c, not pad) | 10120980 |  | const 0 |  |
| 0x8c | 0x003e | mouse button bit1 (not pad) | 10120a20 |  | const 0 |  |
| 0x8d | 0x0040 | slot1(pos6) [lo 0x2] | 10121000 |  |  | Num-[0x80:noCtrl/Alt] |
| 0x8e | 0x0040 |  |  |  |  |  |
| 0x8f | 0x0040 | slot0(pos7) [lo 0x1] | 10120e60 |  | slot0 held ->1.0 | Y[0x10:(Ctrl or Alt)&DAT_1036cf5c], Enter[0x80:noCtrl/Alt], NumEnter[0x80:noCtrl/Alt] |
| 0x90 | 0x0040 | slot3(pos5) [lo 0x8] | 10120f30 |  | slot3 held ->1.0 | N[0x10:(Ctrl or Alt)&DAT_1036cf5c], Esc[0x80:noCtrl/Alt] |
| 0x91 | 0x0001 | slot0(pos7) [lo 0x1] | 10120e60 |  |  | Y[0x10:(Ctrl or Alt)&DAT_1036cf5c], Enter[0x80:noCtrl/Alt], NumEnter[0x80:noCtrl/Alt] |
| 0x92 | 0x0001 | slot3(pos5) [lo 0x8] | 10120f30 |  |  | N[0x10:(Ctrl or Alt)&DAT_1036cf5c], Esc[0x80:noCtrl/Alt] |
| 0x93 | 0x003e | slot0(pos7) [lo 0x1] | 10120e60 |  |  | Y[0x10:(Ctrl or Alt)&DAT_1036cf5c], Enter, NumEnter |
| 0x94 | 0x003e | slot3(pos5) [lo 0x8] | 10120f30 |  |  | N[0x10:(Ctrl or Alt)&DAT_1036cf5c], Esc |
| 0x95 | 0x0001 | slot3(pos5) [lo 0x8] | 10120f30 |  |  | N[0x10:(Ctrl or Alt)&DAT_1036cf5c], Esc |
| 0x96 | 0x003e | mouse button bit2 (not pad) | 10120ad0 |  | const 0 |  |
| 0x97 | 0x0001 |  |  |  |  | Tab[0x01:Shift], Num0[0x01:Shift] |
| 0x98 | 0xffff | slot12(pos25) AND slot14(pos2) | 101234f0 |  |  | R[0x09:Shift+Ctrl+Alt] |
| 0x99 | 0xffff | slot8(pos0) AND slot10(pos4) | 101234c0 |  |  | B[0x09:Shift+Ctrl+Alt] |
| 0x9a | 0xffff | slot13(pos3) [lo 0x2000] | 10121530 |  |  | LAlt[0x04:Alt] |
| 0x9b | 0xffff | slot9(pos1) AND slot10(pos4) AND slot13(pos3) AND slot14(pos2) | 101235f0 |  |  | D[0x09:Shift+Ctrl+Alt] |
| 0x9c | 0xffff | slot3(pos5) [lo 0x8] | 10120f30 |  |  | LShift[0x01:Shift] |
| 0x9d | 0x0004 |  |  |  | stick +0x252 (slots21/22 axis) / FUN_10122e30(7) (mouse/other) |  |
| 0x9e | 0x0004 |  |  |  | stick +0x253 (slots23/24 axis) / FUN_10122e30(8) (mouse/other) |  |
| 0x9f | 0xffff |  |  | kbd scancode 0x30 (B) via FUN_10123120 (not pad) |  | B[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xa0 | 0x0008 |  |  |  |  |  |
| 0xa1 | 0x0008 |  |  |  |  |  |
| 0xa2 | 0x0008 |  |  |  |  |  |
| 0xa3 | 0x0008 |  |  |  |  |  |
| 0xa4 | 0x0004 | slot27(no pos) [lo 0x8000000] | 10121970 |  |  | .[0x10:(Ctrl or Alt)&DAT_1036cf5c], PgUp[0xc0:notInChat?+noCtrl/Alt], Num9[0xc0:notInChat?+noCtrl/Alt] |
| 0xa5 | 0x0004 | slot28(no pos) [lo 0x10000000] | 10121a80 |  |  | ,[0x10:(Ctrl or Alt)&DAT_1036cf5c], PgDn[0xc0:notInChat?+noCtrl/Alt], Num3[0xc0:notInChat?+noCtrl/Alt] |
| 0xa6 | 0xffff | slot10(pos4) [lo 0x400] | 10121310 |  |  | D |
| 0xa7 | 0x0040 | slot14(pos2) [lo 0x4000] | 10121640 |  | slot14 held ->1.0 | V[0x10:(Ctrl or Alt)&DAT_1036cf5c], Num5[0xc0:notInChat?+noCtrl/Alt] |
| 0xa8 | 0xffff | slot9(pos1) OR slot13(pos3) OR kbd Alt(held) OR kbd Ctrl(held) | 10123520 |  |  |  |
| 0xa9 | 0xffff |  |  |  |  | Q[0x80:noCtrl/Alt] |
| 0xaa | 0xffff |  |  |  |  | E[0x80:noCtrl/Alt] |
| 0xab | 0x0001 |  |  |  |  | F8[0x02:Ctrl] |
| 0xac | 0x0001 |  |  |  |  | F8[0x04:Alt] |
| 0xad | 0x003e |  |  |  |  | U[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xae | 0x003e |  |  |  |  | X[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xaf | 0x0004 | slot9(pos1) OR slot13(pos3) OR kbd Alt(held) OR kbd Ctrl(held) | 10123520 |  |  | 0x00[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb0 | 0x0001 | (slot9(pos1) OR slot13(pos3) held) AND slot6(pos21) | 101228f0 |  |  | Up[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb1 | 0x0001 | (slot9(pos1) OR slot13(pos3) held) AND slot7(pos22) | 10122920 |  |  | Down[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb2 | 0x0001 | (slot9(pos1) OR slot13(pos3) held) AND slot4(pos24) | 10122980 |  |  | Right[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb3 | 0x0001 | (slot9(pos1) OR slot13(pos3) held) AND slot5(pos23) | 10122950 |  |  | Left[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb4 | 0x003e |  |  |  |  | Q[0x10:(Ctrl or Alt)&DAT_1036cf5c] |
| 0xb5 | 0x003e | (slot9(pos1) AND slot13(pos3) held) AND slot4(pos24) | 101229b0 |  |  | Right[0x04:Alt] |
| 0xb6 | 0x003e | (slot9(pos1) AND slot13(pos3) held) AND slot5(pos23) | 101229f0 |  |  | Left[0x04:Alt] |
| 0xb7 | 0x0004 | slot32(pos27) [hi 0x1] | 10122a30 |  |  |  |
| 0xb8 | 0x0001 | slot33(pos28) [hi 0x2] | 10122b20 |  |  |  |
| 0xb9 | 0x0008 | slot35(pos30) [hi 0x8] | 10122c10 |  |  |  |
| 0xba | 0x0004 | slot36(pos31) [hi 0x10] | 10122d00 |  |  |  |
