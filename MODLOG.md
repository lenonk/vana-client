# vana-client RE journal

Confirmed facts only; each with where it was verified. Sources: LSB `~/landsandboat` (commit d90234d2d1),
xiloader (`~/ffxi/lab/xiloader`, ceb79a0), Ghidra on the installer. Decompiled output stays in `~/ffxi/lab`.

## Installer (FFXIFullSetup_US)
- RAR5 SFX, 5 volumes. Contents: `FFXISetup.exe` (MFC bootstrapper), `PlayOnline/PlayOnlineViewer.msi`,
  `FINAL_FANTASY_XI/FINAL_FANTASY_XI.msi` + `data1..7.cab`, `redist/Directx_Jun2010`.
- FFXISetup.exe (Ghidra, FUN_00402a60/b10/be0): per checked box runs `DXSETUP.exe`, `msiexec /i` POL msi,
  `msiexec /i` FFXI msi. No other logic.
- FFXI msi: WiX, 61349 files, 7 external MSZIP cabs; game dir `INSTALLDIR\SquareEnix\FINAL FANTASY XI`.
  Custom actions are UI-only plus DISM DirectPlay and an optional config-tool launch.
- WiX smart-cabbing: identical files share one stream offset in a cab (several CFFILE rows, same
  offset+size). Verified: extractor failed on overlap until handled.
- Native install verified: 61349 files, 13.37 GiB, all MsiFileHash MD5s match, ~30 s.

## Auth (xi_connect, TCP 54231)
- TLS 1.3. Server cert is X.509 v1 (webpki rejects it); no verification, as xiloader.
- JSON in, JSON out. Must send `"version": [2,2,x]`. Login reply: `result` 1, `account_id`,
  `session_hash` as a 16-int array. Verified live (account 1000 created via command 0x20).

## Lobby (xi_connect view TCP 54001, data TCP 54230)
- View packets: `u32 size, "IXFF", u32 command, 16-byte identifier` (= session hash outbound).
- Data socket: send `0xFE + hash@12` first. Server→data messages: 0x01 (5 B, wants 0xA1 account id),
  0x02 (5 B, wants 0xA2 key), 0x03 (0x148 B char list).
- Order verified live: 0x26 version (string @0x74) → 0x05 key; 0x1F → data 0x01/A1/0x03 + view 0x20;
  0x24 → 0x23 world list; 0x22/0x21 create char; 0x07 select → data 0x02/A2 → view 0x0B (map ip/port).
- Map session key = MD5(session_hash) ++ u32(key_counter + 3). key_counter = 0x05.key + 1, +1 per request
  {07,14,1F,21,22,24,28,2B}, +1 per reply {03,0B,20,23}, +1 per error reply to {22,24,28,2B}.
  Verified: zone server decrypts our traffic.
- chr_info_sub2 is 140 bytes; TC_OPERATION_MAKE GrapIDTbl @12, zone_no @28, mjob_level @29, zone_no2 @35.

## Zone (xi_map, UDP 54230)
- 28-byte header: u16 our seq, u16 last server seq seen, u32 time @8. Sub-packets: u16 (id:9 | size:7 in
  4-byte units), u16 sync (must be our current seq).
- First datagram: raw 0x00A + MD5 trailer, unencrypted. Retry it: the zone learns of the session ~0.2 s
  after the lobby's 0x0B. LoginPacketCheck = byte sum of packet[8..92].
- Later: Huffman-compressed payload (leading 0x01, LSB-first), u32 bit count (+8), MD5, then Blowfish over
  whole 8-byte blocks after the header.
- Blowfish variant: F = ((S1[b]&1)^32) + ((S3[a]&1)^32) + S2[c] + S0[d]; key bytes sign-extended in the
  key schedule; key = MD5(20-byte session key) cut at first zero byte. Matches LSB blowfish.cpp (test).
- Compression table source: FFXiMain.dll `.data` VA 0x10355aa8 (file 0x37ea8), 256 packed 9-byte records
  `{u8 sym, u32 code, u32 bits}`, sym = (i - 0x80) & 0xff. Encoder builder 0x100dcae0, tree builder
  0x100dcb90, decompressor 0x100dcc90, net init 0x100f6a30. Regenerates LSB compress.dat and
  decompress.dat byte-for-byte (`~/ffxi/lab/compress_re/gen_tables.py`); Rust test checks equality.
- FFXiMain.dll code is packed: `.text` has no raw data; LZSS stream in section `POL1` (0x1df410 bytes)
  unpacked by the stub at entry 0x10b93410, real entry 0x1030bceb. Flag byte MSB-first, 1 = literal,
  0 = `b0 b1`: offset `((b0<<8)|b1) & 0xfff` (0 ends), length `(b0>>4)+3`. Unpacked PE for Ghidra:
  `~/ffxi/lab/compress_re/unpacked/FFXiMain_unpacked.dll`.
- Zone scenery DAT = file id 100 + zone (verified zone 231 -> ROM/1/32.DAT, 2150 chunks).

## DAT files
- Index: VTABLE{n} byte per id = ROM number; FTABLE{n} u16 = dir<<7 | file -> ROM{n}/dir/file.DAT.
  102961 ids, 79226 present, none missing, no id in two ROM sets (verified).
- Chunk header 16 bytes: name[4], u32 type:7 | size_in_16B:19, 8 zero. Types seen in zones: 0x01 folder,
  0x00 end, 0x1C MZB, 0x2E MMB, 0x20 IMG, 0x2F, 0x3D, 0x05, 0x19, 0x21, 0x4A, 0x36, 0x07.
- MMB/MZB key tables: FFXiMain.dll file offsets 0x4CE60 / 0x4D188 (256 B each, MD5-checked).
- MMB: decoded payload = 16 B head (`MMB` id or u24 size; byte 4 == 2 -> 48-byte vertices), header @16
  (name[16], pieces, bounds x1 x2 y1 y2 z1 z2, first offset @60), pieces of models: texture[16],
  nverts u16, blending u16, vertices (pos, normal, BGRA colour, uv), u32 index count, u16 indices
  (+2 pad if odd). Strip unless (`MMB` id && type == 0). Zone 231: 1054 models, 0 bad indices.
- MZB: 32 B header (u24 len, version, u24 count @4, end-of-records @20), records of 100/84/92 bytes
  (name ^ 0x55, translate, rotate, scale). Matrix = T * Rz*Ry*Rx * S. All 1898 placements in zone 231
  resolve to an MMB by name.
- IMG: flag @0, name @1, width @21, height @25; 0xA1 = "1TXD"/"3TXD" (DXT1/DXT3, size @61, data @69);
  0x01/0x81/0x91 palette @57, 0xB1 palette @61; BGRA palette, 8-bit rows stored bottom-up.
- Texture alpha is PS2-style for paletted and DXT3 images (0x80 = opaque; doubled on load). DXT1 is not.
- Most scenery is opaque: texture alpha is not coverage unless the MMB name starts with '_' or the model's
  blending has bit 0x8000 (then alpha-test). Verified: N. San d'Oria lawn `san_siba` (mean alpha 14) is
  the floor, and disappeared under a blanket alpha test.
- Collision mesh (decoded MZB): table at u32 @8 (skip zeros); grid offset @table+0x10; grid is
  (byte@0xc * 10) x (byte@0xd * 10) u32 cell offsets. Cell list: u32 head with own x @bits14..22 and
  y @bits23..31 (checked for every cell in zones 100/104/230/231/243), then (matrix, geometry) offset pairs
  until 0. Matrix: 16 floats, translation at 12..14. Geometry: u32 verts, u32 normals, u32 tris,
  u16 tri count, u16 flags (bit0: no line-of-sight block); triangle = 4 u16 (v0 v1 v2 normal, & 0x3FFF).
  Same (matrix, geometry) pair repeats across cells: dedupe. Zone 231: 58970 triangles.
- Renderer is Direct3D 8 (FFXiMain imports d3d8.dll / Direct3DCreate8): fixed-function pipeline.
- World axes: x east, z north, y down (right-handed). Vertex colours are half intensity (x2 in shader).
- Rendered zone 231 from the spawn point offscreen: geometry, textures and placement correct.

## Zoning (verified 231 -> 230 live)
- RID chunk 0x36: count @0x30, 0x40-byte entries at 0x40: pos, rot, scale (oriented box), name[8] @0x24.
  'z' + 8 chars = zone line (own tag + destination tag); 4-char 'z' names are arrival spots. Positions
  match LSB data/zones/*/zone.yaml exactly.
- Client sends 0x05E {u32 tag (first 4 chars), x, y, z (wire order), u16 ActIndex, u8 exit bit, u8 mode}.
  Server: 0x00B {u8 state @4 (2 = zone change, 1 = logout), u32 ip @8, u32 port @12}; reply 0x00D with the
  old key, then key counter += 2 and reconnect with 0x00A retries (server logs the session move).

## Zone session
- New characters log in inside their intro cutscene (0x00A EventPara != 0); 0x0E7 logout is rejected
  `InEvent` until 0x05B ends it.
- After view 0x21 (create) the client goes straight in with the new character: LSB fills only its name and ids
  in the session's list ("We never _see_ the character", login_helpers.cpp createCharacter), so the select screen
  would show it as race 0 / no job / area unknown *(inf: from LSB, not traced in FFXiMain)*. Ours selects it by name
  right after the list (2026-10-22: a Mithra MNK from Windurst showed as a Hume male; the DB had race 7, mjob 2).
- LSB sends a 0-byte datagram when a session has nothing queued (map_networking.cpp send_parse: *buffsize = 0).
  Ours drops datagrams shorter than the header instead of disconnecting; a new character's first login hit it
  ("short datagram (0 bytes)") while the server had loaded it into Port Windurst fine.
- Status byte (LSB `xi::Animation`, data/enums/animation.yaml): 0 none, 1 attack, 2 despawn, 3 death,
  4 event, 5 chocobo, 33 healing (rest, also the logout countdown), 47 sit, 63..73 sitchair, 85 mount.
  POS_HEAD @0x1F in 0x00A/0x00D/0x00E.
- 0x037 (own status): BufStatus[32] @4, UniqueNo @0x24, flags0 u32 @0x28 (hpp bits 16-23, GraphSize 11-12),
  flags1 @0x2C, server_status @0x30, Flags2 @0x34, Flags3 @0x38, dead_counter1 u32 @0x3C (60 Hz; includes 6 min,
  the client forces home point below that), dead_counter2 u32 @0x40 (absolute: Earth s since the Vana'diel epoch)
  (corrected 2026-10-23; asm 0x1009d976..0x1009d9b1).
- 0x0DF (own/trust vitals): UniqueNo @4, Hp @8, Mp @0xC, Tp @0x10, ActIndex @0x14, Hpp @0x16, Mpp @0x17.
- Home point while dead: 0x01A {UniqueNo, ActIndex, u16 ActionID 0x0B, 16-byte union, StatusId 0 = accept}.
- 0x0E7 {u16 Mode, u16 Kind}: 1/1 = /logout. Server applies LEAVEGAME, which adds HEALING; disconnect after 30 s.
  Damage cancels healing.
- A character saved with hp 0 is re-killed on every 0x00A zone-in (loadDeathTimestamp -> Die), each time
  adding a KO to char_history. Retail never lets the dead walk or touch zone lines; the client must block it.
- Open: Djarum twice reached 0 HP with no mobs in the zone (Ronfaure has no spawn data on this server):
  once around a session timeout, once right after /logout in East Ronfaure (knelt, stood, died). Not
  reproduced in three tries; 0x029 battle messages now print in the chat log (see Combat), which should show the source.

## Characters (merged from the model work; renders verified for all 8 races)
- 0x29 skeleton: u16 ?, u16 count, 30-byte bones {u8 parent, u8 term, f32 quat xyzw, f32 trans xyz}
  (parent-relative). Hume M (7072): 94 bones; 3-24 and 89-93 attach points; model front = +x.
- 0x2A mesh: offsets/counts in u16 words; flags @2 (0x80 = bone refs via table, low 7 bits 1 = cloth);
  mirror u16 @4; poly @6/len @10; bone table @12/@16; weight counts @18 {n1, n2}; bone refs @24;
  vertices @30/len @34. n1 single-bone {pos, normal} 24 B then n2 two-bone 56 B (positions pre-scaled
  by weight). Bone ref: bits 0-6 bone, 7-13 mirrored bone, 14-15 axis to negate in the mirror copy.
  Poly stream tags: 0x8010 material, 0x8000 texture name, "ST" strip, "T\0" list ({u16 idx, f32 u, v}
  per corner), "SC"/"C\0" skipped, 0xFFFF end. Stored clockwise from the front; mirror swaps winding.
- 0x2B animation: {u8 ver, u8, u16 tracks, u16 frames, f32 speed}, then 84-byte tracks {i32 bone; rot,
  trans, scale index blocks, then floats}; index n -> `frames` floats at 10 + 4n; rot index bit 31 = none.
  local_rot = anim_q * rest_q; local_trans = rest_t + anim_t.
- Motion regions: xxx0 legs (skeleton file), xxx1 upper body (skeleton+1+sub info[5]), xxx2 skirt
  (skeleton+3+max(body info[9]-1, 0)). 0x45 info chunk per part file (FUN_100ceae0).
- File ids (FFXiMain .data): skeleton per race u16 @0x10353b70; part ranges per race @0x10353ad8 ->
  9 slots x 5 {first, count}; a model id walks the ranges (FUN_100d0ef0). Table: SLOT_RANGES in
  dat/character.rs. Taru F shares the Taru M skeleton; head 0 / weapon 0 are empty dummies.

## NPCs and monsters (0x00E) (merged; 1913 of 2014 LSB model ids load)
- 0x00E handler (FFXiMain ~0x1009b055, jump table 0x1009b5d4 on byte 0x30 & 7): SubKind 0 -> model id
  u16 @0x32; SubKind 1 -> u8 face, u8 race, u16 slot ids (FUN_10099710); 7 -> chocobo (race from low
  byte). Model load switch: FUN_100d0ef0.
- Model id -> file (FUN_100c3180 type 0): <1500: 1300+id; <3000: 50295+id; <3500: 96907+id; else 98239+id.
  Self-contained files (skeleton, meshes, textures, idl0/wlk0/run0). Skeleton-less files use their 0x07
  "init" scheduler (FUN_10057000: ops 0x8d..0x95 set race..sub, 0x98 sets a base model id).
- Race tables extend to 36: 29-31 children, 32-36 chocobo colours. Poly "SC" record = 2n+12 bytes.
- Model-less NPCs (empty files, door/elevator kinds) are invisible helpers: no box, name only when targeted.

## Requirements from the user (standing)
- A straight clone of the official client in look and feel first; improvements (graphics settings, UI
  overhaul) after. Everything from the decompiled client, not guessed.
- Mouse support better than retail from the start.
- Character creator like the original: models walking, running, sitting, race-specific motions, cutscenes.
- Zone changes fade out, load, fade in.

## Model size and motions (FFXiMain; merged and wired into the client)
- GraphSize = 0x00D/0x00E/0x00A u32 @0x20 bits 9-10. Scale = info[10 + GraphSize] / 100 (FUN_100212f0;
  0xFF = 1.0); info bytes copied from 0x45 chunks only where unset (first file wins). Hume/Elvaan
  95/100/105, Taru 82/87/92, Mithra 89/93/97, Galka 92/96/100; monsters 85/100/115/140.
- Client tick 1/60 s (FUN_10014000). Motion player (update 0x10019ab0): frame += ticks * speed * 0.5 *
  rate, wraps at frames; sampling clamps to the last key (it repeats the first).
- Rotation keys (FUN_10019d30 -> FUN_10018c20 -> FUN_10018d00 -> FUN_10032400): key floor(f) and the next one; if
  their dot is < 0 the next is negated, then (1 - t) a + t b. Keys flip sign freely: pugil (model 348) bone 17 does
  on nearly every frame of idl0, so a plain component lerp passed near zero there and the fins spun (fixed in
  dat/character.rs Track::sample and render/anim.rs rot_at; test npc::pugil_keys_take_the_short_way).
- Motion choice FUN_100c6b70 on status @0x1F: 1 battle stand, 2/3 "cor", 7/33 "rx1", 47 "si1", 63-83
  "ci1", 5/85 riding, fishing "fh1". Else by movement ratio s: s > 1/3 -> walk at 3s if 3s <= 1.5, else
  run at s; s > 1e-4 -> walk at 3s; else stand. Switches blend over 16 ticks (implemented: slerp/lerp of bone poses).
- Actor defaults (FUN_100c3d20 @0x100c414d): idle idl?, walk wlk?, run run?, battle btl? (? = region digit);
  engaged monsters stand in btl0 unless their scheduler overrides it (op 0x79).
- Movement ratio FUN_100c68f0: moved this frame / (ticks * SpeedBase * 0.1 / 60), x1.6 riding; smoothed
  0.25 new + 0.75 old, max 3; 0 at once below 1e-4. Speed @0x1C, SpeedBase @0x1D (x0.1 units/s).
- Our run speed comes from our own Speed byte in 0x00A (50 -> 5.0 units/s).
- Remote entities ease toward the server position at their speed; jumps over 15 units snap.

## Lobby title screen
- Expansion list = lobby 0x05 reply u32 @0x20 bitmask (LSB generateExpansionBitmask): 0x0001 base,
  0x0002 Zilart, 0x0004 Promathia, 0x0008 Aht Urhgan, 0x0010 Goddess, 0x0020 Crystalline Prophecy,
  0x0040 Moogle Kupo, 0x0080 Shantotto, 0x0100 Visions, 0x0200 Scars, 0x0400 Heroes, 0x0800 Adoulin.
  Driven by LSB settings/login.lua flags. Unknown before login (nothing listed).
- Flow: title + credentials (replaces PlayOnline) -> title menu (Select/Create/Delete/Config/Back) ->
  screens; Back logs out. Drawn from the lobby DATs and game font (client/lobby_ui.rs). Deviations from
  retail, by request: only server-enabled logos are drawn (retail dims unowned ones), and without any the
  logo art (sprite 0 cell 1, x 80..744) shifts +100 design units to centre. The credentials window has no
  retail original; it uses the style-1 window frame and the game font. Select/Create/Delete/Config are
  still egui over the title art.

## Menus / UI (FFXiMain + DATs; title and HUD rendered offscreen and checked by eye)
- PC menu DATs: US 39541 lobby/title + 39542 in-game menus (JP 39533/39532; 55745/6, 56165/6 other
  languages; 78/79 and 1/2 are the PS2-era "lobbyps2" sets, renamed lobbyps2 -> lobbywin by FUN_10118090).
  Window frame styles ("window type") are files 14..21 (FUN_10159a70 loads 14 + style), each with a
  "win00" 0x31 library of 10 pieces and its own newtex/hfr1/vfr1/corner. Names are global: a sprite's
  texture or a library may live in another loaded file (FUN_1011cd60 searches all).
- Chunk 0x31 = sprite library (format checked against all 41 libraries in 39542, ends inside padding):
  name[16], u8 ntex, ntex x name[16], u16 nsprites, per sprite u8 ncells + 61-byte cells:
  4 x (i16 x, i16 y) corners TL TR BL BR in sprite space; u16 w, h, u16 u, v (texture rect, pixels);
  u8 flip (bit0 U, bit1 V); 4 x RGBA corner colours (PS2 scale, 0x80 = 1.0); u8 blend on, then PS2
  alpha fields A, B, D of (A - B) * As + D (0 source, 1 frame, 2 zero; FUN_1011bc10 maps them to
  D3D blend states); texture name[16]. Draw (FUN_1011c590 -> FUN_1011b290): vertex = corner * scale +
  pos - 0.5, colour = cell colour * tint >> 7 per channel (FUN_1011c9c0, tint ARGB 0x80 = 1.0),
  ADDRESSU/V = WRAP. Colour bytes read as RGBA (buttons and windows come out blue; inferred, not traced).
- Chunk 0x30 = menu layout: name[16], u8 @0x10, u8 children @0x11, then elements from 0x20, each
  u16 size @0, i16 x @2, y @4, @6/@8 cursor offset, u16 w @0xa, h @0xc, i16 frame offset @0xe/@0x10,
  u8 anchor @0x13 (window); window counts @0x14 (refs) @0x15/@0x16 (string lengths), child id u16 @0x12,
  cursor links @0x15..0x1a, counts @0x1b/@0x1d/@0x1e; then refs {u16 state kind, u16 sprite index,
  library name[16]} and two strings. Kind 0 = normal sprite, 4 = disabled, 6 = cursor (anc) library.
- Menu registry: FFXiMain .data 0x10366460, 0x2c-byte entries {name[32], handler ptr, u32 flags, ...}
  (FUN_10158060); flag 0x100000 = scaled by (W/640, H/480).
- Placement (FUN_101160d0): pos = file x, y (scaled if flagged); anchor 1 adds W - 512, 2 adds H - 448,
  3 both. Layout space is 512x448 and is not scaled for HUD windows. Children: window pos + child pos.
  Frame rect = pos + (@0xe, @0x10), size w x h, drawn by FUN_1022a290 with the style library: piece 0
  background tiled 1:1 over w x h (9 alternate), edges 2/7 (24, 0 / h-5, w-48 x 5), 4/5 (0 / w-5, 24,
  5 x h-48), corners 1/3/6/8 24x24. Then the window sprite with its first cell skipped.
- UI area W x H = 0x105dc35c..0x105dc362 (x0, y0, x1, y1) = UI resolution DAT_104441b0 / DAT_10444178,
  falling back to the backbuffer size (FUN_10010080; a separate render target when they differ).
- Title (loby1win draw FUN_1023a9c0, loby2win draw FUN_10235810, init FUN_10235620): 1024x768 design
  stretched by (W/1024, H/768). Clear colour 0x808080 with no zone (FUN_10183ff0(0)). lobbywin sprite 0
  is the art (titlwin 1024x1024 texture; first cell is a newtex wash drawn 2x full screen at 0x40808080),
  sprite 1 the same laid out for 1280x720 (no caller found yet). Buttons = loby2win children 1..5, sprites 126/128/130/
  132/183 (Select, Create, Delete, Back, Config; on screen Config is above Back), selected tint
  0x80805020 and -1 px; disabled variants +1. Captions are baked into the sprites as keytophd glyph cells.
  Enable flags at object +0x20 + i * 4 (FUN_102333b0 on the character list): Select and Delete need a
  character, Create a free slot; Config and Back always on. The cursor still lands on disabled buttons;
  confirming one (Create excepted) plays the error sound "0004" and does nothing (FUN_10235ec0 case 5).
  Logos: table 0x1037bf00, sprites 141 157 159 172 173 174 175 177 178 179 180 at (768..960, 80 + 46 n),
  owned 0xff808080 else 0x50404040. Copyright year: digit sprites 145.. (',' 155, '-' 156) from design
  x 278, y 608, "- %d" after 2003. Version text at (W - 512 + 370, H - 448 + 364) (FUN_10159920 mode 3).
  lobyhelp: frame (0, H - 69) W x 26 (anchor 2, frame offset -256, 155), help text centred at +5.
  Cursor hand: anc_s sprites 0..5 at child pos + header @6/@8, frame every 3 ticks (FUN_10116c00).
- HUD (39542, unscaled): logwindo (16, 298) 366x134 anchor 2; partywin (384, 298) 112x134 anchor 3,
  rows = children at (3, 7 + 20 n); targetwi (384, 252) 112x44 anchor 3; menuwind (384, 48) 112x202
  anchor 1, items are windowps label sprites at (16, 6 + 16 n). Gauge FUN_101c1190: fill sprite at
  (+4, +1) scaled in x by the fraction with blending off, frame sprite at (x, y); HP = windowps 38/97,
  MP = 39/98. Party row (FUN_101f9560): HP gauge (+26, +7), MP gauge (+49, +17), HP number (+88, +1),
  MP number (+98, +10). Target (FUN_10149900): name (+5, +6), HP gauge (+28, +29).
- HUD text (FUN_101bf0c0 sprite arrays DAT_1061d8f0: [0] "font    fontshp", [1] windowps, [0xd] hpcol...):
  party names, numbers and the target name use the fontshp sprite font, glyph = byte - 0x20 (0x8c of
  them; 0x85 0xDA / 0x85 0xC8 -> sprites 95 / 96), each advancing its width - 1 (FUN_101bff20); numbers
  are drawn right-aligned from the last glyph (FUN_101c02c0). Names over 8 bytes = 7 + ellipsis sprite
  0x6c (FUN_101c08f0). HP number tint = hpcol sprite 3 (<25 %), 2 (<50), 1 (<75), else 0 (FUN_101f93e0).
  TP (FUN_101f9940, shown when option 0xae is set) right-aligned at row (+0x30, +10), 0x80606060, or
  0x802060d0 from 1000. Target name fitted into the window width - 13 (FUN_101c04c0).
- Not decoded yet: the main menu's arrow pages (children 13/14) and where Log Out lives in them; the
  chat log's line height and per-type colours. The client uses 16 px lines, white, /logout and /shutdown.

## Font (FFXiMain + menu DAT; text-shot renders checked by eye)
- Menu DAT per language (FUN_10250de0; first u32 of tables at FFXiMain file 0x60828 / 0x60280 / 0x600d0 /
  0x60438): ja 39532, en 39542, fr 55746, de 56166. It holds IMG "font    moji" (1024x2048 DXT3, 64 x 128
  cells of 16 px), shape chunks (0x31) "font    usgaiji" / "font    fontshp", and 16x16 "font    el*" icons.
- Font quality DAT_10444188 (FUN_10119400, table .rdata file 0xa198): 0 the DXT3 moji; 1 file 39543
  (1024x2048 PNG); 2 file 39544 (2048x4096 PNG, 32 px texels, still 16 px quads; falls back to 39543 on
  low video memory).
- Glyph index FUN_10119830: single byte -> byte - 0x20; lead bytes 0x81-0x9F, 0xE0-0xEF, 0xFA-0xFC
  (FUN_1011a3e0); pairs via FUN_10119620 (JIS rows packed: SJIS 0x8140 -> 96, 0x889F -> 1024, > 0x931F ->
  0xCB). 0x2115..0x2125 (SJIS 0xEF1F..0xEF2F) are usgaiji shapes: 8 elements, green/red auto-translate
  brackets, 6 badges.
- Advance FUN_1011a1f0: i8 table of 0x440 at FFXiMain file 0x42bb0 (MD5 592a3523505144cc5dd1314f41421fca),
  beyond it 14; gaiji advance = shape x extent. Pen += trunc(advance * scale + 0.5); French pulls index
  0x87 8 px left. Quad = whole 16x16 cell at (pen - 0.5, y - 0.5) (FUN_101188b0, FVF 0x144). '\n' = 16 px,
  wrapped line 15 px (FUN_10211860).
- Draw FUN_10118d60 / FUN_10118aa0, UI state FUN_10159ae0: stage 0 MODULATE2X (vertex 0x80808080 neutral),
  alpha test GREATER 0, glyphs additive SRCBLEND = DESTBLEND = ONE. The atlases hold coverage in RGB with a
  binary alpha mask: there is no outline or shadow. Gaiji use their own PS2 blend.
- Unknown: chat colour codes 0x1E/0x1F and 0x7F xx (palette via FUN_101bcf00 -> FUN_10125fb0), auto-translate
  0xFD blocks. fontshp is the sprite font for numbers, party / target names and name plates.

## HUD details (FFXiMain; checked live as Tuxedo)
- Party window (FUN_101facc0) swaps layouts by member count instead of resizing: "menu    ptw0" outside a
  party ("Solo"), else "ptw1".."ptw6" from 0x10378d30 + n * 0x20 (frames 34..134 px high).
- Name plates: pass FUN_10080100 (task "CXiActorNameDraw") sets ZWRITE on, fog off, alpha test > 0,
  SRCALPHA / INVSRCALPHA, stage 0 colour MODULATE2X, alpha MODULATE4X, then calls actor vtable +0x284
  (FUN_100ce4d0): position from +0x278 (FUN_100ce480: a model point via vtable +0xb4 / +0x1c4, plus height
  +0x8c0, default 0), text +0xcc via FUN_10084b20. That draws fontshp glyphs (FUN_1011ce20) as
  pre-transformed quads at the projected depth, size = screen (W, H) * 0.00234375 * scale, centred, each
  advancing its width; '\n' + 8 px. scale (FUN_100819d0): FUN_100818c0 stores trunc(z_ndc * 4096) with the
  projection's near 0.1 / far 65535 (FUN_100146e0); scale 1 below 4020, (4100 - z) / 80 up to 4100, hidden
  beyond. In view depth d: 1 up to d = 5.39, then about 0.05 + 5.12 / d (6 → 0.91, 10 → 0.56, 50 → 0.16),
  so plates do shrink with distance; no distance cut-off. All 7 actor vtables share FUN_100ce480 at +0x278.
  Quads are XYZRHW (FVF 0x144, rhw 1); anchors up to 100 px off screen still draw. Matches retail video frames. Colour = actor +0x78, chosen by FUN_1008db40 (table filled by FUN_1011c8b0; see "Name colours: the rule")
  from "menu    ncol" (23 sprites, first-cell colour): 0 white (players), 1 cyan (party), 4 green (NPCs),
  5 pale yellow (monsters), 6 red (claimed by us / party), 7 magenta (claimed by others), 8 orange, 9 grey,
  0x12..0x16 team colours. The monster flag is @0x20 bit 0 (actor +0x120 bit 13); @0x25 is the hitbox size (+0x208 = @0x25 * 0.1), not a
  monster mark (corrected 2026-10-23). The claimer is at @0x2C.
- Main menu pages: menuwind (FUN_1013b3f0) and socialme (FUN_10147fc0) open each other on left / right
  (events 3/4/7/0x1c) or children 13..15, keeping the row (pair table 0x10364220). Social children: 1
  Missions, 2 Quests, 3 Key Items, 4 View House, 5 Bazaar, 6 Macros, 7 Config, 8 Help Desk, 9 Communication,
  10 Current Time, 11 Shut Down (FUN_10159f90), 12 Log Out (FUN_1015a020). Both open the yes/no box
  FUN_102075f0 with group 0xe message 0x48 "Log out?" / 0x277 "Shut down?" (0x49 / 0x278 to cancel one in
  progress). A menu item's first string is its help message id in group 0xe (Map "40" = "View map of current area.").
- Messages (thunk_FUN_1014c660(group, id) -> FUN_1014c660): group table 0x10365f50 {group, object}; FUN_1014d350
  loads each object from FUN_10250e50(slot) = per-language file table (en at FFXiMain file 0x60280). Group 0xe =
  slot 0xc = file 55651 (ROM/165/75). Groups 8 and 10 are XISTRING files 48 / 50 (lobby strings). d_msg: header
  0x40, table size @0x1c, count @0x28, XOR 0xFF when @0x2c = 1; table of (offset, length); an entry is a field
  count, (offset, kind) pairs, and the text 0x1c bytes into the field. XISTRING v2: count @0x24, 12-byte
  entries from 0x38. Reader: dat/msg.rs.
- Zoning text: no in-game code shows a loading message (the only "Downloading %s..." is lobby group 8 id 0x58),
  so zoning is a plain fade to black. WRONG: keytops3 165..169 is the indicator (see the Downloading data section).
- Chat log (FUN_1015b660): lines 16 px apart, newest at the bottom (text area bottom - 16), x + 4. Per-line
  colours come from each line's record (configurable palette); not decoded, drawn white.

## Character creator preview: the "dancer" engine (FFXiMain; all 8 races x 8 faces x 2 hairs rendered, checked by eye)
- The creator preview is not the in-game character system. It is Square's sq* middleware ("dancer",
  D:\build0001\FFXi_Win\Main\dancer\St*.cpp; modules sqmd model, sqsh shape, sqsk skeleton, sqin skin,
  sqmo motion, sqsd shader, sqop trigger), with its own 11 MB pool (_dancerInit FUN_10247370) and D3D path.
  Scene object DAT_10623bc8; controller [0x10623e6c] (ctor FUN_10246db0, update FUN_10246df0, per-frame
  draw FUN_10246ee0(1) from the main loop @0x10010fcf -> FUN_10243450 update, FUN_10245ab0 draw).
- Files per creator race r (0 HumeM 1 HumeF 2 ElvM 3 ElvF 4 TaruM 5 TaruF 6 Mithra 7 Galka):
  model group 0x1037ce80[r] = 31184 31164 31124 31104 31244 31224 31204 31144 (20 ids): +0/+1 low-poly
  body DMB/DMP ("RT<race>BJ", flag-0x20 path only), +2/+3 body ("RT<race>BS", the starter outfit),
  +4+2f/+5+2f head for face f ("RT<race>HA".."HH"). Motion group 0x1037cea0[r] = 31496 31438 31322
  31264 31670 31612 31554 31380 (58 ids): +0 FOV / +1 camera matrix (camera 0), +2/+3 camera 1, +4..+7
  body FrameChannel loops (back-walk, run, idle, walk), +8 body performance, +9 body idle loop,
  per face f: +0xa+6f..+0xd+6f face loops, +0xe+6f face performance, +0xf+6f face idle loop.
  31728+r "OC:01.00" and 31736+r "SeSep" (rthu rthu rtel rtel rtta rtta rtmi rtga) = trigger and sound
  tracks for the performance (FUN_10242d60 -> FUN_102487f0). Load: FUN_10242a70 (race/face),
  FUN_10242d90 (body channels), FUN_10242e30 (face channels), FUN_10247f00 (DMB id, DMP id + 1).
- DMB (sqmdDMBResolve FUN_1027d8a0): "DMB\0", model header @0x10 (0xAC bytes): counts/capacities
  textures @0x24/0x28, shaders 0x34/0x38, updaters 0x44/0x48, shapes 0x58/0x5c, aux 0x68/0x6c,
  tags 0x7c/0x80, files 0x90/0x94, scene 0xa4. Then, 16-aligned, u32 arrays sized max(count, cap):
  textures+flags, shaders+flags, updaters+updaterFlags, shapes+matrices+shapeFlags, aux+flags,
  tags+tagTypes+tagIndices, files+fileTypes+fileIndices, shaderIndices+shapeIndices, sceneIndices.
  0xEFBEDDBA ("baddbeef") marks a slot filled at load. Then (each 16-aligned) texture records, shader
  records, 0x40 per placeholder matrix, tag strings then file strings (NUL, 4-aligned). Every one of the
  160 creator DMBs ends exactly at the file size.
- Texture record (FUN_1027dfc0): 0x24 head (+0x1c -1 = no image), image head to +0x60: width @0x40,
  height @0x44, bytes/pixel @0x48 (4 RGBA, 3 RGB, 1 index or luminance), data @0x4c (baddbeef = inline
  w*h*bpp follows, 16-aligned; else index of a texture to borrow), CLUT entry size @0x54, count @0x58,
  CLUT @0x5c (-1 none). Bytes are R,G,B(,A) top-down (FUN_10276e00, D3D formats 0x15/0x16/0x33/0x29).
- Tag types: 0 texture, 1 material (shader), 2 skeleton, 3 shape. Shader kinds (FUN_1027e240):
  0xF0001 sqsdGeneric (0x60 words, texture links 0x13..0x19 and 0x2b/0x2c: base 0x13, sphere/"pho"
  map 0x14), 0xF0002 sqsdSampling (0x5b words, base 0x14, "pho" 0x15; 16-entry colour ramp follows;
  faces), 0xF0005 sqsdAniso (0x1b words, links 0x18/0x19; hair). Shape i uses shader shaderIndices[i].
- DMP: blocks of {u32 flags, u32 size, name[16], ...} + 0x40 head, data, 16-aligned. First the shapes
  (flags & 4), then the updaters in order: updaterFlags 0xEDFEEDFE = skeleton, 0xBC00ssss = skin for
  shape ssss (skeleton index in bits 16..), other values = "_sort" updaters with no block (FUN_1027e8e0).
- SQLE object: "SQLE", u32 flags (bit 0 = byte-swap), u32 kind, u32 version, comment[0x50], data @0x60.
  TriStrip 0xA0009 v2 (FUN_1028f520): u32 nverts, u32 nattrs, per attr {u32 id, u32 comps, floats}
  (0 position, 1 normal, 2 uv), u32 ncodes, codes, (v2) u32. Codes (FUN_102a17b0): codes[0] = runs;
  run n > 0 = n/3 listed triangles; n <= 0 = strip of -n indices. Triangle counts match the comments.
- Skeleton 0xB0001 v4 (FUN_10290810): u32 bones @0x60, 0x40 each: t xyz, q xyzw, s xyz, i32 channel
  counts (trans 0/3, rot 0/4, scale 0/3), 2 ints, parent. Local = T * R * S, parent-relative; model
  space y up, front +z. Body roots at Jhip (89 bones HumeM); the head file has its own skeleton rooted
  at NJspineC and is posed by its own channels (no attachment).
- Skin 0x150001 v2 (FUN_102904c0): u32 clusters @0x60, u32 idMax, per cluster {joint, n, n vertex ids,
  n weights}; weights sum to 1 per vertex. Vertex = sum w * world(joint) * inverse(bind world(joint)) * v.
- Channels (sqmoChannelBuildSQO FUN_1027a930): 0x50001 FrameChannel v4 (FUN_1027aab0): f32 time,
  n, n types, frames, frames*n raw floats. 0x50003 PlaybackChannel v3 (FUN_1027ace0): f32 time, n,
  frames, f32 step, packed; packed: per channel {u32 len, u32 bits, f32 step, f32 initial, len bytes},
  then n types. value(f) = initial + step * sum of deltas 0..f-1 (FUN_102886a0); deltas sign-magnitude:
  2 bits MSB-first, 4-bit high nibble first, 8 bit, 16 bit big-endian (FUN_10288db0, LAB_10288e30/e80/
  eb0). Types: 0 scalar, 1 quaternion (4 floats, slerp), 2 scale (FUN_10272ac0). A skeleton frame is
  read bone by bone using the bone's channel counts (sum = channel width: HumeM body 299, head 488).
  Time in frames at 30/s (2387 frames = 79.53 s for HumeM). All 464 creator channel files parse to EOF.
- Cameras: camera k = FOV channel (1 float, degrees; full angle, used as tan(fov*pi/360) by
  FUN_10279500; taken as vertical) + 16-float camera-to-world matrix channel (row-major, -z forward,
  y up). Camera 0 for HumeM: FOV 37.85, from (9.6, 3.8, -87.5) behind the character to a face close-up at
  (15.3, 14.5, 461) by frame 2200. The performance walks the character ~480 units along +z.
- View modes (scene +0x1c): 0 camera 0 + performance, 1 camera 1 + performance, 2 idle loop (+9 body,
  +0xf face) with an orbit camera; input group 0x3f id 0x10 cycles 0 -> 1 -> 2 -> 0 (FUN_10243450).
  Mode 2: keys 0x3f/0x4f, 0x50 zoom +-8, axes 0x3f/6, 7 orbit (FUN_10243600 -> FUN_10267520); preset
  from 0x1037ced0/cef0/cf10[r] by the race/face scale 0x1037cc80[r*8+f] (==1, >1, <1) into 13 x 9-float
  presets @0x103a5528 (FUN_10267660). Modes 3/4 (stick-walk with the +4..+7 loops at speeds
  0x1037cd80[r*4+k], hume 0.235 0.637 0 0.202) are only on the flag-0x10 path, which nothing sets.
- The creator starts mode 0 (FUN_10247040(0)), area environment 0x85, weather 'suny', hour 11
  (FUN_1022ef00 @0x1022f11f). There is no automatic turntable.
- Menus driving it: race hover FUN_10246fa0(race, face), face hover FUN_10246fe0(face), hair hover
  FUN_10247070(hair) -> FUN_10266130(hair + 1): every shape on material "<x>CHG<n>MT" switches to
  "<x>CHG<hair+1>MT" (hair A/B = other face/hair texture, e.g. face1TX/face2TX). Size and job never
  touch the preview: FUN_102460b0 stores size at scene+0x18 and scales by 0x1037cc80[race*8+face] only.
- BGM per race 0x1037cec0 = 0x85 0xa8 0x84 0x83 0xa7 0x82 0xa6 0xa5 (FUN_10036b40 at load,
  FUN_10036a90 again when the performance wraps, FUN_10245920: performance time < saved +0x4c). A race
  change reloads everything (FUN_10242a70) and sets +0x4c = 100000.0, so the music starts at once; a face
  change (FUN_10242eb0 -> FUN_10242f00) swaps only the face and keeps the dance and music going. File sound/win/music/data/musicNNN.bgw
  (format "%s\music%3.3u.bgw" FUN_1023bed0; id -> NNN inferred). Nation step plays BGM 0x78.
- Lights (FUN_10242a20 / FUN_10245bc0): 5 lights {struct 0x1037cc54[i], node 0x1037cc68[i]}, kind
  0x70002, colour x1.5 on the normal path: 0.6, 0.3, 0.1, 0.3 grey; node quats (0.357 -0.264 0.595 -0.668),
  (0.515 0.239 -0.690 -0.449), (-0.187 0.155 -0.663 -0.706), (-0.002 0.002 0.855 0.517); light 4 = the
  zone environment colour (FUN_1017ff50) / 4, rotation euler (0, pi/2, 1.2566). The dancer clears only
  depth/stencil (Clear flags 6), so whatever the lobby drew stays behind the character.
- In-game GraphSize is unchanged (see Model size): size only matters after creation.

## Dancer shading (creator preview), traced to the pixel (creator-shot renders it; before/after checked by eye)
- Pipeline: D3D8 fixed function, CPU texgen, multipass. A shader's apply function records passes
  (FUN_10297810 -> Generic FUN_10286450, Sampling FUN_1028e980, Aniso FUN_1028eca0); per-vertex texgen
  and lighting run on the CPU in FUN_102808d0; FUN_10279860(+1) raises D3DRS_ZBIAS per extra pass.
  Blending is in gamma space (no sRGB). Stage-0 combine (FUN_102776a0, 0x8c0 set by FUN_10277540):
  0 = colour TEXTURE x DIFFUSE, alpha TEXTURE; 1 = TEXTURE only; address WRAP unless "clamp".
- Blend setups (stage-0 path, FUN_10277fb0..10278a00): Add = ONE/ONE (10277fb0, 102780c0, 10278320);
  Alpha = SRCALPHA/INVSRCALPHA (10278480); DstAlphaAdd = DESTALPHA/ONE (102786b0); ZERO/SRCALPHA
  (102788e0); ZERO/SRCCOLOR (10278a00); REVSUBTRACT ONE/ONE (10278200); off (10278b10). The strength
  arguments (envStrength, phongStrength) are only used by the multi-stage path, which these shaders never
  take, so highlights are added at full strength. COLORWRITEENABLE via FUN_10277db0 (nonzero arg = masked).
- Shader words (field names from the dump FUN_10281e30/FUN_10298ce0). Material (all kinds): colour
  0x0a-0x0c, ambient 0x0d, diffuse 0x0e, specular 0x0f, shininess 0x10, transparency 0x11.
  Generic 0xF0001: textures colour 0x13, env 0x14, bump 0x15, phong 0x16, phongMask 0x17, refract 0x19,
  ambient maps 0x2b/0x2c (count 0x2d); envStrength 0x1b, bumpStrength 0x1c, phongStrength 0x1d,
  refractStrength 0x1e, refractLevel 0x1f, phongSize 0x20, flags 0x21 (bit0 uvPersp, bit1 edgeAA),
  cull 0x22, visible 0x23, decal 0x24, clamp 0x25, useLights 0x26, twoSide 0x27, useAlphaMask 0x28,
  useAlphaTest 0x29, useVertexColors 0x2a. Sampling 0xF0002: cull 0x13, colour 0x14, phong 0x15, back 0x16,
  16 ramp colours RGBA 0x17.. (stride 4), phongSize 0x57, phongStrength 0x58, useAlphaMask 0x59, clamp 0x5a.
  Aniso 0xF0005: cull 0x13, visible 0x14, useAlphaTest 0x15, mode 0x16, colour 0x18, aniso texture 0x19.
- Lighting (FUN_10278d10, FUN_10279d00): material diffuse = kd x colour (alpha 1 - transparency),
  emissive = ka x colour, material ambient 0, specular off. Up to 4 directional lights (bank 0), colour
  0x794+12i, unit vector to the light 0x34c+0x40i = row 2 of the light node matrix (transposed by
  FUN_10274960) = node rotation x z. D3D: rgb = clamp(emissive + sum diffuse x Lc_i x max(N.L_i, 0)).
- Creator light rig: 4 directional lights, intensity 0.9 / 0.45 / 0.15 / 0.45 (struct colour 0.6/0.3/0.1/0.3
  x1.5; light kind 0x70002, +0x30 = 1 directional; light 4 is hidden). The node rotation is
  camera x light (FUN_10268040 stores inverse(camera at attach) x light, FUN_10266820 multiplies by the
  current camera each frame; cameras start as identity, FUN_10266450 -> FUN_102637f0, before FUN_10242a20 attaches), so
  L_i = R_cam R(q_i) z. Node position = target + 3000 x L (FUN_10263ae0).
- Generic passes (FUN_10286450), each on the same triangles:
  1. base (transparency < 0.999): colour texture x lit (useLights; else white), alpha = texture alpha.
     Blend Alpha iff the colour texture has alpha (2/4 bytes per pixel or a 4-byte CLUT) and
     useAlphaMask bit 0, else no blending, so the texture alpha lands in the destination alpha.
  2. env (env texture): texture only, uv = ((r.x + 1)/2, (r.y + 1)/2), r = v - 2(v.n)n,
     v = normalize(P - eye), all in model space (FUN_10291f00). Blend DstAlphaAdd if useAlphaMask bit 2
     else Add.
  3. phong mask (texture 0x17): texture only, mesh uv, blending off, colour writes = alpha only, so
     the destination alpha becomes the mask.
  4. phong (texture 0x16): z-write off; uv = ((q.x k + 1)/2, (q.y k + 1)/2), q = r in light 0's frame
     (matrix 0x248 = world x inverse(light 0)), k = (1 - phongSize) + 0.3; vertex colour and alpha =
     (q.z >= 0 ? 1 : 0) (FUN_10292240). Combine = texture x vertex colour, or texture only after an env or
     mask pass. Blend Add if useAlphaMask bit 1 is clear and there is no mask, else DstAlphaAdd.
- Sampling (faces, FUN_1028e980): ramp lighting FUN_10291640 per vertex: s = 15 / (back + 1.0001);
  for each light x = max(0, s(N.L + back)), i = floor x, f = x - i,
  c += lerp(ramp[i], ramp[i+1], f) x Lc; rgb = clamp(c). Base = colour texture x c (alpha = texture);
  Alpha blend iff texture alpha and useAlphaMask bit 0. Phong pass if the phong texture exists and
  phongStrength > 0.001: same texgen (k from phongSize 0x57), z-write off, DstAlphaAdd if useAlphaMask
  bit 1 else Add.
- Aniso (hair, FUN_1028eca0 + FUN_10291d70): one pass, two stages (FUN_10299060 = stage 1).
  Vertex colour is the diffuse material source (DIFFUSEMATERIALSOURCE = COLOR1 when vertex colours and
  normals are on, FUN_10278d10): vc = (kd x colour, max(N.L0, 0)); lit = clamp(colour x ka + sum vc x Lc_i
  x max(N.L_i, 0)), lit.a = vc.a. Stage 0 = colour texture x lit, alpha texture. Stage 1 MULTIPLYADD:
  rgb += lit.a x aniso(uv1) (ARG1 = DIFFUSE|ALPHAREPLICATE, ARG2 = TEXTURE). uv1 = (uv.u, V.B - L0.B)
  for mode 0/2 (B = dP/dv), (uv.v, V.T - L0.T) for mode 1 (T = dP/du) (FUN_10291d50, 0xe24).
  Tangents: FUN_102a06d0, per-corner sums over the triangles (degenerate |det| < 1e-6 -> 1e-6), then
  Gram-Schmidt against the normal and normalized. Blend Alpha iff the colour texture has alpha; alpha
  test GREATEREQUAL 0x5f/255 iff useAlphaTest (FUN_10277ce0).
- Cut-out and blend rules, traced: no generic or sampling material is alpha-tested (useAlphaTest 0x29 is
  stored but no consumer was found in the apply path); only Aniso uses the alpha test. CULLMODE from cull
  bits 2 (CCW) and 4 (CW), else none; cull 1 (every creator material) means frustum test only, no face
  culling. "_sort" updaters (updaterFlags >> 8 = shape) mark eyelashes and hair; the prototype draws
  those triangles back to front (the sort updater FUN_10290c90 is not traced).
- 1-byte textures without a CLUT are P8 with the grey palette (i, i, i, i) (FUN_10276e00), so alpha =
  luminance; that is what makes the "phmk" phong masks work. Where P8 is unsupported the code uses L8
  (alpha 1) and the masks stop working (_dancerInit: 0x2d0 = 1 on ATI 0x1002 or no palette support).
- Usage across the 72 creator models: Generic with no extra map, with env (useAlphaMask 4/6), with phong
  (2), with phong + mask (1); Sampling always with phong (useAlphaMask 2); Aniso modes 1 and 2. No bump,
  refract or ambient maps.
- Creator foley (31728+r "OC:01.00", sqopSystemBuild FUN_1027f210): u32 1, u32 -1, u32 tracks, u32 cap;
  per track u32 -1, u32 count, u32 cap, then {u32 type, u32 frame, u32 value} events. Track 0 follows the
  body performance frame, track 1 the idle loop, tracks 2+ the walk loops (FUN_102457b0,
  FUN_10248790). Type 0 plays the type-0x3d "SeSep" resource named "%04d" from 31736+r (e.g. "4017" =
  se id 0x5dd1 -> sound/win/se/se024/se024017.spw) at the hip bone position (FUN_10036200, distances 3,
  20, 512, 64); "4056" plays unpositioned (FUN_100360e0). Types 1 and 2 add to a counter at
  DAT_10443718+0xdb0 (type 2 only when the value is > 0x7f); its purpose is unknown.

## Zone lighting, sky and fog (FFXiMain; La Theine by hour, title flyover and creator backdrop rendered)
- Data: in the zone scenery DAT (100 + zone), folder `<zone>/weat/<weather tag>`; each key is a type-0x2f
  chunk named HHMM (Vana'diel time, FUN_100711d0). Tags in La Theine (DAT 202): suny fine clod mist rain
  wind; keys like 0000 0500 0600 1200 1700 1800 2100. A missing weather falls back to 'suny'
  (FUN_10183780). 39831 + zone (0x9b97 + zone) are bump maps (chunk 0x5d), 0x1b8c the shadow texture.
- Interpolation FUN_10183320: previous key at or before now (FUN_10182e10) and the next one
  (FUN_101830d0), day = 5184000 units (24 x 216000); t = (now - prev)/(next - prev); every field lerps
  (FUN_10182470, colours per byte). Time source DAT_106175c0 (W+0x1c8 when cam+9 is set, FUN_10182c40).
- 0x2f payload (0xac bytes at res+0x30, FUN_10183740): u8 flags @0 (bit 0: the 0x10/0x30 fields are signed
  byte light directions, not moon colours); actor group @0x0c; background group @0x2c; clear colour @0x4c
  (FUN_1003cd20 -> cam+0x308); floats @0x50/0x54 (cam+0x318/0x314, unknown); scenery draw distance @0x58
  (FUN_101848d0); bytes @0x5c..0x5e (0x5e = dome segments, dome drawn if > 2); float @0x68 (unknown);
  8 sky ring colours @0x6c (horizon to zenith); 8 ring elevations @0x8c (fraction of 90 degrees).
  Group (0x20 bytes): sun diffuse, moon diffuse, ambient, fog colour (B,G,R bytes / 255, FUN_10181170),
  f32 fog end @0x10, fog start @0x14, light scale @0x18 (1.21..1.61 seen).
- Sun/moon FUN_10180700: a = time / 5184000 x 2 pi; moon = (sin a, cos a, 0), sun = -moon; sun used when
  sun.y >= 0 (06:00-18:00, y down) else moon with the moon colour; optional zone yaw W+0x1cc degrees; eased
  0.1 per frame in game (mode 0x60).
- D3D8 per frame (FUN_10174610): light 0 directional, diffuse = background sun/moon colour x light scale,
  ambient DAT_106174dc, direction W+0x14c; RS AMBIENT = background ambient (FUN_10181de0); FOGCOLOR /
  FOGEND / FOGSTART from the background group (x FUN_10182290, 1.0 by default; end <= start -> start + 1)
  (FUN_10182100). Actors use the actor group (light 1, W+0x84, W+0x94/+0x1c/+0x20). Device defaults
  FUN_10008d60: lighting and COLORVERTEX on, DIFFUSE/AMBIENT material source = COLOR1, emissive from the
  material, table fog LINEAR on view depth; stage 0 COLOROP MODULATE2X, ALPHAOP MODULATE4X.
  Zone material (0x3913c) diffuse 1.0, ambient 0.7 (replaced by the vertex colour).
- WGSL: scenery lit = clamp(vc x (ambient + sun x scale x max(N.(-L), 0))), rgb = clamp(tex x lit x 2),
  alpha = tex.a x vc.a x 4 (our textures already double alpha); fog f = clamp((end - z_view)/(end - start)),
  out = mix(fog, rgb, f). Actors: D3DRS_AMBIENT = ftol(ambient x 128) (FUN_10181de0), material ambient 1, diffuse 0.5 (DAT_10346380, FUN_1002a8c0): lit = clamp(Ga + 0.5 x sun x ndl) (MODLOG "Target flash: the ambient bytes"). Sky: e = asin(-ray.y)/(pi/2), lerp
  ring colours by elevation; clear colour when the dome is off. Dome radius (far - near) x 0.8
  (FUN_1003cd20 / FUN_1003c800).
- Title ops: 0x7e copies the title DAT's 0x2f block (DAT 23: 0620, s101, 1700) into DAT_10617500 as a
  static override (no hour lerp; clear 0x308, fog colour 0x304, end 0x300, start 0x2fc; FUN_10183ff0,
  FUN_101810a0); 0x7c builds a weather object from the zone's weat/<tag> (FUN_101804d0 -> FUN_10183780);
  0x7d sets the clock (FUN_101851f0, hour = value / 216000); 0x0e moves the screen tint cam+0x32c/0x330
  toward RGBA @8 / float @0xc over the duration @6. Op 0x73 'loop' 3: 3 is the op's wait (u16 @4); a
  script restarts at its end when it contains an op 0x01 or has the loop flag (+0x141) (FUN_10056c70), so
  the title cycle repeats. Scheduler header +0x1c = script length (mov1 9900).
- Character select actors are standard PC actors (FUN_100a9cd0, vtable 0x10324320) playing the default
  idle idl? (FUN_100c6b70 / FUN_100c3d20); no idle variation in the lobby code (by absence).

## Title flyover and live lighting (client; checked live)
- File 23 schedulers: "main" spawns "loop"; "loop" runs one random field movie of mov1..mov8 (op 0x3d .. 0x3e),
  then expansion blocks (0xaf with an expansion bit: ex1a.., ex2a.., ex3a..). Movie zones: mov1 102, mov2 103,
  mov3 104, mov4 109, mov5 106, mov6 111, mov7 126, mov8 141. client/flyover.rs plays one random movie behind
  the login screen and title menu: ops run when the previous one's wait (u16 @4) is over; camera paths for
  their duration (u16 @6); 0x0f fades multiply the scene colour toward RGBA @8 (0x80 = 1.0; starts black);
  0x10 crossfades render the outgoing camera offscreen and fade it out over the new shot. Lighting as
  flyover-shot: clock 12:30 until op 0x7d, op 0x7e title 0x2f override, op 0x7c the zone's weather keys.
  When the movie ends another random one loads. Not modelled: op 0x0e screen tint, camera roll, actors.
- In game: 0x00A GameTime @0x3C is Earth seconds since the Vana'diel epoch (LSB 1009810800 = Vana midnight);
  one Vana'diel minute = 2.4 s, minute of day = (GameTime / 2.4) mod 1440. Weather number @0x68, changes in
  0x057 (u16 @8). Number -> tag table FFXiMain 0x1036bab8 (FUN_10184ac0): fine suny clod mist dryw heat rain
  squl dust sand wind stom snow bliz thdr bolt aura ligt fogd dark (LSB xi::Weather order). The zone's
  weat keys are parsed with the zone and lit every frame (retail eases the sun 0.1 per frame; we do not).

## Select and creator screens (client; checked live)
- Select (FUN_10238ea0) opens chfwin (Options: Load/Save settings files) and dbnamese (16-slot name list,
  grid sprite 78 + additive highlight 77 in 1024x768 design space, names at slot + (4, 2)); chswin is the
  settings-file slot window, opened only from Load/Save. Help: group 8 0x69 play, 0x6a delete. Delete asks
  with 0xae "The data will be lost forever. Proceed?" (we default the cursor to No). client/select.rs: zone 133
  at 15:30, ring of 16, camera on quadratic curves (eye ends at the slot near point + (0,-1,0)).
  Not done: chfwin, the 149-frame head-point refresh (model point 7), the arc-length camera speed.
  Superseded by "Select Character screen" below: the list is unscaled at (W - 640, H - 480), the camera aims at
  point 7, chfwin and the prompts are drawn.
- Creator: steps chmkrace / chmkface / chmkhair / chmksize / chmkjobs / chmkname / chmktown (512x448 windows at
  32,32; labels are windowps sprites), help 0x6b..0x6f and 0x72; then a confirm. Hover updates the preview
  (race/face reload the dancer, hair switches CHG materials). client/creator.rs: creatorshot's traced passes
  through render::dancer::DancerRenderer over zone 133 lit at 11:00; Tab cycles view 0 -> 1 -> 2 (orbit).
  Superseded by "Creator screen" below: the backdrop camera is the dancer camera and the name help is group 0x1b id 4.
  Still inferred: the confirm text "Create this character?".

## NPC talk, event packets, zone dialogue text and the dialogue window

FFXiMain addresses. Incoming handler offsets count from the packet start, including the 4-byte header.
Inferred items are marked *(inf)*.

### Packets

Handlers are registered by `FUN_100f9460(id, fn)` → `DAT_104cd384 + 0x3e4f4 + id*4` (id < 0x11f), at 0x10092cb7..0x10092db0.
Outgoing packets: `FUN_100f9480(id, prio, 0)` allocates, `FUN_100f9590(pkt, size, 0)` sends.

**c2s 0x01A action.** Builder `FUN_10077120` (alloc at 0x10077129), size 0x1c. Fields:

| Offset | Field |
|---|---|
| +4 | u32 target UniqueNo |
| +8 | u16 target ActIndex |
| +0xa | u16 ActionID (Talk = 0) |
| +0xc..+0x18 | u32 ActionBuf[4] |

The 5-argument wrapper `FUN_100770f0(actindex, uniqueno, action, buf0, prio)` zeroes buf[1..3]. It has 27 call sites; the one for the Talk menu entry was not singled out.

**s2c event start: 0x032 (`1009e8a0`), 0x034 (`1009e970`), 0x033 (`1009ea60`).**

- Requires the game state `*DAT_10444110 == 0x60`. If `DAT_10475440` is already set, the handler prints group 9 message 0x81 instead.
- Sets `DAT_10475440 = 1`, `DAT_1046d210 = Mode | 0x2000`, `DAT_1046dacc = EventPara2`.
- **Hook:** `FUN_100ad2c0(EventNum, EventPara, EventNum2, current zone @ DAT_104cd388+0x365a6)`. Field offsets are +0xa / +0xc / +0x10 for 0x032 and +0x2a / +0x2c / +0x30 for 0x034.
- It then stores: `DAT_10473574` EventNum, `DAT_1046d20a` event id, `DAT_1046e0ac` UniqueNo, `DAT_10470522` ActIndex.
- 0x034: num[8] → `DAT_10475df8`.
- 0x033: String[4][16] → `DAT_10476300 + n*0x100`, mirrored to `DAT_104758b0`. With Mode & 8, Data[8] @+0x50 → `DAT_10475df8`.

**`FUN_100ad2c0`** resets the VM state and picks the file zone:

- Current zone outside 1000..0x513: EventNum when EventNum2 == EventNum, else EventNum2 + 1000.
- Inside that range: current zone + 1000.

It then loads two files:

- **Event file**, `FUN_100ad140`: z<256 → 5820+z; <1000 → z+0x14aff; <2000 → z+0xde31; else z+0x1004b.
- **Dialogue file**, `FUN_100ac7b0`: English 6420+z / 85591+(z-256) / 57945+(z-1000) / 68511+(z-2000). Japanese bases: 6120 / 85291 / 57913 / 68211.

**s2c 0x036 talk (`1009f590`).**

- Text = `FUN_10094d20(8, MesNum & 0x7fff)`. Kind 8 resolves to `FUN_10094c90` → `FUN_10094aa0(DAT_1047547c, id)`, the zone-resident dialogue.
- Type → colour/mode via `DAT_1035056c`.
- With the name: `FUN_100abbb0` ("%s : " + text). With MesNum bit 15 set: `FUN_100abc30`, no name.
- Both build a 0x1304-byte message object (`FUN_100c0e70`):

  | Offset | Content |
  |---|---|
  | +0x621 | text |
  | +0xbfe | speaker ActIndex |
  | +0xc00 | target |
  | +0xc50 | work[5] (`DAT_10474618`) |
  | +0xc64 | num[8] |
  | +0xd04 | strings |

- The queue is processed by `FUN_100c1420`, which runs the expander `FUN_100bf790`. `FUN_100c14d0` sends the result to the log (`FUN_10124050`) and plays the speaker's talk motion for modes 9/0xb/0x32/0x2cf/0x2d.

**s2c 0x027 (`100a0460`).** Same path, with Num1[4] / String1 / String2 / Num2[8].

**s2c 0x052 release (`100a06f0`).** Behaviour by mode:

| Mode | Effect |
|---|---|
| 0 | `DAT_10473068 = 1`, `FUN_10085ee0` (release) |
| 1 | `DAT_10472bb0 = 0` (update acknowledged) |
| 2 | Cancel if the id == `DAT_1046d20a` |
| 3 | `DAT_1046d180 = 0` |
| 4 | `DAT_10475441 = 0` (fishing) |

**s2c 0x05C (`1009e790`).** num[8] → `DAT_10475df8`.

**s2c 0x065 (`1009d970`).** Calls `FUN_1009d500(..., mode @+0x1c)`, the same body as 0x05B WPOS (`1009d4e0`).

**c2s 0x05B event end / update**, size 0x14. Builders: update at `1009ebb0` (Mode 1), end at `1009ed00` (Mode 0). Fields:

| Offset | Field |
|---|---|
| +4 | UniqueNo `DAT_1046e0ac` |
| +8 | EndPara: `DAT_10475df4` (the option), or 0x40000000 when `DAT_1047462c` is set |
| +0xc | ActIndex `DAT_10470522` |
| +0xe | Mode |
| +0x10 | EventNum `DAT_10473574` |
| +0x12 | EventPara `DAT_1046d20a` |

The end builder then sets `DAT_1046d20a = 0xffff`.

**c2s 0x05C** (`1009ec80`), size 0x20:

| Offset | Field |
|---|---|
| +4 / +8 / +0xc | x / y / z |
| +0x10 | UniqueNo |
| +0x14 | EndPara |
| +0x18 | EventNum |
| +0x1a | EventPara |
| +0x1c | ActIndex |
| +0x1e | Mode = 1 |
| +0x1f | dir |

**Flow (Southern San d'Oria, Ailevia → `startEvent(615)`):**

1. c2s 0x01A action 0.
2. s2c 0x032 (event 615, zone 230).
3. The VM runs. A choice opcode (`FUN_100b0960`) sets `DAT_10475df4`.
4. c2s 0x05B Mode 0 with EndPara = option.
5. LSB runs `onEventFinish` and sends s2c 0x052 mode 0 to release.
6. An `onEventUpdate` round trip is c2s 0x05B Mode 1, answered by 0x05C params, then 0x052 mode 1.

### Zone dialogue DAT

Zone 230 is file 6650, with 16553 entries.

- u32 at +0 = size | 0x10000000.
- From byte 4, every byte is XOR 0x80 when byte 3 == 0x10.
- Then a table of u32 offsets relative to +4; count = off[0] / 4.
- Loaded by `FUN_100890c0`, decoded by `FUN_10089180`, read by `FUN_10094aa0` / `FUN_10094ad0`.

**Install / LSB mismatch.** The installed DATs are older than LSB's IDs.lua. The shift is piecewise: +7 around 6431..6503, +29 at 6702, +43 from about 6832. For example, TAUMILA_DIALOG 8002 is DAT 7959, and ROSEL_GREETINGS 7810 is DAT 7767. The install has to match LSB's client version (30261006_0), or text and event ids are wrong.

### Control codes (`FUN_100bf790`; lengths from the same function, display meaning from `FUN_10123060`)

**Plain bytes:**

- Bytes ≥ 0x80 are 2-byte Shift-JIS.
- 00: end.
- 07: newline.
- 0b: choice marker, copied through. `FUN_100b0960` takes the text before it as the question and each 07-separated line after it as an option.
- Other bytes < 0x20 without an entry below: 1 byte, copied through.

**Codes with arguments** (num = msg+0xc64, work = msg+0xc50):

| Code | Length | Meaning |
|---|---|---|
| 01 len kind args… | 2 + len | English grammar block (`FUN_100bc8a0`). See below. |
| 02 / 03 lo hi | 3 | Set `DAT_10476f98` / `DAT_10476f9a`; 03 also calls `FUN_10132030` |
| 05 n | 2 | Kind-2 text of work[n] |
| 08 | 1 | Player name (entity +0x7c / +0x210) |
| 09 | 1 | Speaker name |
| 0a n | 2 | num[n] as "%d" |
| 0c n | 2 + `[a/b]` | num[n] picks an alternative (`FUN_100bbc10`) |
| 0d / 0e n | 2 | "SeSep" sound of work / num |
| 10 / 11 n | 2 | Name of work / num (`FUN_100bbaf0`) |
| 12 n | 2 | Number |
| 13 0/1 | 2 | |
| 14 n | 2 | "%d:%02u:%02u" |
| 16 / 1a n | 2 | Name: id < 0x100 → `FUN_100bba90`, else group 7 (id − 0x100) |
| 17 n | 2 | Player emote motion ("exp0exp1gak0…") |
| 18 / 19 n | 2 | |
| 1c n | 2 | Event string n (msg+0xd04+n*0x100) |
| 1d | 1 | Compass direction of the speaker (group 0xf) |
| 1e xx | 2 | Colour |
| 1f xx | 2 | Message mode, sets msg+4 |

**Grammar block arguments** (`FUN_100bbd20`): a byte with 0x40 set is slot b & 0x3f. A low-6-bit value of 2 is an index packed 7 bits per byte over the next 3 bytes.

**Grammar kinds:**

- **0x20..0x28: items.** Records are 0xc00 bytes (`FUN_101550f0`). Form by (kind − 0x20) % 3: name / with article / plural. Source: index / num / work.
- **0x30..0x35 and 0x42..0x44: names through `FUN_1014e180`** *(inf: key items)*.
- **0x03 / 0x04:** count.
- **0x37 / 0x38:** group 0x14 text.

**7f sub-codes:**

| Code | Length | Meaning |
|---|---|---|
| 7f 31 | 2 | **Prompt**: the log line waits for a key (type 0x8001) |
| 7f 32 / 7f 33 | 2 | Types 0x4002 / 0x4001; 33 also eats a following 07 |
| 7f 34 n | 3 | **Wait** n×60 ticks (0x8002) |
| 7f 35 n / 7f 36 n | 3 | Timed waits, types 0x8003 / 2 |
| 7f 37 | 2 | Type 1 |
| 7f 38 lo hi | 4 | Wait, u16 (0x8002) |
| 7f 80 x | 3 | Article / count mode |
| 7f 81 n | 3 | |
| 7f 84 n | 3 | List of set bits as group 3 names |
| 7f 85 | `[m/f]` | Player gender |
| 7f 86 / 7f 92 n | `[..]` | Plural of work / num |
| 7f 87 / 7f 88 | `[..]` | |
| 7f 8f n | 3 | |
| 7f 90 / 7f 91 | `[..]` | Speaker / target gender |
| 7f 93 | 2 | String at msg+0xc10 |
| 7f 94 / 95 / 96 / 97 | 3 | "%02d" / "$%x" / binary / work number |
| 7f a0..ab | 3 | Vana'diel date / time parts |
| 7f ac, b0, b1, b4, b5 | 3 | |
| Other 7f xx | 2 | |

**Decoded examples from zone 230:**

- 2 "Care to learn about Mog Houses?{choice}Sure! / No, thanks.{next}"
- 7959 "I am Taumila, the owner of this establishment. …{next}"
- 7768 "… Prince {select num4}[Trion/Pieuje] …"
- 7811 "If you bring me three {item pl num0}, …"

### Dialogue window

- **Speech goes to the chat log**, not to a separate box. The window is "menu    logwindo" (39542), at (16,298), 366x134, bottom-anchored. Its background is windowps sprite 0.
- **Prompt arrow:** cursor library "anc     kaipage" (child 1, 10x11). There are 6 frames from the "anc     btwait" texture: a brown down-pointing arrow. They advance one per draw (`FUN_101212b0`); `FUN_1015ad10` draws them with `FUN_1011c470(x, y, 0x80808080)`.
- **Choices:** "menu    query", at (16,208), 366x88, windowps sprite 11, cursor anc_s.
  - Rows are at (28, 28/48/68), 132x16, keytops3 50.
  - 3 rows are visible and scroll.
  - Width = max(text, option + 0x4c).
  - The hidden-option mask is `FUN_100ada70(5)`; the default cursor is `FUN_100ada70(3)`.
- **Wrapping:** `FUN_10123060` wraps a message (`FUN_101267f0`, width from `*(short*)(DAT_105dc3e4+0x32)`). After 4 lines it splits the message and continues it.

### Prototype

- **`dat/dialog.rs`:**
  - `file_id(zone)`
  - `Dialog::load(&DatIndex, zone)`, `get(id)` (readable `{tags}`), `raw(id)`, `len()`
  - `readable()`
  - `expand(raw, &Ctx)`: player / speaker / num / strings / `[a/b]` / gender, with prompt and choice split
- **`dialogshot.rs`:** `dialogue-shot --out P [--zone 230 --id 7959 --speaker Taumila --choice ID --frame N]`. It draws:
  - logwindo with "Speaker : text" in the game font, wrapped;
  - the kaipage arrow after the text when the line ends in 7f 31;
  - with `--choice`, the query window with the question, options and the anc_s cursor;
  - a 4x strip of the arrow frames at `P.arrow.png`.
- **Apply script:** `tools/apply_dialog.py TREE`.

## Event scripts: event DATs, the event VM, cutscene cameras (event-dump, event-ops, cs-shot)

Decompiled from FFXiMain (unpacked); every address below is in the Ghidra corpus. Packets, dialogue
text and the window are in the dialogue entry (modlog_dialog.md); this entry is the script side.

### Flow of one NPC talk
1. Client sends 0x01A action 0 (Talk) to the NPC.
2. LSB answers 0x032 / 0x034 (event with 8 params) / 0x033 (string event). The handlers
   (1009e8a0 / 1009e970 / 1009ea60) call `FUN_100ad2c0(EventNum, EventPara, EventNum2, zone)`, which
   loads the zone's event DAT (FUN_100ad140) and dialogue DAT (FUN_100ac7b0) and starts the VM.
3. The VM runs the NPC's script for that event id (the LSB csid).
4. Op 0x43 sub 0 sends 0x05B mode 1 (update) and sub 1 waits for the server's reply; op 0x47 sends 0x05C
   (position). Op 0x21 ends the event: FUN_100b1850 -> FUN_100acf20 -> FUN_1009ed00 sends 0x05B mode 0
   with option = zone work Z1 (DAT_10475df4), or 0x40000000 when the event was cancelled.

### Event DAT
- File id (FUN_100ad140): zone < 256 -> zone + 0x16bc (230 -> 6050); < 1000 -> + 0x14aff; < 2000 -> + 0xde31;
  else + 0x1004b.
- `u32 count` (low 30 bits), `count` x u32 block sizes, then blocks. Block: `u32 actor` (server id
  0x010exxxx, 0x7ffffff0 = player), `u32 n`, `u16 entry[n]`, `u16 event_id[n]` (0xffff = default/idle
  entries, 0xfffe = wildcard), `u32 nconst`, `u32 const[nconst]`, `u32 code_len`, code.
- Parsed for all 289 zones / 20896 blocks with no errors.

### Event VM
- Binding (FUN_100ad460): each block whose id list holds the current event id (DAT_10475cd4,
  FUN_100bb290) is bound to its actor. FUN_100bb300 builds a 0x268-byte VM object: +2 actor index,
  +8 actor id, +0xc n, +0x10 entries, +0x14 ids, +0x1c consts, +0x20 code, +0x256 pc, +0x258 thread,
  +0x25a yield, +0x260 locals, 16 threads at +0x24 + i*0x20 (u16 priority, u16 pc at +2).
- FUN_100baf30 starts thread 0 at entry[0] (priority 0x80), then the event's entry at priority 0x10.
  FUN_100bae70 picks the lowest priority value; FUN_100bae30 runs ops until one yields. The dispatcher
  is FUN_100ba3f0 (217 opcodes, 0x00-0xd8); an unknown opcode ends the thread (FUN_100ae240).
- Operands: value (FUN_100ada80) is a u16. Bit 15 = constant index; < 0x800 local; 0x1000 zone work Z
  (DAT_10475df0, u32[0x50]); 0x1100 memorize M; 0x1700 G; 0x7f00 event actor fields; 0x7f80 player
  fields. Stores go through FUN_100addc0, strings through FUN_100adf30.
- Server parameters arrive in Z: 0x034 num[8] is Z2..Z9 (DAT_10475df8). The option the client returns
  is Z1.
- Key opcodes (full table: `vana-client event-ops`):
  - Flow: 00 end, 01 goto, 02 if_not (cond byte @5), 1a call / 1b ret, 26 halt, 27-29 threads on
    other actors.
  - Dialogue: 1d message (spoken by the event actor), 2b message_by actor, 48/49/89/8d/c8 other
    message forms, 23 wait_message. 24 choice (message, default option, hidden-option mask) and 25
    wait_choice (Z0 = option - 1; cancel aborts the event).
  - Actors: 1e face, 1f/5a walk, 36/37 position, 5b/66 motion file + play (4cc such as 'tlk0'),
    5e idle motion.
  - Camera and screen: 46 camera mode, 45 scheduler from event camera file, 52/55 stop/wait it,
    69 screen mask, 6a fade, 6c actor fade.
  - Other: 5c/5d bgm, 77/78 time/weather, 71 text input (sub 1 sends 0x60), 9d tables / string compares.
- The code holds inline data tables, so a linear sweep is wrong. The disassembler walks control flow
  from every entry and jump target (stops: 00 01 1b 21 26 ca cb). 20880 / 20896 blocks decode fully,
  with no overlaps and 88.3% of code bytes reached.

### Cutscene cameras
- Op 0x45 `sched_file n, actor, target, '4cc'` runs scheduler `4cc` from DAT 0x77f0 + n; n is
  remapped by FUN_100b26c0 (+0x6551 from 300, +0x9adb from 600).
- These files use the title flyover's chunk format: type 0x07 schedulers (s000…), type 0x06 camera
  paths (c040…, keys of eye / focal / target / roll, fov_y = 2 atan(192 / focal)). A scheduler's op
  0x04 plays a camera path over its tick count.
- n = 200 (file 30904) holds the screen fades fdi1 / fdo1 / ovl1. n = 130 (file 30834) holds the San
  d'Oria cameras.
- Southern San d'Oria opening cutscene: player block, event 0x01f7. Narration messages start at 7625,
  and `sched_file #130 's043'` plays camera c043, a 600-tick pan along the city wall to the gate.

### Prototype
- `vana-client event-dump ZONE [--actor ID] [--event CSID]`: the blocks and disassembly of a zone, with
  dialogue text inlined from the zone dialogue DAT.
- `vana-client event-ops`: the opcode table (handler, length, wait/stop, operands).
- `vana-client cs-shot ZONE N SCHED --out-dir DIR [--steps K]`: renders the scheduler's camera paths in
  the zone at K points.
- Files: dat/event.rs (parser, operand kinds, control-flow walk, disassembler), dat/event_ops.rs
  (generated opcode table), eventdump.rs.

### Unknowns
- Op b1 has no length for sub 4. The 16 blocks that do not decode fully all stop there, one of them in
  zone 230.
- Ops marked "inferred" in event-ops have a known handler but an unconfirmed meaning: 46 sub 2, 69/6a
  screen layers, the units of 1f.
- The installed DATs are older than LSB's text ids (see the dialogue entry), so csids match but
  message ids can be shifted.

## Sound (FFXiMain sound module 0x1023b000..0x10242000 + the install; WAVs and device recordings checked)
- Roots: `sound`, `sound2` .. `sound9` + `\win\music\data` / `\win\se`, searched in that order, first hit
  wins (strings 0x10346608 / 0x1034662c, FUN_10035xxx init; FUN_1023bed0 / FUN_1023f6f0 try 16 slots).
  Music file = `music%03u.bgw` of (BGM id & 0x3ff): the id is the file number (traced, no table).
  Effects `se%03u\se%06u.spw` (id / 1000, id).
- BGMStream (0x30-byte header, copied whole by FUN_102408b0): "BGMStream" @0, u32 format @0x0c (0 ADPCM,
  1 PCM16 LE, 2 PCM16 BE, 3 ATRAC3), size @0x10, id @0x14, length @0x18 (ADPCM blocks, else sample
  frames), loop start @0x1c (-1 = none, same units), rate = (u32 @0x20 + u32 @0x24) & 0x7fffffff, data
  offset @0x28, volume level @0x2c, bits @0x2d, channels @0x2e, @0x2f ADPCM samples per block or ATRAC3
  mode. SeWave: size @0x08, format @0x0c, then the same fields 4 bytes earlier (id @0x10 .. data @0x24),
  volume @0x28, flags @0x29, channels @0x2a, bits @0x2b, block/mode @0x2c (FUN_10240090, voice +0xc).
  Every bgw/spw in the install parses; format-0 sizes = 0x30 + length * ch * (block/2 + 1) exactly.
  Install: 209 bgw (127 ADPCM with 4..128-sample blocks, 81 ATRAC3, and mov/music999.bgw, a plain RIFF
  WAVE that FUN_1023f3c0 plays as PCM with id 0x400), 11806 spw (9659 ADPCM, 1141 PCM16, 1006 ATRAC3),
  all 44100 or 48000 Hz; every one parses and decodes (test every_installed_sound_parses).
- ADPCM (FUN_1023c3a0): per channel per block a byte (filter << 4 | shift), then block/2 bytes, low nibble
  first; s = ((k0 s1 + k1 s2) >> 6) + ((i16)(nib << 12) >> shift), k from 0x1037c538: {0,0} {60,0}
  {115,-52} {98,-55} {122,-60}. The history keeps the unclamped int and the output is a (short) cast (a
  few wrapped samples exist, e.g. 8 in music119). Stereo = channel 0 block then channel 1 block. History
  carries across the loop.
- ATRAC3 (format 3; ported from FUN_102bf920..FUN_102c4970 into audio/atrac3.rs, all tables from the DLL):
  key = the rate's low word (bgw u32 @0x24, spw @0x20); k = rotl(key, 11), 100 u32 t[i+1] = t[i] +
  rotl(t[i], 3) (FUN_1023c310), every frame XORed from the table start (FUN_1023c360). Frame bytes by
  mode (FUN_1023d230 / FUN_1023d9b0): 1 -> 0x130 (105 kbps), 2 -> 0xc0 joint (66), else 0x180 (132);
  mono spw half, decoded as the frame repeated into both halves, channel 0 kept (FUN_1023cb20). Every
  format-3 file in the install uses mode 0. Frame k = samples 1024k.. (no delay); the first frame after
  init is decoded twice (FUN_102bfb30 returns nothing on its first call; callers retry). Loop: read
  from frame (L >> 10) - 1 as priming, then copy frame L >> 10 from L & 0x3ff (FUN_1023c620). Output is
  ffmpeg's atrac3 negated, within 1 LSB (music040 200 frames, se000040, se032140). The last 3 frames
  of music069/071/900 fail to decode in the client too (silence).
- Sound thread: one BGM stream (DAT_10623b28, two 0x1c0 slots, play uses the first), 12 effect voices
  (DAT_10623b38, 0xf4 each, free one from the end else stolen by priority FUN_1023fb90), 14 cached
  spw. Ticks at 50 Hz; fades are given in 60 Hz ticks and converted * 50 / 60. Command queue
  FUN_10241950 -> table 0x1037c560: 1 play embedded SeWave, 2 play se id, 6 stop music, 7 play music
  (path, level, fade; ignored while the same id plays), 8 music level (fade), 9 fade out then stop,
  10 save the music position for its id (DAT_10623b2c[0x400]; a later play of that id resumes there),
  11 clear saved positions (id or all).
- Levels 0..127 -> 0..4096 by 0x1037c338 (0, 2340, 2511 .. 4096). Music g = T[file vol @0x2c] *
  T[level] >> 12. DirectSound volume (FUN_1023e1f0) = ((P * g >> 12) * 10000 >> 12) - 10000 cB, P from
  0x1037c238 (3974 at the 0x40 centre); pan p != 0x40 -> +-((p & 0x3f) or its complement) * 10000 / 63.
  Ramps are value << 12 with step (target * 4096 - cur + 0x800) / steps; fade-ins start at T[1].
  Config factors: music level = vol * DAT_10346650 * DAT_10346654 (both init 1.0); menu effects level =
  DAT_10346568 * 127. Verified live: PipeWire recording of music108 = decoded track x 0.5383, the
  predicted -538 cB for file volume 100 (correlation 1.00000); effect peak 1271 = 1791 x 0.7096.
- Music change (CYyMusicLoadTask, FUN_10036530 -> update FUN_100365e0, per frame): optionally save the
  position (cmd 10), fade out over max(fade, 30) ticks, silence, 4 ticks, stop, start the new id at once,
  ramp to vol2 after 10 ticks if it differs, then an optional follow-up track. Strictly sequential:
  the client never crossfades. Asking for the id already wanted does nothing.
- Title music FUN_1023ae50(config 0xaf) -> FUN_10036980(0x6c ..): option -> (mode, n) at 0x10331ed8:
  0 newest owned, 1 random (timeGetTime % 5), 2..6 fixed n. n -> (expansion bit, entry) FUN_1023ae80:
  0 (0, 1), 1 (2, 2), 2 (3, 3), 3 (4, 4), 4 (11, 5). Entries {id, ticks, next} at 0x10346580:
  1 (108, 0, 108), 2 (242, 8100, 108), 3 (179, 7140, 108), 4 (140, 8160, 108), 5 (58, 8160, 108);
  entry 0 = 0x6c. Mode 0 plays the expansion track, fades it over 360 ticks, then loops 108; random and
  fixed stay on their track. The mask (DAT_10444110 + 0x10) is the installed-ROM mask, not the lobby's (traced, see lobycwin).
- Lobby (FUN_1022ef00): the dancer creator plays the race BGM; its step DAT_10623a58 reaching 7 (the nation step,
  ours 6: the same block tears the dancer down, FUN_10246f10) -> FUN_10036530(0x78 ..), cancelled (-1) -> title
  music. The city choice itself changes no music (files 24..26 have no music op; checked against retail video by
  the user). Back on the dancer steps the race BGM returns at the next performance wrap (FUN_10036a90). Race BGM: FUN_10036b40 fades out at race load
  without starting; FUN_10036a90 starts it when the performance time wraps (FUN_10245920); 0xa9 on
  the low-poly path. No music call on the select screen (title music continues).
- Zone music FUN_100926a0 (on 0x00A, then per frame): slots DAT_104735dc[8]: 0..4 = 0x00A MusicNum
  @0x56 (day, night, solo battle, party battle, mount), 5 = 0x6f (death), 6/7 from events; volumes
  DAT_10472b94 (0x7f). 0x05F handler 0x10098150: u16 slot @4 (< 8) gets u16 id @6, forcing a
  re-choice; event ops 0..7 / 0x80..0x87 set slot (+ volume) too (FUN_100b5070). Choice: dead (status
  2/3) -> 5; status 5 or 0x55 with slot 4 set -> 4; fishing 0x26 / 0x39 -> 7 if set, else no change;
  status 1 (engaged) -> 2 with one party member, else 3; Vana'diel hour < 6 or > 17 -> 1; else 0.
  On a change: leaving slot 0/1 saves its position and remembers the slot; coming back to it from
  2/3/7 fades in over 60 ticks and resumes; anything else clears all saved positions first and starts
  at once. Fade-out is always 60 ticks.
- Menu sounds: helpers FUN_10036d10.. look up type-0x3d resources by 4-char name (FUN_100712e0). DAT 0
  (ROM/0/0.DAT) has SeSep chunks "0001".."50xx" whose u32 @8 is the se id (= the name). Menu events
  (FUN_10208f30 and friends): 1/2 cursor -> "0001", 5 select -> "0002", 6 cancel -> "0003", refused ->
  "0004"; FUN_10157290 (close) -> "0013"; FUN_1013b080 (main menu opens) -> "0014".
- Creator foley in the client: 31728 + race track 0 type-0 events {frame, value} play SeSep "value"
  from 31736 + race (e.g. 4017 -> se 24017), checked live (HumeM: 240xx footsteps and cloth).
- Not traced: positional effect volume (FUN_10035e50 distance curve, used by the creator foley at the
  hip bone; we play it centred at full level), FUN_1023fb90 voice stealing, slot 6's condition
  (DAT_1047054c), the 0x10c sub-state test on fishing, battle flag DAT_1046e0f4, FUN_10096560 (sets
  slots 2/3 to 0x46).
- In-game footsteps (found, not implemented): actor FUN_100d2970, only within a distance of the camera,
  builds a 4-char name '0', '0' + vtable+0x218 (likely the ground kind), a digit/letter from actor
  +0x879, '1' + actor +0x87a, and looks it up in the type-1 folder "fses" ("fser" in rain, squall or
  thunder weather unless actor +0x17c is set), then plays it positioned through FUN_10036180. The
  inputs (+0x218, +0x879, +0x87a) and the step timing are not traced.

## Event VM runtime (event_vm.rs), event-run / event-sweep

A headless interpreter for the zone event scripts. It runs them the way FFXiMain does (FUN_100ba3f0 and
its handlers) and hands world work to the client through a `Host` trait. Everything below was
decompiled; the parts that are not confirmed are listed under "Inferred".

### Scheduling (FUN_100baf30, FUN_100bae70, FUN_100bae30)
- There is one VM per bound block. A block is bound when its id list holds the event id or the 0xfffe
  wildcard (FUN_100bb290).
- VMs run in actor-slot order, NPCs (id & 0x3ff) first and the player last.
- Each VM has:
  - 16 thread slots of 0x20 bytes, laid out below;
  - a 17th slot at +0x224, the pending end handler;
  - one 8-deep call stack shared by its threads (+0x244);
  - a locals block (+0x260) with 0x50 words, then the actor transform: +0x140 x, +0x144 height,
    +0x148 z, +0x154 dir, +0x168 speed (5.0 at bind).

  | Slot offset | Contents |
  |---|---|
  | +0 | priority (0xff = free) |
  | +2 | pc |
  | +4 | walk target |
  | +0x16 | entry index |
  | +0x17 | 0x28 / 0x29 state |
  | +0x18 | timer (-1 = idle) |
  | +0x1c | caller actor id |

- **Start:** slot 0 runs entry[0] at priority 0x80 at once, until it yields. Slot 0 then takes the
  event's entry (exact id, else 0xfffe) at priority 0x10.
- **Each frame:** each VM runs one thread, the lowest priority value (ties go to the highest slot),
  until an op yields. Actor +0x120 bit 0x80000 (op 0x2f) pauses that VM.
- **Gate:** VMs run while `(!DAT_1046daca && !DAT_1047462c) || DAT_1046def8`.
- **Waiting:** a waiting op re-executes every frame without advancing.

### Ending (FUN_100b1850, FUN_100bb240, FUN_100b1b10, FUN_100acf20)
- Op 0x21 sets daca = 1 and def8 = 1, yields, and does not advance.
- The next frame-end resets every thread of every VM. A registered end handler (op 0x7a sub 4) moves
  into slot 0 and runs while def8 == 2.
- When no handler is left, def8 = 0 and the client sends c2s 0x05B mode 0:

  | Field | Value |
  |---|---|
  | +4 | target UniqueNo |
  | +8 | option = Z1 (DAT_10475df4), or 0x40000000 when cancelled |
  | +0xc | ActIndex |
  | +0x10 | zone |
  | +0x12 | event id |

- These cancel the event: a cancelled message (window state 2), a cancelled choice (Z0 = 0xfe), and
  s2c 0x052 mode 2 with this event's id.

### Work and values (FUN_100ada80 / FUN_100ad8f0 / FUN_100addc0 / FUN_100adf30)
- **Locals:** < 0x50.
- **Z (DAT_10475df0):** 0x60 words.
  - Start clears Z0, Z1 and Z16 and up.
  - 0x034 num[8] lands in Z2..Z9.
  - Messages take their {num n} values from Z2..Z9 (FUN_100c0e70 copies DAT_10475df8).
- **M (0x1100) and G (0x1700):** persist across events (`Memory`).
- **Actor values:** 0x7f00..03 are the event actor's x / height / z ×1000 and dir ×4096/2π;
  0x7f80..83 are the same for the player.
- **Operand order:** positions are always (x, z, height), ×0.001.

### Server round trips
- **Update (0x43):** sub 0 sends 0x05B mode 1 and sets DAT_10472bb0. Sub 1 yields until s2c 0x052
  mode 1 clears it (100a06f0); 0x05C params arrive first.
- **Position (0x47):** sub 0 sends 0x05C and sets bb0 and d17f. Sub 1 also waits for 0x065 / 0x05B
  position modes 1-2 (FUN_1009d500).

### Messages and choices
- **Message ops:**
  - 0x1d: speaker is the event actor;
  - 0x2b: speaker is actor@1;
  - 0x48: system message, no name;
  - 0x49: no name, speaker actor@1;
  - 0xb0 sub 0: speaker actor@2.
- Each sets the window state DAT_10475800 = 1 and advances without yielding.
- DAT_10470548, the actor that {speaker} names, is set by those ops. 0x48 keeps the previous one and
  0x24 clears it.
- **0x23:** advances once the window is closed, yields while it is open, and aborts the event when the
  window is cancelled.
- **0x24:**
  - It prints the question (at least one frame), then builds the menu.
  - The question is the text before 0x0b; options are split at 0x07.
  - Bit i of val@5 hides option i. The cursor starts at option val@3.
  - It advances without yielding.
- **0x25:** Z0 = original option index (1-based pick - 1). Cancel gives Z0 = 0xfe and aborts the
  event. It yields once.
- **Correction:** 0x89 / 0x8d / 0xc8 / 0x8b / 0xb8 are the **map** window and its markers (FUN_100b08e0
  → FUN_101ee9c0, "menu_mapframe"), not messages. 0x8a closes the map. They are renamed in event_ops,
  and event-dump no longer prints dialogue text for them.

### Threads (FUN_100b1a90, FUN_100b1a40, FUN_100b1a00; ECX = the target VM, checked in the asm)
- **0x27 start:**
  - It takes the last free slot.
  - If an active slot already runs that entry, it does nothing.
  - With no free slot it yields and retries.
  - If the target has no bound block, it does nothing.
- **0x28:** waits while another slot of the target holds the entry.
- **0x29:** waits until the entry is seen, then until it is gone.
- **0x2a p:** waits while the target has a thread with priority <= p.
- **0x7a:**
  - sub 0 / 1: kill threads (all of them / those running one entry);
  - sub 2 / 3: act as another actor (locals and actor switched) / switch back;
  - sub 4 / 5: register / clear an end handler.

### Waits
- Timers count 60 Hz ticks, dt = max(frame ticks, 1).
- 0x1c `wait n` and 0x6f (16 ticks) yield every frame, including the frame they finish on.
- **Walk (0x1f / 0x5a):**
  - sub 0 stores the target (x, z, height) in the slot.
  - sub 1 moves speed × dt / 60 per frame toward it: flat for 1f, 3D for 5a.
  - dir = atan2(-dz, dx). 1f also sets the height to the target and ground-snaps.
- **Turning (0x70 / 0x76):** waits on actor +0x12c bit 1, meaning the turn has not finished.
- **Scheduler (0x55 etc.):** waits while FUN_10061f20 reports the scheduler is playing.
- **Scheduler files:** 0x45/52/55 use 0x77f0 + remap(n). The other bases are 0x1394, 0xc7ef, 0xdd6d,
  0x1071b, 0x11323, 0x11423 and 0x19031, with no remap.

### Opcode table fixes
- **b1:** {0:4, 1:4, 2:4}.
  - Sub 3 and 4 have no handler in this DLL. In the client the pc would not advance and nothing would
    yield, so it would hang.
  - The data's lengths are {3:4, 4:6}, inferred from the bytes.
  - All 16 blocks that use them are the same bard NPC, behind `get_config sub 2 == 2`, which this DLL
    never stores.
  - The disassembler now decodes all 20896 blocks. The VM reports these forms as `Fault`.
- **46:** {0:2, 1:2, 2:4} (player has a model, +0x120 bit 9).
  - sub 0 = FUN_10082b20 (DAT_10346088 = 1)
  - sub 1 = FUN_10082be0 (0)
  - sub 2 stores that flag

  Cutscenes run with the flag at 0.

### Talk (0x01A action 0)
- **Sender:** FUN_1007aac0, the only caller with action 0. It is reached from:
  - target-cursor confirm, FUN_101524e0;
  - the player menu, FUN_10127b60;
  - text commands, FUN_1007a910 / FUN_1007ad60.
- **Checks, in order:**
  1. No event is active and no talk is pending (DAT_10470520).
  2. Player and target exist with nonzero indexes.
  3. The player is not mounted.
  4. The target UniqueNo has a nonzero high byte.
  5. A target index above 0x6ff needs target +0x12c bit 30.
  6. The player has a model, and so does the target unless forced.
  7. The player does not have +0x128 bit 6.
  8. The 3D distance is ≤ 6.0, unless forced.
  9. The target is not dead (status 2 / 3).
- **After sending:** DAT_10470520 = 1, the talk lock.
- **Lock release:** op 0x20, s2c 0x052 mode 0, event cleanup or zone reset.

### Server text shift (tools/msgshift.py, events/msgshift.json)
- s2c 0x036 and 0x02A index the same zone dialogue file as events (FUN_10094d20(8, id)), so ids from
  LandSandBoat need the installed client's ids.
- **Method:** match each LSB `IDs.lua` text comment against the DAT text, after normalising both. Each
  unique match is an anchor; consecutive anchors with the same delta form a segment.
- **Results:**
  - 287 zones; 99.4% of commented entries matched (8439 / 8493); no conflicting matches.
  - 284 zones are high confidence and 3 are medium (154, 239, 281).
  - Ids between two segments are left unknown.
  - Zone 230: 6818-8244 is -43 (63 anchors), then 8263-10355 is -44.
- Event message ids come from the client's own event DAT and need no shift.

### Commands and tests
- `vana-client event-run ZONE EVENT [--target ID] [--choice N|-1 ...] [--param P ...]` prints the
  effects, the final actor transforms and the frame count. `FFXI_EVENT_TRACE=1` prints each executed op.
- `vana-client event-sweep [ZONE...]` runs every event id with scripted inputs.
- Tests (`cargo test event_vm`):
  - **0x0249, actor 0x01b:** the Mog House explanation.
    - Sure → messages 0, 1, 3, 4, 5, then 0x05B option 0.
    - No → 0, 1, 8, then option 1.
    - Cancel → option 0x40000000.
  - **0x024a:** {select num0} reads the 0x034 param ("Bastok"). Cancelling the second message aborts
    the event.
  - **0x01f7, the opening cutscene:** cameras s043, s044, s045 from file 30834, narration 7625 first,
    then the end packet.
- **Sweep of all 295 zones (31854 events):**
  - 29656 finish (93.1%).
  - 874 go idle: every thread ends without 0x21, so the server ends them.
  - 1324 are still running after 5 minutes. These are menus that loop under the sweep's choice
    heuristic, and ambient loops.
  - 4 never yield. They loop on state the headless host does not model.

### Inferred / open
- Item and key item names in text ({item}, {keyitem}): dialog::expand drops them; they need the item
  DATs.
- 0x6a screen fade operands are raw.
- Motion busy comes from the host (actor vtable +0x2a0); the headless host keeps it busy for 30 frames.
- DAT_1046ddf9 (event running) and the actor-loaded wait in FUN_100ad460 are not modelled; all actors
  are taken to exist.

## Item DATs, name tables, item and key-item names in dialogue (dat/items.rs, dat/names.rs)

### Item records (FUN_101550f0, FUN_101535e0, FUN_10154e50)
- Range table DAT_103662e8: 0x14-byte entries (file, first id, end id, base, parser). The file comes
  from language slot FUN_10250e50(slot); the record is at (id - first) * 0xc00.
- Decryption (FUN_10156f00): every byte rotated right by 5, over the whole 0xc00 bytes.
- Language tables (FUN_10250de0): 0 ja 0x1037e428, 1 en 0x1037de80, 2 de 0x1037dcd0, 3 fr 0x1037e038
  (dat/msg.rs labels 0x600d0 as fr; it is the German table).

| ids | parser | kind | ja / en / de / fr |
|---|---|---|---|
| 0000-0fff | 101557b0 | general | 4 / 73 / 55815 / 56235 |
| 2200-27ff | 101557b0 | general 2 | 55551 / 55671 / 55791 / 56211 |
| 1000-1fff | 10155920 | usable | 5 / 74 / 55816 / 56236 |
| 2000-21ff | 10155a90 | automaton | 8 / 77 / 55819 / 56239 |
| 2800-3fff | 10155bf0 | armor | 7 / 76 / 55818 / 56238 |
| 5a00-6fff | 10155bf0 | armor 2 | 55548 / 55668 / 55788 / 56208 |
| 4000-59ff | 10155da0 | weapon | 6 / 75 / 55817 / 56237 |
| 7000-73ff | 10155f70 | ? | 55547 / 55667 / 55787 / 56207 |
| 7400-77ff | 101560e0 | equipment-like | 55550 / 55670 / 55790 / 56210 |
| e000-eff7 | 10156270 | Records of Eminence objectives | 55552 / 55672 / 55792 / 56212 |
| eff8-efff | 101563d0 | no strings | 95 (all) |
| f000-f1ff | 101564c0 | monstrosity species | 55549 / 55669 / 55789 / 56209 |
| f200-f5ff | 10156630 | ? | 55553 / 55673 / 55793 / 56213 |
| f600-f613 | 10156790 | no strings | 55504 (all) |
| f620-f62f | 10156a20 | ? | 55558 / 55678 / 55798 / 56218 |
| f630-f71f | 101568d0 | ? | 55557 / 55677 / 55797 / 56217 |
| ffff-1000e | 10156b70 | currency (gil) | 9 / 91 / 55820 / 56240 |

- Common header: +0 id, +4 flags, +6 stack, +8 type, +0xa resource, +0xc targets.
- Armor: +0xe level, +0x10 slots, +0x12 races, +0x14 jobs (u32), +0x18 superior level, +0x1a shield
  size, +0x1c charges, +0x1d cast time, +0x1e use delay, +0x20 reuse delay (u32); strings at +0x2c.
- Weapon: as armor to +0x18, then +0x1c damage, +0x1e delay, +0x20 dps, +0x22 skill, +0x28 charges,
  +0x29 cast, +0x2a use delay, +0x2c reuse; strings at +0x38. (Bronze Sword: level 1, DMG 6, delay 231,
  skill 3.)
- Strings at +0x18 (general), +0x1c (usable), +0x2c, +0x38, …: u32 count, (u32 offset, u32 type) pairs
  (FUN_1024cde0); type 0 text 0x1c bytes in after a u16 marker 1, type 1 a u32 (FUN_1024d0f0).
  - ja: name, descriptions. en: name, article (number), singular, plural, descriptions (FUN_10156d00).
    de: name + 7 grammar fields. fr: name + 4.
- Icon (FUN_101557b0 +0xcec / +0xce8): +0x280 u32 size (0x839), +0x284 data: flag byte, 16-byte name,
  BITMAPINFOHEADER (32x32, 8 bpp), 256 BGRA colours (alpha 0..0x80), pixels bottom-up.
- Fields the parsers map but whose use is not traced are kept raw (`Item::extra`, keyed by object
  offset, e.g. 0xd34).
- The German and French weapon / armor files are smaller and use an older record layout (no field at
  +0x18): only their general items decode correctly.

### English articles (FUN_100bc8a0 grammar blocks, phrase builder FUN_1024f820)
- Item string object (vtable 0x10329b1c, FUN_1014fda0): article type 0 "a", 1 "an", 2 "pair of",
  3 "suit of" (then a/an by the vowel test FUN_1024e280, "aieuoAIUEO").
- Key items (slot 0x3d, FUN_1014d7e0, en fields [id, type, -, -, singular, plural, desc]): type 4 is
  a mass noun ("some"); 2/3 countable with "a"/"an" (3); 0/1 article by vowel (vtable 0x10329aa0).
- Grammar kinds: 0x01 indefinite article, 0x02 "the", 0x03/0x04 n (n != 1 prints n);
  0x20-0x28 items, 0x30-0x35 / 0x42-0x44 key items: form (k % 3) = bare (mode 2/3) / singular with
  "pair of" (mode 3) / plural (mode 4); source by k / 3 = index, num[], work[].
  0x37/0x38 place (message group 0x14), 0x12 title, 0x39/0x40/0x41 objective (item id + 0xe000).
- Articles render as: indefinite + mode 3 = "a"/"an", modes 2 / 4 = "some"; definite = "the".
- Plain codes 10/11 = spell name of work / num (FUN_100bbaf0, slot 0x5a); 16/1a = weapon skill name
  below 0x100 (FUN_100bba90, slot 0x5c; job abilities are 0x200 + id).

### Name tables (dat/names.rs, d_msg)
- d_msg XOR is flagged by u16 +0xa == 1 (spell and ability files are plain), +0x1c table size 0 means
  fixed-size entries of +0x20 bytes.
- Files (ja / en / de / fr): key items 55585 / 55705 / 55825 / 56245; spells 55582 / 55702 / 55822 /
  56242 (help 55614 …); abilities 55581 / 55701 / 55821 / 56241 (help 55613 …); places 55535 / 55465 /
  55775 / 56195; titles 55584 / 55704 / 55824 / 56244; jobs 55467 / 55467 / 55776 / 56196.

### Commands
- `vana-client item-dump [--lang en|ja|de|fr] [--key] ID...`
- `vana-client item-icon-shot ID... --out PNG`
- `event-run` loads the English names: zone 230 event 0x029a prints "a magical shell called a linkshell",
  and event 0x01f7 with `--param 536` prints "Here, take this adventurer coupon."

## LSB text id mapping (dat/msgshift.rs), event sound volume ops, real event wait times

### Bundled msg-shift table
- The server's text ids need mapping. s2c 0x036 / 0x02A look their text up in the zone dialogue
  file (FUN_10094d20(8, id)), but LandSandBoat's ids come from a newer client. Event message ids
  come from the event DAT and need no mapping.
- **Table:** `crates/vana-client/data/msgshift.txt`, included at build time.
  - It holds derived numbers only, no LSB text.
  - One `zone Z MD5 CONFIDENCE` line per zone, where MD5 is the first 8 bytes of the md5 of the
    dialogue file the mapping was made from.
  - Then `FROM TO DELTA ANCHORS` lines: LSB ids FROM..=TO map to id + DELTA.
- **Loader:** `MsgShift::bundled().verified(&index)` drops zones whose dialogue file differs from the
  install, so another client version is never mis-mapped. `map(zone, lsb_id)` returns
  `Option<u32>`; ids between segments give None.
- **Regenerate:** `vana-client msgshift-gen --lsb /path/to/landsandboat [--out ...]` (Rust port of
  tools/msgshift.py):
  - It reads each zone's `IDs.lua` text table and `scripts/enum/zone.codegen.lua`.
  - It normalises both sides: control codes, [a/b] alternatives and <placeholders> are removed, and
    only lower-case letters are kept.
  - Matching is exact first, then by prefix for texts of 20 letters or more.
  - Each unique match is an anchor. A lone delta between two neighbours that agree is dropped as an
    outlier, and equal deltas merge into segments.
- **Output from this install:**
  - 287 zones; 8439 of 8493 commented entries matched.
  - The segments are identical to tools/msgshift.py for all 287 zones.
  - 284 zones are high confidence and 3 are medium.
- **Check:** `vana-client msgshift-map 230 8002` gives 7959 "I am Taumila, …".

### Event ops 0x69 / 0x6a are sound volume, not screen layers
- **0x69 (FUN_100b5c30):** mask val@2. Each bit's volume becomes 1.0 if u8@1 is 0, else 0, and any
  running fade on it is cancelled. Bit 0x10 sets DAT_10346578.
- **0x6a (FUN_100b5cc0):** for each bit of mask val@5, a fade task (FUN_10034be0, updated by
  FUN_10034b20) moves the volume from its current level to val@1 × 0.001 over val@3 ticks:
  `level = (target - start) * (1 - left / total) + start`.
- **Mask bits:**

  | Bit | Channel | Setter |
  |---|---|---|
  | 1 | world sound effects, DAT_1034656c (scales 3D sounds, FUN_10035590) | FUN_10035230 |
  | 2 | interface sounds, DAT_10346568 (inferred from FUN_10036d10, its one use) | FUN_10035280 |
  | 4 | music, DAT_10346570 (applied to the current BGM through FUN_10037630) | FUN_10035190 |
  | 8 | all of the above plus the stream at DAT_10450fe8 | FUN_100352d0 |

  Every level is scaled by the config volumes _DAT_10346560 × _DAT_10346564.
- **Example:** the San d'Oria opening starts with `0x69 1, #5`, muting world sound effects and music.
- The visual fades are schedulers, e.g. fdi1 / fdo1 in file 30904.
- event_vm emits `Effect::Volume { mask, level, ticks }`. The ops are renamed sound_volume /
  sound_fade.

### How long an event stays Busy
- **Schedulers** (0x45 starts one, 0x55 waits on it). The waiting op FUN_10061f20 is busy while the
  file is still loading, or while a CMoSchedularTask with that tag exists for the actor / target pair
  (FUN_10056060).
  - **The task (FUN_10056e60):** each frame its timer drops by the frame ticks. At 0 it runs ops in
    order, and each op adds its wait (u16 at +4) × speed.
  - **Speed (FUN_10056430):** the start op's last operand (val@0xf) sets it.
    - 0: speed 1.0.
    - 1: loops until 0x52 stops it.
    - n > 1: speed = n / total length, so the scheduler lasts n ticks.
  - **End:** after op 0 the task flags itself done and is deleted the next frame. Ops 0x67 / 0x6a
    end it at once.
  - So length = Σ op waits (ticks) + one frame. Examples: s043 600 ticks (its camera path), v017
    180 + 100, fdi1 60.
  - `event_vm::scheduler_ticks(file, tag, param)` computes it, and `sched_timer(index)` feeds it to
    the headless host.
- **Motions:**
  - 0x5b / 0x66 (FUN_100b3570) request the motion file once (flag +0x160 + bank) and yield only
    while it loads (FUN_100cf260). They then start the 4cc on the model (vtable +0x298), advance and
    yield once. They do not wait for the motion to finish.
  - 0x2c starts one the actor already has.
  - 0x53 (FUN_100b3170) is the wait: it yields while model vtable +0x2a0(4cc, target) reports the
    motion or scheduler is still playing.
  - `Busy::MotionLoad { actor, file }` and `Busy::Playing { actor, name }` model these.
- **Effect on the opening cutscene:** with real scheduler lengths, 0x01f7 runs 12097 frames
  (3:22 at 60 Hz) with messages closed after 2 frames, instead of 8019.

### Open
- FUN_10056c70 restarts a task whose resource (+0x80 → +0x78) holds an op of type 1. Every event
  scheduler starts with op 1 yet finishes in the client, so that list is not the scheduler's own
  ops. It is not traced further.
- How long a motion plays (the time +0x2a0 stays true) comes from the motion itself. The headless
  host keeps it busy for 30 frames.

## Inventory and equipment menus (inventor, equip, persona, money) and their item list

All of this is decompiled from FFXiMain. The menu layouts are the 0x30 chunks of file 39542, read by
the existing dat/ui.rs.

### Which class draws which window
- The menu table at 0x10366460 holds one 0x2c-byte entry per menu, matched in FUN_10158060. Each entry
  has the 16-byte name, a pointer to its handler global (+0x20), flags (+0x24) and three bytes after.
- Draw is vtable slot 4 (+0x10) of the handler.

| Menu | Handler | Draw |
|---|---|---|
| `inventor` | DAT_1061e260, list class FUN_101e9590 | FUN_101edb70, shared list base |
| `equip` | DAT_10565794 | FUN_1012ea90 |
| `persona` (status panel) | DAT_105659c4 | FUN_10141c10 |
| `money` | DAT_10565830 | FUN_1013b8b0 |
| `iteminfo` | DAT_105657ec | not traced |

- **Items menu:** FUN_101e9f10 opens `inventor` for a container source object. Its vfunc +0x54 is the
  container kind; its vfunc +4 places the window at (16,48), (200,48) or (314,48).
- **Equipment command:** FUN_1012ec60 opens persona, money, the inventory list (with the equippable
  source DAT_1056579c) and equip.

### Sprite-library names per language (FUN_101185f0 → FUN_10250e90, table 0x1037e1f0)
- Menu sprite refs name `menu frames`, `menu comwin` and `menu key1top`. The client swaps these for the
  language's library: for US that is `framesus`, `comwin` and `key1topu`.
- The language comes from `*(DAT_10444110+8)`: 0 ja, 1 us, 2 de, 3 fr.
- dat/ui.rs `UiFile::lib` now applies the swap (`lang_lib`). Before this, the inventor, equip and money
  window art was missing.

### Item list (FUN_101e9170 builds rows, FUN_101edb70 / FUN_101ebda0 draw them)
- **Rows:** one per occupied container slot except slot 0 (gil), in container order. A row is
  0x50 bytes with up to 8 cells (kind, value, colour).
- **Layout:**
  - 10 rows visible (FUN_101ed820(10)), 18 px apart (+0x26).
  - Rows start at window + (3, 4) (+0x2c / +0x2d).
- **Columns** (u16 tables): Japanese uses 0x10375edc {0, 18, 140, 165}; other languages use
  0x10375ee4 {0, 18, 140, 18}.

  | Cell | Kind | Drawn as |
  |---|---|---|
  | Icon | 0x16 | The 32x32 icon at 0.5 scale (FUN_101554c0 → FUN_10154cb0) |
  | Name | 0x19 | Game font at (x + 18, y + 2), FUN_10126240 |
  | Count | 7 | Shown only when the item's stack is above 1: "%d" in the sprite font, right-aligned at column end - 2 |

  With the US table the count column is 140..18, so the count lands right-aligned at x + 16, y + 4,
  over the icon.
- **Name colours** (FUN_101e8700, ARGB, 0x80 = 1.0):

  | Tint | Colour | When |
  |---|---|---|
  | 0x80404040 | grey | player +0x132 bit 0 is set |
  | 0x80407090 | blue | equipped weapon/armour that also passes FUN_100fbc70 (item id + extdata; meaning not traced) |
  | 0x80509050 | green | equipped, or the equipped linkshell / linkshell 2 |
  | 0x80909040 | yellow | non-equipment with item flag 0x400, or item type 8 (crystal), 0x16 or 0x17 |
  | 0x80808080 | white | everything else |

- **Cursor:** the window's kind-6 library (`anc_item`: hand plus a 174 px bar) is drawn at the cursor
  stop child, 132x16 at (3, 6 + 18k).

### Equipment window (FUN_1012ea90, FUN_1012d920)
- **Slot → child table** (0x103639d4), in slot order main, sub, range, ammo, head, body, hands, legs,
  feet, neck, waist, ear1, ear2, ring1, ring2, back:
  1, 5, 9, 13, 2, 3, 7, 12, 16, 6, 8, 10, 14, 11, 15, 4.
- **Grid:** the children are 32x32 at x 24/58/92/126 and y 11/45/79/113, so rows read
  Main Sub Range Ammo / Head Neck Ear1 Ear2 / Body Hands Ring1 Ring2 / Back Waist Legs Feet.
- **Slot labels:** the children's kind-0 refs, framesus #40..55.
- **Icons:** each equipped item at scale 1 at its child's position. Stackables get min(count, 99)
  printed as "%2d" right-aligned at (+30, +22).
- **Number fields** (table 0x103639f8, relative to the window):
  - Eight left-aligned element values at x 16/61/105/149 and y 162/175.
  - Attack at (48,148) and defence at (144,148), both 26 wide and right-aligned.
  - All clamped to -99..9999, at most 4 digits (FUN_1012d1f0).
  - They are drawn when persona +0x1f is set.

### Status panel (FUN_10141c10; HP/MP/TP and STR..CHR labels are in windowps #8)
- All positions are from the window origin.
- **Name:** clipped to width - 6 at (+4, +7), ending in fontshp #0x6c when cut (FUN_101c04c0).
- **Jobs:** "Lv%2d " (group 10 #0x59) for the main job at (+4, +0x13) and the sub job at (+4, +0x1f),
  then a job sprite at the text end + 2.
- **ILv line:**
  - "ILv" at (+4, +0x2b);
  - item level as "%3d/" or "---/", right-aligned at +0x46;
  - "Su%2d" at +0x4a.
- **HP / MP:** "%4d/" right-aligned at +0x46, then "%d" max at +0x48, at y +0x3a and +0x46.
- **TP:** "%4d" right-aligned at (+0x4b, +0x52).
- **STR..CHR:**
  - "%4d" at (+0x32, +0x60 + 13i).
  - Then " +%d" in green 0x80408040, or the negative bonus in red 0x80804040.

### Gil window (FUN_1013b8b0 → FUN_101bfbb0)
- Container slot 0's count, formatted "%d,%03d,%03d" with the separator from group 12 #0x5b (",").
- The value is clamped to 0..999,999,999 and "  G" is appended.
- It is right-aligned at (+0x67, +0x1f).

### Code
- **Model** (inventory.rs): `ItemInfo` trait, `build_rows`, `name_tint`, `ItemList`, `Equipment`
  (`SLOT_CHILD`, `EQUIP_FIELDS`), `Persona::build`, `gil_text`. It has two tests.
- **Drawing** (render/invwin.rs): `push_inventory`, `push_equipment`, `push_persona`, `push_money`.
  These only read the models.
- **Commands** (invshot.rs): `inventory-shot [--scroll N --cursor K]` and `equipment-shot [--cursor C]`.
  Without dat/items.rs they use placeholder items. With it (apply_items.py), invitems.rs implements
  `ItemInfo` for `Items`, giving real names, stack, type, flags and icons.

### Unknowns
- The scroll bar (`menu scroll` sprites 0..3) and the "used/total" slot count: I did not find what
  draws them.
- The `iteminfo` window contents.
- The job sprite after "Lv%2d " (FUN_1011c470's object is lost in the decompile). The shot draws the
  job name as text in its place.
- What FUN_100fbc70 tests (blue names).
- Whether a slot label is hidden under an equipped icon. Labels show through transparent icon pixels in
  the shot.
- The persona offsets while equipping (persona +0x1c = 2, DAT_103658f0) are not applied.

## Combat: battle messages, 0x028 actions, combat motions, the action menu (dat/battle.rs, dat/sched.rs, net/action.rs, client/combat.rs, combat-shot)

Ghidra copy for this work: ~/ffxi/lab/combat/proj (mk.sh / q.sh beside it). Handlers registered at 0x10092e00:
0x028 FUN_1009caa0, 0x029 FUN_1009cf70, 0x02D FUN_1009d1b0 (all `LAB_`s in the original dump).

### Battle message text ("msg basic")
- 0x029 / 0x02D text = FUN_10094d20(5, MessageNum). FUN_10094d20(kind, id) calls table 0x10322430[kind]: 1 DAT_10476f00,
  2 DAT_10475450, 3 DAT_10475454, 4 DAT_1047545c, 5 DAT_10475460, 6 DAT_10475468, 7 DAT_10475474, 8 zone dialogue.
  FUN_10093c80 fills them from language slots (FUN_10250e50): kind 2 = slot 0x60, 3 = 0x61, 4 = 0x62, 5 = 0x63, 6 = 0x65,
  7 = 0x66. English files: 0x60 7021 (combat skill names), 0x61 7023, 0x63 **7027** (ROM/27/72.DAT), 0x66 7035.
  Same format as the zone dialogue DAT (FUN_10094aa0). 7027 has 1024 entries (10-bit ids).
- Ids are **not shifted**: LSB `MsgBasic` ids match the installed text through 777 (checked 1, 6, 8, 9, 15, 264, 282,
  718, 777 and the comments of ~40 more). 240 entries are blank (`7f 31 00 07`), among them every Master Level
  message from 809 (809..828): the install predates them; those lines print nothing.
- Packet fields (offsets with the 4-byte header): 0x029 @4 caster id, @8 target id, @0xc Data, @0x10 Data2, @0x14/@0x16
  ActIndex caster/target, @0x18 message, @0x1a type. 0x02D @4/@8 ids, @0xc/@0xe indices, @0x10/@0x14 data, @0x18 message.
  Data / Data2 become work[0] / work[1] (DAT_10474618, copied to message +0xc50 by FUN_100c0e70); caster / target
  index go to message +0xbfe / +0xc00.
- Expander codes used by battle messages (FUN_100bf790, English FUN_100bc8a0):
  `01 len 10` caster name, `01 len 11` target name, `01 len 13 a` status name of work[a] (FUN_1021d5f0),
  `01 len 20..28` items (form k%3 name / article / plural, source k/3 index / num / work; grammar argument
  FUN_100bbd20); `12 n` work[n] "%d"; `10 n` spell name of work[n] (slot 0x5a = 55702); `16 n` work[n] < 0x100:
  ability (slot 0x5c = 55701), else kind 7 entry work[n] - 0x100; `05 n` kind-2 name; `7f 8f n` ability work[n] + 0x200;
  `7f 84 n` kind-3 names (n * 0x20 + bit) of the set bits of work[4], space separated; `14 n` h:mm:ss; `1f xx` sets the
  message mode (level up uses 0x79).
  `7f 88 n [The /]` article of actor n (0 caster, 1 target): PC (id < 0x01000000) picks alternative 99 = none;
  NPC picks 1 when FUN_10089300(index) (no article) else 0; an absent entity picks 0. `7f 87 n [hits/hit]`: 1 when
  FUN_10089350(index) (plural). `7f 86 n [/s]`: 1 when work[n] != 1.
- Name flags: DAT_1046ad78[ActIndex] bit 2 / bit 3, set by the 0x00D (0x10098ea6) and 0x00E (0x1009ab3b) handlers from
  u32 @0x24 bits 29 / 30 (LSB flags2 NamedFlag / SingleFlag; mob name_prefix bits 0x20 / 0x40, "Ding Bats").
- d_msg name files come in two layouts: indexed (table size @0x1c) and fixed records (table size 0, record size @0x20,
  count @0x28: 55701 abilities 0x50 bytes, 55702 spells 0x8c). The XOR-0xFF flag is byte @0x0a (1 in 55651 / 55725,
  0 in the plain 55701 / 55702); dat/msg.rs read it from @0x2c before. Status names: 55725 (1024 entries "KO",
  "weakness", "sleep"...); its language slot was not found (inferred from content).

### Chat mode and colour
- FUN_1009cda0(caster index, target index, message) returns a mode byte from one of seven 1024-byte tables in .data:
  target is us 0x1034d360, caster is us 0x1034cf60, caster in party 0x1034d760, target in party 0x1034db60, caster in
  alliance 0x1034df60, target in alliance 0x1034e360, otherwise (or caster / target missing) 0x1034e760; checked in that
  order. 0xC8 = not shown. "hits" (1): ours 0x14, on us 0x1c, others 0x28.
- Line colour (log record setup ~0x101237f0): FUN_10126010(mode) finds {u16 mode, u8 colour id, u8 group} in the
  zero-terminated table 0x10362b50; FUN_10126040(id) finds {u32 id, u32 colour} in 0x103629c0, 0x80808080 if absent.
  Colour bytes are B, G, R, A at 0x80 = 1.0 (the font's MODULATE2X): on us 0x5d = (2c 30 a0 70) red, system 0x68 =
  (10 80 80 60) yellow, our hits 0x63 = (80 80 c0 58). The chat filter test (FUN_100abf20 -> FUN_100ac450) and the
  blacklist (FUN_100a00a0) are not modelled. These .data tables are raw in the installed (packed) DLL too.

### 0x028 action packet
- Client reader FUN_10185a20: bit stream LSB first from packet byte 5 (FUN_10185dd0 at +4, FUN_10185530 skips WorkSize),
  FUN_10185550(n). Header 32 actor, 6 target count, 4 res_sum, 4 category, 32 cmd_arg, 32 info; per target (at most
  0x40) 32 id, 4 result count; per result (at most 8) 3 miss, 2 kind, 12 animation, 5 info, 5 scale, 17 value,
  10 message, 31 bit, 1 + (6 kind, 4 info, 17 value, 10 message) proc, 1 + (6, 4, 14, 10) react. Identical to LSB's
  GP_SERV_COMMAND_BATTLE2::pack (packBitsBE is LSB-first despite its name); round trip in net/action.rs tests.
- FUN_10186150 / FUN_101861b0 keep 0x34-byte records: one target -> one record per result; several targets -> each
  target's first result only. Record +0xe animation, +0x14 value, +0x18 message, +0x1c bit, +0x24 proc value,
  +0x28 proc message, +0x2e react value, +0x30 react message.
- FUN_1009c730(action, record, which 0/1/2) prints main / additional effect / reaction messages with work =
  [cmd_arg, value, proc value, react value, bit] and the same caster / target relation.
- Category dispatch (FUN_1009caa0): 1, 2 -> actor vtable +0x2b4; 3..6, 0xb, 0xd..0xf -> +0x2c0; 7..10, 0xc ->
  +0x298(cmd_arg). For PCs (vtable 0x10324320) +0x2b4 = FUN_100ccff0 -> +0x298 = FUN_100cce00: runs the type-7
  scheduler named by cmd_arg (melee "atk0") from the actor's files (FUN_100cc440), else globally (FUN_10071510).
- Effects are not shown yet. They load in FUN_100a78c0 (+0x2c0): file = FUN_100a7380(category) + animation id:
  3/14/15 0xd48, 4/8 0xaf0, 5/9 0x1330 (0xdca5 above 99), 6/10 0x113c, 7/11 0xf3c (0xc1ef from 0x200, 0xe739 from
  0x600, 0x14b07 from 0x800), 13 0xc0ef; FUN_10072130 loads it, LAB_100a7640 runs it. Skeleton actors with
  category 3/14/15 go through FUN_100d0570(animation, 0/1/2) instead.

### Combat schedulers and motions
- Type-7 scheduler: op stream at u32 @16, length in ticks u32 @0x1c; ops {u8 op, u8 length/4, u16, u16 wait @4};
  the next op runs after this op's wait (cumulative waits add up to the header length: ati0 24+12+44 = 80).
- Model 0 (file 1300, orc) "atk0": op 03 "hwat" (DAT 0: three op 0x75 = weapon draw schedulers "!w01".., FUN_100d3cc0),
  op 57 "vatk" (op 0x3d..0x3e random voice "atk1".."atk4", op 0x0a = SeSep sound), op 0x24 (FUN_100d4600: pick the
  swing scheduler), op 0x32.
- FUN_100d46f0 picks the swing: 4cc pattern byte 0 'a' (b/c/d by FUN_10185ea0 for multi-hit), byte 1 "t123t123"[flags],
  default 't', byte 2 'i' (monsters) or 'f'/'r'/'b'/'l' (actors with +0xf8 set, PC type), byte 3 any; random among
  the matches (FUN_100cc740 counts them), avoiding a repeat. Orc: ati0 / ati1 / ati2.
- ati0: op 05 "at0?" (motion; '?' = all regions; u16 @6 = 80 ticks, f32 @0x10 speed 1.0), op 03 "aloc" wait 24, op 0a
  "skaz" wait 12, op 03 "dada" wait 44: the hit (DAT 0 "dada": atpr, crtl, op 2b = FUN_101857f0 flushes the action,
  dam0) runs at tick 36. ati1 hits at 56 (106 long), ati2 at 50 (80). at00 is 31 frames at speed 0.75 = 83 ticks.
- Damage reaction FUN_100cbd20 (scheduler op 0x25): "dfi?" (front) / "dbi?" (back) when +0xf8 is clear, "dfm?" /
  "dbm?" when set; guard FUN_100cbe30 "gdi?" / "gdm?"; speed param x 1.0/2.0/.. by hit level; played by FUN_100cb8e0.
- "dead": op 05 "ded?" (u16 @6 0x6f), op 03 "vded" wait 60, op 78 wait 51, op 05 "cor0" at tick 111 (corpse loop).
- 0x038 (GP_SERV_COMMAND_SCHEDULOR @4 caster, @8 target, @0xc 4cc, @0x10 / @0x12 indices) "kesu" before LSB's despawn
  (3 s later, CDespawnState): DAT 0 "kesu" op 29 fades the actor over 90 ticks to colour 0x808080 alpha 0.
- PCs: the race skeleton files (Hume M 7072..7076) and weapon part files (sounds "skaz" "sinr" only) hold no
  "atf?" / "btl" schedulers or motions. Where PC battle motions load from is **not traced**.

### Action menu and engaging
- PC action menu (opened with the confirm key; the PS2 "battlemo"/"attackmo" menus are not referenced by the PC
  client): FUN_10127790 picks a layout from 0x10363648 (8 x 13 bytes, 0x1c ends): engaged (+0x16c == 1) 6, a target
  that FUN_101c26a0 accepts (attackable) 2 * 2 = 4, another target 1 * 2 = 2, none 0 (+1 for chocobo statuses).
  FUN_101275b0 filters each command with FUN_10127230. Windows "menu    actionm1".."actiom10" (0x103636b0) by row count.
- Commands 1..0x1b (0x103634f8 + 12 * c: u16 sprite, u8 library, help id in group 0xe): 1 Attack (keytops3 0x10, help
  31 "Start auto-attack."), 2 Call for help, 3 Disengage (0x16, 32), 4 Chat (1, 22) / 5 Tell, 7 Abilities (4), 8 Magic
  (7), 9 Items (0xa), 0xa Examine / 0xb Check (0x13, 29 / 30), 0xc Treasure, 0xd Fish, 0x12 Switch target (6, 37),
  0x13 Trade, 0x19 Trust (0x1f9, 718). Library 1 (commands 6, 0x10, 0x11, 0x13, 0x14, 0x1a, 0x1b) is not traced.
  combat-shot renders layout 4 as Attack, Magic, Abilities, Trust, Items, Check and layout 6 as Switch Target, Magic,
  Abilities, Trust, Items, Disengage, Check: the decode matches the help texts.
- Row draw FUN_101276a0: highlighted label tinted 0x80805020, others 0x80808080. Row rects (21, 5 + 16 n) 88x16 are
  taken from the PS2 battlemo layout (inferred; actionmN is 8 + 16 n high, consistent).
- "/attack" is key function 0x3b (FUN_101a6110 binds it to the text "/attack"); its handler FUN_10079490 sends 0x01A
  action 2 on the target, 0x0f (change target) when engaged on another target, 4 (disengage) when engaged.
  0x01A layout as in the event entry (FUN_10077120). LSB answers an invalid target with 0x029 446 "You cannot attack
  that target." (mode set by `1f 8d`). Check = c2s 0x0DD {UniqueNo, u32 ActIndex, u8 kind 0}.

### Client (verified with unit tests and offscreen renders; live only partly)
- dat/battle.rs: BattleText (7027 + names + mode / colour tables from FFXiMain), expander; tests expand real messages
  ("Tuxedo hits the Wild Rabbit for 5 points of damage.", "Ding Bats hit Tuxedo ...", "Tuxedo casts Cure." /
  "Tuxedo recovers 12 HP.") and check modes and colours.
- net/action.rs: reader plus LSB's packer; client/combat.rs test replays LSB-built 0x029 / 0x02D / 0x028 sub-packets
  through the client parsers into chat lines and colours. `vana-client combat-shot --out P [--layout 4|6]` renders
  the same replay in the log window with the target window and the action menu.
- Motions (client/combat.rs): melee plays the chosen ati?/atf? scheduler's op-05 motion once and prints the results and
  starts the target's dfi/dbi (or gdi) reaction at the "dada" tick; other categories print at once. Inferred: front /
  back from the attacker's side of the target; the reaction pose (dfi0 has 2 keys) is held 24 ticks. Death (status
  2/3) plays "ded" for the dead scheduler's length, then the existing "cor" loop. "kesu" hides the actor after 90 ticks
  (no fade). npc-shot --model 0 --anim at0 / dfi / ded shows the poses.
- Live (Tuxedo, N. San d'Oria 231): Enter opens the action menu (layout 0: Chat, Magic, Abilities, Trust, Items),
  /attack with no target prints "[no target]" and sends nothing (as FUN_10079490), clean logout. Tuxedo spawns in a
  walled 3-unit pocket at (-59.2, 50.4) and cannot walk out; no NPC within Tab range (30), so no live 0x028 / 0x029.
- Not done: effect files, PC battle stance / swing motions, multi-hit "b/c/d" swing schedulers, party / alliance
  relations, the chat filters, the target cursor for Switch target, the kesu fade itself.

## Live NPC talk, event playback, server zone text, Items / Equipment (client/event.rs, client/items.rs)

Wired into the client and checked live on the test account (new character Lnxtest, Hume M WAR, San d'Oria).
Decompile: ~/ffxi/lab/creatorwork/ghidra/all.c.

### Talk
- Enter on a target (FUN_101524e0): an NPC (UniqueNo high byte 0x01) is talked to at once through FUN_1007aac0;
  otherwise the action menu opens (FUN_10127790). FUN_101524e0 takes the menu path for NPCs with actor +0x1cc
  bit 0x100 (actor +0x12c bit 13) unless +0x12c bit 30. FUN_1008cb80 builds +0x1cc (bit 2 NPC, 0x10 pet-like,
  0x100 from +0x12c bit 13; correction from the Effects filter decode: 0x04 = in your party, 0x10 = monster). The client sends monsters (0x00E @0x25 bit 0x08) down the menu path (inferred).
- +0x12c is taken to be u32 @0x25 of 0x00E (inferred from LSB): bit 30 = byte 0x28 bit 0x40, LSB "triggerable".
- Action menu libraries (FUN_101270a0): +0x14 = FUN_101bf3a0(2) = keytops3 (library 0), +0x18 =
  FUN_101bf3a0(1) = windowps (library 1). Command 6 = Talk (windowps 0x16b, FUN_10127b60 case 5 ->
  FUN_1007aac0). It is in layout 7, chosen for an NPC with +0x1cc bits 2 and 0x100. Command 9 Items (case 8)
  opens FUN_101e9f10(DAT_1061e270); the main menu's Items uses DAT_1061e26c.
- FUN_1007aac0 checks, in order:
  1. no event (DAT_10475440) and no talk lock (DAT_10470520);
  2. not mounted (else group message 0x4b);
  3. both actors exist; an index above 0x6ff needs +0x12c bit 30;
  4. the player has a model (+0x120 bit 9), the target too unless forced;
  5. player +0x128 bit 6 sends a local 0xac message instead;
  6. 3D distance <= 6.0, else a local battle message 9 about ourselves (FUN_100a0150);
  7. the target is not dead.

  Then it sends 0x01A action 0, sets the talk lock unless target +0x128 bit 5, and clears DAT_10473068.
- LSB answers a talk without an event with 0x052 mode 0 (verified live, Ceraule).
- Main menu (FUN_1013b3f0 case 5): child 1 Status (persona), 2 Equipment (FUN_1012ec60), 3 Magic, 4 Items
  (FUN_101e9f10(DAT_1061e26c)), 5 Abilities, 8 Search, 11 Region, 12 Map / Mog House.

### Event start and the login event
- 0x00A: EventNo @0x40 (text table), EventNum @0x62, EventPara @0x64, EventMode @0x66.
- The client starts the login event itself, in its per-frame function FUN_10094ea0: FUN_10120260(1), talk lock 1,
  DAT_10475440 = 1, Mode, FUN_100ad2c0(EventNum, EventPara, EventNo, zone). The target is the player (own
  UniqueNo and ActIndex).
- The old auto-skip in net.rs is gone. The client plays the event once the zone is up and its actors exist (the
  FUN_100ad460 wait, see the cutscene fixes entry; the earlier 1.5 s guess is gone).
- LSB writers (offsets include the header):

  | Packet | Fields |
  |---|---|
  | 0x032 | UniqueNo @4, ActIndex @8, EventNum @0xa, EventPara @0xc, Mode @0xe, EventNum2 @0x10, EventPara2 @0x12 |
  | 0x034 | UniqueNo @4, num[8] @8, ActIndex @0x28, EventNum @0x2a, EventPara @0x2c, Mode @0x2e (8 without flags), EventNum2 @0x30, EventPara2 @0x32 |
  | 0x033 | UniqueNo @4, ActIndex @8, EventNum @0xa, EventPara @0xc, Mode @0xe, String[4][16] @0x10, Data[8] @0x50 |
  | 0x05C | num[8] @4 |
  | 0x052 | u32 Mode @4; mode 2 is or'ed with eventId << 8 |
  | 0x05B / 0x065 (WPOS) | x, y, z @4, UniqueNo @0x10, ActIndex @0x14, Mode @0x16, dir @0x17 |

- FUN_100ad2c0's file zone: a current zone in 1000..0x513 gives current + 1000. Otherwise EventNum when
  EventNum2 == EventNum, else EventNum2 + 1000.
- c2s 0x05B (16-byte body) and 0x05C (28 bytes, Mode 1 at +0x1e) are as in the dialogue entry. LSB answers
  every 0x05B with 0x052 mode 1 (verified: two after the intro end).
- Verified live:
  - The San d'Oria intro (0x1f7, mode 0x83) played start to finish: cameras s043.. from 30834, fdi1 / fdo1 fades,
    narration, the guard scene and the final positions. The server took the end, and the intro did not replay
    on the next login.
  - Endracion (0x3e8): cutscene camera close-up, choice "Your business?", mission accepted.
  - Aravoge, T.K. (0x7ffa with 0x034 params): a choice of three; Escape on a prompt cancels (0x05B option
    0x40000000), and the server released it.

### In-event state (FUN_10094ea0, FUN_10158670 -> FUN_10158a60)
- Player movement (FUN_100954f0) runs only without an event.
- The target/input controller FUN_10151d20 does nothing while DAT_10565d34 +0x74 bit 2 is set (set every frame
  from DAT_10475440).
- **HUD.** On entering an event FUN_10158670 broadcasts 0x800 to the menu registry (0x10366460, 0x2c-byte
  entries, +0x24 flags, +0x28 byte flags).
  - A menu closes unless its +0x28 bit 4 is set and flags 0x400000 is clear.
  - With flags 0x2000000 it closes only when FUN_10085a00(0, 1) is false (EventPara2 bit 1).
  - Dumped from the DLL: menuwind, socialme, playermo (action menu), mp_pmode, inventor, equip, persona, money
    and iteminfo close. targetwi closes unless EventPara2 & 1. logwindo, partywin, query, helpwind and buff
    stay.
  - 0x1000 (event end) reopens the menus with flags 0x1000000 (casttime, iteminfo).
- An event that has been released, with the message window closed, is cancelled by key (0x3e, 0xf):
  FUN_10036dd0, daca = def8 = 1, DAT_1047462c = 1 (option 0x40000000). The client cancels an idle event (all
  threads ended without 0x21) once 0x052 mode 0 has come, without waiting for a key (inferred).
- Ops 0x22 / 0x4e (+0x120 bit 0x20000): FUN_1008cdc0 hides a bound actor while the event runs when the bit is
  set (FUN_10084360) and shows it when clear (FUN_10084320). In the intro the player is hidden during the
  panoramas.
- Narration lines end in `7f 34 9` then `7f 31`. They close after 540 ticks without a key; the s043 camera
  is 600 ticks, with a 60-tick fade. The client closes timed lines by itself and waits for a key only on a
  plain 7f 31 (inferred).
- Fade schedulers fdi1 / fdo1 (30904) have one op 0x0f each: RGBA (128,128,128,0) / (0,0,0,128) over 60 ticks.
  They are drawn as an overlay colour with alpha a / 128 (inferred).

### NPC names (FUN_10093bd0)
- File 0x1a40 + zone below 0x100, 0x150db + zone from 0x100, 0x1055f + zone for 1000..0x513. Zone 230 = 6950.
- 0x20-byte records, count = size >> 5. The name is 24 bytes (the client zeroes +0x18); the u32 server id is
  at +0x1c.
- LSB sends no name for NPCs with an equipped look (the look overwrites @0x34) and a door id for doors, so
  those names come from this list. Event speakers with no entity (Rahal, Curilla in the intro) are named from
  it as well.
- The list covers mobs too (zone 101: "Tunnel Worm" at 0x1065010). LSB sends database names with underscores,
  so the list takes priority over the packet name; the packet name is only used for ids not in the list.

### Server zone text (0x036 / 0x02A)
- 0x036: UniqueNo @4, ActIndex @8, MesNum @0xa, Type @0xc.
- 0x02A: UniqueNo @4, num[4] @8, ActIndex @0x18, MesNum @0x1a, Type @0x1c, Flag @0x1d, String[32] @0x1e.
- Ids are mapped with MsgShift (only zones whose dialogue file matches). num[0..3] fill {num}; names come from
  dat/names.rs.
- **Gaps in the bundled map.** Zone 230 LSB 6439 "Obtained: <item>." is DAT 6432, which is -7, the next
  segment's delta. 6433 and 6434 are both "Obtained {num0} gil", so the generator had no unique anchor. The
  client takes the next segment's delta within 8 ids (inferred rule; unit test text_gap).

### Items and Equipment
- LSB:

  | Packet | Fields |
  |---|---|
  | 0x01C | ItemNum[18] u8 @4 (1 + size) |
  | 0x01E | count u32 @4, container @8, index @9 |
  | 0x01F | count @4, id @8, container @0xa, index @0xb |
  | 0x020 | count @4, price @8, id @0xc, container @0xe, index @0xf |
  | 0x050 | index @4, slot @5, container @6 |

  Gil is inventory slot 0.
- c2s 0x050 (FUN_100e25d0): +4 item index, +5 equip slot, +6 container. Index 0 removes (FUN_1012e430 with no
  item; LSB EquipItem slot 0 = unequip). Verified live: the Onion Sword equipped in Main and removed.
- **c2s 0x061** (FUN_100967e0, u8 @4) asks for the stats (LSB SendLocalPlayerPackets -> 0x061 CLISTATUS). The
  Equipment command FUN_1012ec60 sends it with 1; FUN_10092600 sends it with 0.
- **CLISTATUS:** hpmax @4, mpmax @8, jobs @0xc..@0xf, bp_base[7] @0x14, bp_adj[7] @0x22, atk @0x30, def @0x32,
  def_elem[8] @0x34, su_lv @0x52, ilvl @0x55. Verified: the status panel shows Lv 1 WAR, HP 31/31, STR 8 ..
  CHR 6, Attack 16, Defence 24.
- Equipment grid children run 1 + col * 4 + row.
- While equipping, the item list is drawn at x 314 (inferred from the three positions of the list's vfunc +4).
  Backspace / Delete removes the slot's piece (inferred key).

### Not done / open
- Event motions (tlk0, thk1, sl00 ..) are not in the NPC model files. They come from the 0x5b / 0x66 motion file
  (val@1 + a base by race), which the client does not load yet, so event actors do not gesture.
- Actor alpha fades (0x6c) hide the actor below half alpha instead of fading it.
- Scheduler ops other than 0x04 / 0x0f (actor motions, sounds) are not run. (Flag bit 20 is the follow-camera end
  point, handled since; see the cutscene fixes entry.)
- Item use (c2s 0x037), the iteminfo window, sorting and the other containers.
- Chat text with SJIS quotes (0x8167 / 0x8168) prints '?' (the log line is decoded, then re-encoded).
- Event volume ops reach the mixer (Audio::event_volume), but the live runs had audio off.

## Event motions, actor fades, PC battle motions (dat/motionfile.rs, client/actmotion.rs)

Decompile: ~/ffxi/lab/creatorwork/ghidra/all.c (and all.asm where Ghidra lost register adds); tables read
with ~/ffxi/lab/combat/q.sh. Inferred items are marked *(inf)*.

### Motion files on top of a model (the model's +0x7e4 list)
- FUN_100d22a0(file, region slot, id, kind) appends {file, kind @+0xc, id @+0x14} at the list tail
  (FUN_1002b9c0); FUN_100d2340(kind) drops every entry of a kind. Kinds seen: 0 normal upper body, 1 battle,
  2 event, 5 normal skirt.
- Lookups walk the list from its tail (FUN_1002b9a0 returns the tail, then +8 = previous): the newest file
  wins. FUN_100cb8e0 marks each region a file filled (bit in DAT_1047797c) and later files do not refill
  it, then falls back to the model's own files (vtable +0x400 / +0x1dc). Schedulers: FUN_100cc440, same
  order. Client: actmotion::find / scheduler (newest file first, per chunk name).

### Event motion files (ops 0x5b / 0x66 / 0x5f, FUN_100b3570)
- 0x5b and 0x5f sub 3 / 5: bank 0, FUN_100d00a0, only for model kinds 1, 2, 7, 8 (FUN_10082d10 = actor
  +0x70 -> +0xee). File = val + 0x7d68 (< 0x200), + 0xbfef (< 0x400), + 0xdc19 (< 0x800), + 0xe95b (< 0xc00),
  else + 0x10323. Checked: #417 -> 32521 (sin0 look lklr btle btls), #416 -> 32520 (utah utan hapy),
  #872 -> 50007 (hi00..hi10). These files hold all three regions. Kind 0 is taken to be real players *(inf)*.
- 0x66 and 0x5f sub 4 / 6: bank 1, FUN_100d01b0, model kinds 0, 1, 6; val >= 0x118 only prints an error.
  Blocks of 0x46 (asm 0x100d0211..0x100d0293; the decompile drops the subtractions): v < 0x46: A = 0x7fc8 + v,
  skirt B = 0x800e + v (body info[9] == 1) or 0x8054 + v (else); 0x46..: A 0xef39 + k, B 0xef7f / 0xefc5;
  0x8c..: 0x15711 / 0x15757 / 0x1579d; 0xd2..: 0x18f5f / 0x18fa5 / 0x18feb (k = v - block start). B loads only
  when (char) body info[9] > 0 (vtable +0x3d8, byte +9).
- There is no race in the id: each val is one race's file, and a script carries the val for its actor (by
  the highest bone index: 32712..32719 Hume M (93), 32722.. Hume F (96), 32732..32745 Elvaan (98), 32760..
  Mithra, 32772.. Galka, 32727 / 32746.. Taru). Elvaan NPCs use #20 (32732: tlk0 tlk1 thk1 thk2 pas0 poi?)
  and #21 (32733: sl00 dis0 poi0).
- After loading (the op yields while FUN_100cf260 reports files loading) it calls model vtable +0x298(4cc)
  = FUN_100cce00: scheduler 4cc from the actor's files (FUN_100cc440), else from DAT 0 (FUN_10071510).
  Subs 5 / 6 also pass val@0x10 to vtable +0x2a8 (a scheduler speed *(inf)*; not applied).
- Op 0x53 (FUN_100b3170 -> vtable +0x2a0 = FUN_10083650) is busy while the CMoSchedularTask of that 4cc
  exists (FUN_10056060), i.e. for the scheduler's header length. 0x5f sub 7 asks vtable +0x2ac instead
  (handled as the same wait *(inf)*).
- Unbinding an event actor (FUN_100bb750) drops kind 2 (event files), restores the idle +0x1a0 from +0x1a4
  and clears +0x7d8. 0x5f sub 2 (FUN_100b3870) also drops kind 2.

### Scheduler op 0x05 (motion) and what ends it
- FUN_10057000 case 5 -> FUN_100cb8e0(4cc, f32 @0x10 speed, rate, byte @0x1e & 0x7f, (float) u16 @0x18,
  (float) u16 @0x1c, ...) -> FUN_100cb500 -> FUN_10019830 -> motion queue FUN_10019ed0. Byte @0x1e is the
  play count (motion +0x2c, counted down at each wrap by FUN_10019ab0; 0 = loop for ever). u16 @0x18 is the
  blend-in ticks *(inf from its use as +0x14 and 1 / ticks at +0x18)*; u16 @0x1c is not traced.
- tlk0 (Hume M, 32712): ta1? count 1 for 53 ticks, then ta2? count 0 (loop); header length 306. thk1: th1?
  then a th2? loop. pas0 / dis0: one counted motion.
- When a counted motion ends with nothing queued after it, vtable +0x1fc(1) sets model +0x79c; queueing a
  region-0 motion clears it (vtable +0x1fc(0)). FUN_100c6380 brings the status motion back only when +0x79c
  is set or the wanted name differs from +0x7d8 (model[0x1f6]). So a looping event motion (ta2?) holds after
  its scheduler until the status motion changes, or until +0x7d8 is cleared to four spaces: op 0x5e / 0x6b
  (FUN_100b53b0 sets +0x1a0 / +0x7c8 and clears +0x7d8), the engage states, event unbind. Scripts follow
  talk motions with set_idle_motion 'idl0' for this.
- Client: Play (actmotion.rs) runs the op-05 motions at their start ticks, is busy for the header length,
  and holds a count-0 last motion until the status motion changes or an idle op cuts it.

### Actor alpha (op 0x6c, FUN_100b5800; DAT 0 kesu)
- First call: start = model colour (vtable +0x2c) high byte, target u8 val@5, frames val@7 (0 -> 1), step =
  (target - start) / frames. Each frame: frames -= ticks; while >= 0, alpha += ticks * step and the colour
  (vtable +0x30) = alpha << 24 | rgb, yield; then the target exactly, and the op advances.
- Client: Actor.alpha (0x80 = opaque) goes into the vertex alpha and the actor pipeline alpha-blends
  (SRCALPHA / INVSRCALPHA *(inf)*: the actor pass's D3D blend states were not traced). kesu fades over 90
  ticks (DAT 0 op 0x29) instead of a hide. Actors an event faded are made opaque when it ends *(inf)*.

### PC battle motions (FUN_100cf650 / FUN_100cf900, FUN_100d0dc0 / FUN_100d0d00)
- Actor state machine (around 0x10089e00, state byte +0x1b8): status +0x16c == 1 loads the battle files
  (FUN_100cf650, or FUN_100cf900 with dual wield +0x8b5) and calls vtable +0x160; state 1 plays scheduler
  "in N"; state 2 vtable +0x31c(1, 0) (weapon to the hand *(inf)*); state 3 waits for status != 1, plays
  "bntg", reloads the normal files (FUN_100cf450); state 6 plays "klbt", "out N", "hwat", vtable
  +0x31c(0, 0); state 7 waits while "waot" plays, then FUN_100d2340(1) drops the battle files and plays
  "res0"; state 8 "res2". N = main weapon info[4] (+0x87c; FUN_100d0e60 / FUN_100d0e80).
- While engaged FUN_100d0dc0(race, 0) = table 0x10352334[race] + main info[3] (+0x87b): upper-body file
  9672 Hume M, 12848 Hume F, 16024 Elvaan M, 19200 Elvaan F, 22376 Taru, 25776 Mithra, 28952 Galka
  (skeleton + 2600). Skirt: FUN_100d0d00(race, 1, 0) = 0x10352348[race * 2 + max(body info[9] - 1, 0)] +
  info[3]: 9800 / 9928, 12976 / 13104, 16152 / 16280, 19328 / 19456, 22504 / 22632, 25904 / 26032,
  29080 / 29208. Dual wield loads 0x103523a4 (40815..) + info[3] and 0x1035236c (40431..) + sub info[3]
  (+0x88a) with skirts 0x103523b8 / 0x10352380 instead (not implemented).
- Weapon motion set = main weapon file 0x45 info[3] (FUN_100cea10 case 6): 8392 (main 0) 10 = hand-to-hand,
  8393 (main 1) 2, 8394 / 8395 3, 8396 / 8397 1, 8400 4, 8410 5. A battle file holds btl0/1, wlk1, run1,
  at00..at21, dfm1 / dbm1, gdm1, the swing schedulers ati0..2 atf0 atb0 atr0 atl0, "in 0" / "out0", atk0;
  its skirt file btl2, at02.., dfi6 dbi6.
- Correction to the combat entry: actor +0xf8 is not "PC type". It is set each frame the actor moved more
  than a threshold (store at 0x100a4248). FUN_100d46f0: standing -> a random "ati?" (as monsters), moving ->
  "atf?" / "atr?" / "atb?" / "atl?" by the move direction +0x598 (FUN_100a6760: 2 right, 3 back, 4 left,
  else f), first match. Reactions likewise: dfi / dbi standing, dfm / dbm moving (upper body only). atf0
  plays amb? (upper body), so the legs keep running.
- LSB keeps look.main when the main weapon is unequipped (charutils UnequipItem, SLOT_MAIN case), so
  Lnxtest (Onion Sword removed, fighting hand-to-hand) still shows main model 1 and gets the sword set 9674.

### Verified
- Unit tests against the DATs: dat::motionfile (bank 0 / 1 ids, Elvaan bones, battle files and their
  schedulers, the sword ati0 plan), client::actmotion (tlk0 plan, loop hold and cut, fade steps), dat::sched
  (count / blend fields).
- char-shot --motions: Hume M ta2 (talk), btl / at0 from 9682 + 9810, Elvaan th1 (hand on chin) from
  32741 + 32811, all posed correctly (scratchpad mo/sheet1.png, mo/elv_th1.png).
- Live (Lnxtest, zone 230): talking to Luthiaque (0x10e6021, event 0x292) loads 32732 + 32872 and plays
  tlk0: the arm goes out during the line and back to idle after (mo/crop_t7.png from t7_1..t7_7).
- Live (zone 101): engaging a Wild Rabbit loads [9674, 9930]; Lnxtest holds the fists-up battle stance and
  runs in it while chasing, combat lines print (mo/sheet_b1a.png, mo/crop_b1.png). The swings were not
  caught clearly (Lnxtest kept moving, so the upper-body atf0 applies). That run ended when the Thunderbolt
  eGPU dropped off the bus (hardware, 09:32:34).
- Live again (zone 101, Tunnel Worm, killed): standing swings show as a wind-up and a full-body lunge between
  battle-stance frames, then after the kill the disengage drops back to the idle pose (mo/crop_b3.png:
  b3_15 / b3_16 / b3_20 swings, b3_21 / b3_25 stance, b3_41..45 idle; mo/sheet_b3.png).

### Weapons and motion lookup (traced by a sub-agent, 2026-10-22)
- Motion lookup FUN_100cb500: chunks matching the name's first 3 letters (FUN_10073300(.., 0x2b, name & 0xffffff,
  0xffffff)), each played in the region its 4th character names (atoi of chunk name +0x23, regions 0..6), one per
  region (bitmask DAT_1047797c). "idl0" plays idl0, idl1 and idl2. Ours matched the exact 4cc first, so an event
  `idle 'idl0'` posed the legs only and every upper bone fell back to the rest pose (Trion's sword carry bones
  83 / 95 rest at the model origin: the sword lay on the floor). Fixed in actmotion::find / find_animations.
- Weapon meshes are named wepN (N 0..9) and rigidly skinned to one carry bone (PC Elvaan main 1, file 14745: wep0 ->
  bone 77 = skeleton point 112 = main info[6] 0x70); points 112..125 are carry bones, 126 (bone 66) / 127 (bone 52)
  the hand grips *(inf: which is which hand)*. Motions key the carry bones (the sheathed place).
- Visibility: each mesh object has 4 hide bytes at +0xc and draws only while all are 0 (draw loop all.c 29167).
  Scheduler op 0x75 (case at 0x1005ae64): u32 @8 1 hide / 0 show on byte 0 of every wepN, N = i16 @0xc; duration
  short @6 x speed runs FUN_100609b0 (hide at once, show at the end if @8 == 0). Op 0xa3 the same on byte 1; for
  N 0 / 1 FUN_100d3d90 / 3f30 play the "!w01" / "!w11" and "!w02" / "!w12" effect schedulers. FUN_100d3cc0 / 3e90.
- FUN_1008ad70 plays a model's "init" scheduler once loaded. Trion (model 64, file 1364, its own 99-bone elv_
  skeleton): init = op 0x3b -> "wof4" = op 0x75 N 4 hide, so in retail his sword and scabbard are hidden; event 0x1f4
  never plays "won4". Ours follows init's 0x3b calls for 0x75 / 0xa3 and leaves hidden wepN parts out of the model
  (dat/npc.rs; test trion_init_hides_the_sword). Not done: per-actor hide bytes, so a later op 0x75 show (won4) or
  timed hide does nothing.
- Draw / sheathe: vtable +0x31c = FUN_100a30c0(bool, slot) sets model +0x83c + slot (+0x320 reads both; override
  pair +0x83e via +0x324 / +0x328, scheduler ops 0xa7 / 0xa8, timed FUN_10060e50). FUN_10029320 (pose setup) remaps
  when drawn: bone(point 127) -> bone(main info[6], +0x87e), bone(point 126) -> bone(sub info[6], +0x888);
  FUN_100339b0 pass 0 skips the carry subtree, pass 1 sets world[carry] = world[grip] *(inf: the copy)* and re-chains
  its children. Default sheathed (model reset all.c 145748 clears +0x31c / +0x324; init all.c 146831 sets +0x31c(1, 0)
  only for engaged or flagged actors). Not done in ours.
- Track scale channels (Trion's 83 / 95 have 1.075) are ignored by our posing; FUN_10019d30 not checked.

### Open
- Weapon attach on draw / sheathe (vtable +0x31c; see "Weapons and motion lookup"), the bntg / klbt / hwat / res0 schedulers, dual-wield files,
  multi-hit swing letters, the move direction for atr / atb / atl (moving always picks f).
- Scheduler ops other than 0x05 in motion schedulers (sounds 0x57 / 0x0a, 0x20 / 0x2f / 0x59).
- The actor pass's real blend states for faded actors.
- Test-drive tools added: zone-path ZONE --from x,y,z --to x,z (A* with the client's walk collision),
  play --chase (walk to the target while engaged), char-shot --motions ID...

### 0x00E update mask (LSB entity_update.cpp)
- Bits: 0x01 position, 0x02 claim, 0x04 HP, 0x08 name, 0x10 model, 0x20 despawn. Fields outside the set bits are zero.
- 0x1C speed / 0x1D animation speed come only with 0x01. 0x1E HP% and 0x1F status (animation) only with 0x04. 0x20 status is always written.
- The look at 0x30 is written on every update, but spawns set 0x10 only for equipped/chocobo looks (mask 0x57). Monsters and plain-model NPCs spawn with 0x0F, so the client reads the model from any non-despawn update.
- 3520 LSB NPCs use model 50 (0x32), 182 model 52: stub meshes a few millimetres tall, i.e. invisible helpers. They count as having a model for Talk (FUN_1007aac0), but their name plates show only while targeted.

### Keymap (DAT_103619f0, tested by FUN_1011ff30)
- Three u16 bindings per key function id at 0x103619f0 + id * 6. Low byte = DirectInput key, high byte = modifiers:
  0x01 Shift, 0x02 Ctrl, 0x04 Alt, 0x08 Ctrl+Alt, 0x10 (Ctrl or Alt) and gate DAT_103612e4 (1 at load, cleared by text-entry windows), 0x20 no Shift, 0x40 (u16 0x4000) not while typing, 0x80 neither Ctrl nor Alt.
- Shortcuts registered by FUN_101a5e80(id, handler, command, n), all bound to letter + flag 0x10 (Ctrl or Alt):
  E 0x36, R 0x37, T 0x38 /tell, I 0x39, P 0x3a /party, A 0x3b /attack, S 0x3c, H 0x3d /heal, L 0x3e /linkshell, C 0x3f, M 0x41, J 0x7f, W 0x80, B 0x9f, U 0xad /unity, X 0xae /linkshell2; 0x81 /shoot <t>, 0x82 /help.
- 0x3b (LAB_101a6700) sends the command text through FUN_1007f6d0, like typing /attack (toggles engage).
- 0x3f (LAB_101a6770) calls FUN_10130280(0, 1, 0): talk when the target is an NPC (+0x13c bit 0x400), otherwise check (rate limited).
- Handlers:
  - Mode 1 (run the command): A `/attack`, H `/heal` (LAB_101a6720; c2s 0x0E8 Mode u32 @4, 0 toggle).
  - Mode 2 (LAB_101a64c0, allowed while typing): S/T/P/L/U/X open the input as "%s%s" of `/say ` .. `/linkshell2 ` and the typed text. FUN_101a6550 first strips leading spaces and a channel command from table 0x1036ec68 (/say /s /tell /t /shout /sh /party /p /linkshell /l /linkshell2 /l2 /unity /u), plus the name after /tell.
  - R (LAB_101a6820): "/tell <name> <text>" from the tell history FUN_10186c50(i); pressing it again while typing takes the next name.
  - Mode 4: E equipment (FUN_1012ec60), I items (FUN_101e9f10), C check/talk, M magic (FUN_10221370(0,1,0x100)), J/W/B ability lists FUN_10218df0(1 / 3 or 0x14 with Monstrosity / 2).
- Implemented: all except M/J/W/B (no magic or ability windows yet). Movement keys are not taken while Ctrl or Alt is held (flag 0x80).
- Chat: c2s 0x0B5 {Kind, 0, Str} with kind 0 say, 1 shout, 4 party, 5 linkshell, 0x1a yell, 0x1b linkshell 2, 0x21 unity; c2s 0x0B6 tell {2 bytes 0, name[15], text}. s2c 0x017 kind 3 is a tell. The log's punctuation ("(Name)", "<Name>", "Name>>") is inferred.

### Input layer (client/input.rs; full spec ~/ffxi/lab/research/keys_spec.md)
- Tables read from FFXiMain at start: bindings 0x103619f0 (0xb4 ids x 3 u16), context masks 0x103612e8, compact records
  0x103623c8 (42, until zero bits), k1 rows 0x1036e0a8 (28 rows of record indices, 0xff ends), k2 rows 0x1036e190 (28 x 5
  ids, 0xffff ends). Pad callback bits per id are code, so they are a table in input.rs (kspec dump.txt).
- Key state FUN_10005520 (object FUN_10005210): a key going down gets held | press | repeat (bits 4 | 2 | 1) and counter 0;
  held, the frame bits clear (& 0xf4) and the counter adds ftol(FUN_10014000()) (whole ticks, at least 1); past input
  +0x94 = 16 it sets repeat and drops to 16 - 2.0 (_DAT_1031e800). Up: 0, or release (8) after a hold. Retail's frame was 2
  ticks, so a held key repeats after 18 ticks, then every 4. Ours counts in 2-tick frames whatever the frame rate *(inf: at
  60 fps retail repeats every 3 ticks; 30 fps was its normal rate)*. A press and release inside one frame still presses.
- Typing: Enter / NumEnter / Esc presses are cleared before key functions (FUN_101aa930 takes them).
- FUN_1011ff30: fails when DAT_104cf22c (FUN_101202a0, cleared each frame by FUN_10120290) and mode != 4, when the mask
  misses DAT_10362678, or mask & 0x41 while typing; then pad callback 0, then the three bindings with their flags
  (FUN_101201c0 Ctrl = 0x1d / 0x9d held, FUN_101201f0 Alt = 0x38 / 0xb8, FUN_10120220 Shift *(inf: 0x2a / 0x36)*; flag 0x10
  also needs DAT_103612e4, cleared only by Key Assignment). Bindings on 0x1d / 0x2a / 0x38 answer only in mode 4.
- FUN_1011f850: with typing and NumLock (toggle bit 0x10 of DIK 0x45) the numpad digits fail. Ours takes NumLock from the
  last numpad digit's logical key (winit has no toggle state).
- Pad (DAT_104cf334): +0x220 held, +0x228 edge, +0x238 press-or-repeat, +0x230 release. Compact bits (FUN_1011fbc0) OR into
  held; newly held bits set edge and repeat; an up / down edge drops 0x30 (left / right). No edge and something held: +0x248
  adds the ticks, the first such frame only arms (+0xda8), then past 16 repeat |= held and 14. Callback modes 2 / 3 keep only
  their own bit in the edges (FUN_1011da00).
- Compact record rule (FUN_1011fbc0, rewritten): no Ctrl / Alt: rule 0, 7, 4 with Shift, 8 without; else rule 1 Ctrl only,
  2 Alt only, 7, or 0 when the key is no k2 row's key (FUN_101953b0). State mask & (6 - window active) == mask.
- Key Assignment: FUN_10195590 sets slot 0 of each k2 row's ids to 0x10 << 8 | key (FUN_1011fb40, 0 clears), then
  FUN_10195940 the k1 rows' record keys (FUN_1011fb10), then with keyboard type 2 FUN_10195910 sets 0xaa / 0xa9 to
  0x80 << 8 | k1 row 28 / 29 (FUN_1011fb70).
- Wired so far: 0x44 (Num*, compact H) and 0x78 (Num+, Ctrl / Alt + F, compact F; mode 0). Drives tap keys (egui events,
  act:numpad* / numpad+, act:dik:XX; act:down:XX / act:up:XX hold).
- Axes (FUN_1011fe30, Input::axis): the larger of the raw keys (FUN_1011f300 -> FUN_10120550: n1 Num6 +1 / Num4 -1, n2
  Num8 -1 / Num2 +1, n3 Right / Left, n4 Down +1 / Up -1) and the pad bytes (b - 0x80) / 128 that FUN_1011fbc0 writes from
  the compact records (+0x252 D 0xff / A 0, +0x253 W 0 / S 0xff, +0x250 L 0xff / J 0, +0x251 I 0 / K 0xff; the later
  record wins). Typing: numpad movement only with NumLock off, arrows never; in a window or outside the field the arrows
  need Shift (FUN_1011f7b0 / FUN_1011f910).

### Movement keys (FUN_100a48d0; client mod.rs update_movement)
- Speed FUN_100a5f60: |v| > 0.9 full, 0.05..0.9 a third, below nothing; walk (player +0xfa) caps at a third. Speed
  = vt+0x1a0 (max 30, _DAT_1031ee48) x 1/60 a tick.
- Walk 0x45 (Num/, Ctrl / Alt + Z, compact Z) toggles DAT_10565d34 +0x50 (FUN_10151910); drawn with the walk motion
  (ratio 1/3, FUN_100c6b70).
- Autorun 0x1c (Num7, compact R): FUN_100a4700 toggles DAT_10475579 and zeroes the vector DAT_1047555c. While on and the
  vector is zero the move input becomes it (LAB_100a53ae); X input rotates it by 2 x X x ticks x 0.0174528 (local_9c =
  DAT_10443fe4 +0x2c x _DAT_1031f864, asm ~0x100a5360) and the move follows it; no input and |vector| <= 0.001 ends it,
  so Num7 standing still does nothing. With Q / E turning while on, forward is forced and the turn acts as X input.
  Forced off in statuses 2, 3, 6, 7, 0x21, 0x26..0x2c, 0x2f, 0x30, 0x32..0x35, 0x38..0x53 and events.
- Q / E (0xaa / 0xa9, keyboard type 2, free camera, X input 0, camera axes idle): player yaw += ticks x 0.0174528 x
  1.57075 x (E - Q) (_DAT_10322260 = 1.57075; 94 degrees a second); the camera turns with it (FUN_1001d4c0).
- Face-camera 0x9c (LShift held, type 2, free): the player faces the camera yaw, so A / D strafe; camera axes and Q / E off.

### Free camera (FUN_1001e030 free branch; client/camera.rs; traced by a sub-agent from all.c 22024..22887)
- State: eye (camera +0x44) and look (+0x50) only; +0x48 is the eye's y. One state serves the free, locked and first
  person views (ours: World::cam). Per frame the free branch loops ftol(ticks) whole ticks; ours steps 1-tick states
  with an accumulator and draws between the last two.
- Look eases 0.25 a tick toward the player's position minus 0.6 (_DAT_1031e8a0) x model height (bounds span, at most
  2.5 with vt+0x14c). Ours uses the posed model's top *(inf: bounds)*.
- Axis 6: the eye turns about the look by ticks x v x 0.0279244 (_DAT_1031f25c; FUN_1001dd80, angle atan2(dz, dx), the
  opposite sense to our yaw); axis 7: eye.y += ticks x v x 0.1066667 (_DAT_1031f254). Config 0x8b / 0x8c invert. Input
  sets the manual hold DAT_1044445c = 10; the unlock hold _DAT_10444464 (20) blocks input and the spring.
- Spring per tick: d = |look - eye|; d > 100: eye += unit x (d - 100) x 0.5; d >= 6 (_DAT_1031f258): eye += (look -
  eye) x (d - 6) x 0.012 (_DAT_1031f234); d < 3 (_DAT_1031eba8): pushed out by (3 - d) x 0.125 and horizontally to 3
  (_DAT_1031f22c), hold 10. Rest anywhere 3..6. Height memory: eye above the look and the gap shrinking (uphill), the
  eye keeps the previous gap (_DAT_10444460).
- Chase (config 0x91): after camera / turn input wait (hold 4) until the player has moved 0.5; then while the eye is
  -0.5 < dot < 0.99 off the player's back, eye.xz += (look + d behind - eye) x 0.025 a tick.
- Zoom: 0x4f / 0x50 held change focal by ticks x 6 (242..900), saved as config 0x49; both held ease to 350 by 25 % a
  frame (DAT_1044446c). Type 2: FUN_10253af0 *(inf: wheel)* zooms too. Ours: the wheel zooms 12 a notch (user's choice:
  FOV, not distance).
- Home 0x51: 8 frames (ours 16 ticks) of eye += (3 behind the player at look height - eye) x 0.125 a tick, until the
  camera's yaw is within 0.05 of the player's.
- Collision: ray look -> eye puts the eye 0.5 in front of a hit; 0.17 clear of the floor (FUN_1017a310, within 10) and
  ceiling; a sweep from the frame's start eye stops 0.2 short (never through walls); side rays at +-0.17 along the
  camera's X. Ours: no ceiling, no side rays, no path mode (DAT_10444458: the camera retraces the look's path when the
  view keeps being blocked *(inf)*).
- First person (DAT_10475578, 0x10: Num5, Ctrl / Alt + V, compact V; SE 0014): eye 0.5 behind the actor on its facing
  at 0.91 x joint 12's height (max 2; resting and seated: joint 6); the view where the stick points, eased 0.25 a frame,
  max 1.413675 rad pitch / 1.8849 yaw off the body; look = eye + R(yaw, pitch)(5, 0.25, 0); eye and look ease 0.125 a
  tick; focal 280; hold 8. Our own model fades out at 0.0625 a tick (actor +0x167 -= ticks x _DAT_1031f860, FUN at
  all.c ~146770) *(inf: back in at the same rate)*.
- Face-camera: the eye = look + the offset stored when Shift went down; no axes or spring.
- Ours, not retail: face() (zone-in, event end, drives) puts the eye 5 back and 1 up, in front of what the look -> eye
  ray hits *(inf)*; the sweep is skipped when the look cannot see the start eye. Without both an eye placed inside the
  hill behind a zone-in point stayed underground for good (the sweep undid every move out).
- Gone from ours: the yaw / pitch / distance orbit, the release camera and its hand-over. Mouse drag (not retail) turns
  the eye 0.008 rad / px and raises it 0.03 / px, with the manual hold.

### Installed DAT packs (2026-10-21)
- AshenbubsHD Prime (November 2021) and remapster-dats-pack 1.2 are installed over the game DATs. 32482 originals (6.1 GB) are in `~/ffxi/dat-backup/original` (same ROM*/dir/file layout); `~/ffxi/dat-backup/manifest.tsv` lists every file (replaced / added); `restore_originals.py` beside it undoes the install.
- 1591 pack files are at paths this install's VTABLE/FTABLE never reference (newer retail content), so nothing loads them until the game files are updated.
- Neither pack touches the UI, font or message files (39541, 6950 are original). Zone 101 (ROM/0/121.DAT) and character models are pack files; all tests and offscreen renders pass on them.

## Creator screen: script text, help line, backdrop camera, shadow, size, nation flyover, CPU (2026-10-21)

Decompile: ~/ffxi/lab/creatorwork/ghidra/all.c / all.asm. Offscreen check: `creator-shot --screen` (whole screen: zone,
dancer, 2D layer; `--step 0..7 --nation --name --help-time --bench N`). Inferred items are marked *(inf)*.

### Handlers and windows (menu registry 0x10366460: {name[32], handler, flags})
- chdummy -> DAT_10623a48 (flags 0x10167); lobyhelp -> DAT_10623ae8 (vtable 0x1032a090, draw FUN_10236b40);
  chmkname -> DAT_10623a34 (vtable 0x10331ab0, draw FUN_1022d9b0); race1..8 -> DAT_10623a20, nation1..3 ->
  DAT_10623a44, both flags 0x23. Flag 0x20 = no window frame (FUN_101144b0 skips FUN_1022a290 for it); such windows
  draw their sprite whole *(inf: all cells)*.
- Layouts name the library "menu    lobbyps2"; FUN_10118090 renames it to lobbywin. Our lookups missed it, so every
  lobbyps2 ref drew nothing: the race titles, nation emblems and the chmkname bar (sprite 74, whose cells 1..7 are
  the hfr1 header lines and the menu2fon "Name" caption). dat/ui.rs lang_lib maps it now.
- Race title: FUN_1022cd80(race) opens "menu    race1".."race8" (e.g. 303,16, anchor right; lobbywin sprite 3 + race:
  name and gender sign in chmkfnt). Closed with -1 on the nation step.

### Script text (FUN_102306d0, drawn by lobyhelp while +0x14 == 1, i.e. race .. name steps)
- Origin FUN_10159920 mode 0 = (x1 * s/2, y1 * s/2) + p with s = [ECX+0x84]; mode 3 is (1 - s/2) x1 - 512 + p and
  gives the known W - 512 offset only with s = 0, so the origin is (0, 0) *(inf from that)*.
- Sprite 11 cells 0..0x13 (STR..CHR labels) at (0x38, 0xd8); race name = group 6 id race at (0x28, 0xb8);
  "Face %c " (group 1 id 5, face + '1') at (0x1e, 0x156), then +10 "Hair %s " (id 6 with id 0xb / 0xc = A / B),
  then +10 "Size %s " (id 7 with ids 8 / 9 / 10 = S / M / L); stats "%4d" of table 0x1037bb10[(race * 6 + job) * 7 + i]
  at (0x54, 0xd8 + 16 i). Checked against retail: Hume F job 0 = 8 7 6 7 5 5 6, Taru M job 1 = 6 7 8 6 7 6 6.
- Glyphs FUN_102308d0 (lobbywin sprites, chmkfnt texture): '0'-'9' -> 12.., 'A'-'Z' -> 22.., 'a'-'z' -> 48..; SJIS
  pairs with second byte 0xBE / C3 / C7 / CE / D5 / C8 -> 0xa0..0xa4, 0xa7; other bytes advance 6; advance = sprite x extent.

### Help line (lobyhelp FUN_10236b40)
- Text = table[+0x2c]: creator steps set +0x2c = step + 0x13 -> group 8 0x6b..0x6f (race..job), group 0x1b id 4
  (name: "Please enter a name (or press the numeric keypad's + key ..."), nation step 0x1a -> group 8 0x72
  (0x1f -> 0x77 when DAT_10623ac0+0x24 != 1, not understood). The old "Enter a name for your character." was invented.
- Drawn at window y + 0xa0 (the help bar text slot). Width <= W - 16: centred. Else the string becomes four SJIS
  0x8140 spaces + text (format 0x1037c038), waits 0x1e frames after a message change, then moves 2 px a frame, drawn at
  -offset and at width - offset, wrapping at its width. Frames counted at 30 per second *(inf: the default frame rate)*.
- XISTRING (file 48 = group 8): the u32 length's high half carries flags (0x10000 on the %-code strings); dat/msg.rs
  masks it *(inf)*. Before, group 8 failed to load ("string out of range").

### Name window (FUN_1022d9b0)
- Name at window + (0x10, 4) via the text input FUN_101bc850, caret on when frame % 30 > 15; FUN_1022dc60 makes the
  first letter upper case and the rest lower. While the window is active, lobbywin sprite 140 (0x230 / 4) at child 2:
  the slant line, chmkfnt "Namingway" and font cells "Press the + Key to / generate a random name." (the PC text).
  The random name: see "Random names" below.

### Backdrop camera = the dancer camera (FUN_102451f0, every dancer update)
- Game view = dancer camera node matrix A (row-major, translation [3] [7] [11]) rebuilt as D3D rows
  [A0 A1 A2 0], [-A4 -A5 -A6 0], [-A8 -A9 -A10 0], [-0.1 p, 1] with p = A^T t (0x1037ca2c = 0.1); projection [0], [5]
  from the dancer's (DAT_10957c70 +0x2ec / +0x300). So dancer (x, y, z) = 10 x zone (x, -y, -z): the dancer origin
  is the zone-133 origin and the character stands on its ground; the zone moves with every performance camera.
  The start camera FUN_1001d930 / 950: eye (0, -5, 40), target (0, 0, 0) (shown before the first dancer loads).

### Ground shadow (FUN_10297e20, from the dancer pass; traced by a sub-agent)
- Planar: shadow group type 4 on light 4 (FUN_10268300(4)); every shape with shapeFlags 0x40 casts (all creator
  shapes). Light 4 is not camera-attached: L = (0.30902, 0.95106, 0) (rotation (0, pi/2, 1.25664)). Shadow matrix
  FUN_1028dc40 with the plane y = 0 (gx+0x12c): p' = (x - y Lx/Ly, 0, z - y Lz/Ly). Untextured, colour 1 - c with
  c = background sun colour / 4, blend ZERO / SRCCOLOR, no Z test, stencil EQUAL 0 / INCR so a pixel darkens once; drawn
  before the model. Ours pins its depth near the far plane with LESS instead of the stencil *(equivalent, inf)*.

### Size and body scale
- FUN_102460b0 stores the size at scene+0x18 and scales the body (item +0x2a) by 0x1037cc80[race * 8 + face]; the
  body channels' translation gets 1 / scale (FUN_10246aa0), so the body scales about its root; the head item is not
  scaled. Hume M faces 3 / 4-6 = 0.96 / 1.04, Hume F 0.96 on faces 0, 3, 7, Elvaan M face 6 0.96, Elvaan F 0.96 on
  faces 0, 1, 3, 4; others 1.0. Size S/M/L never changes the preview (retail too). Sent: 0x21 body byte 57 (LSB
  login_helpers ref<uint8>(buf, 57)); unit test net::lobby::create_tests.

### Nation step (traced by a sub-agent, then implemented)
- FUN_1022ea40 (chdummy, mode 1 on the town step): 10-frame settle after the highlight changes, 'ffot' of file
  33193 (op 0x0f to black over 60 ticks), FUN_1023ade0 / FUN_1023ae20(n + 1) -> file 0x17 + n + 1 = 24 / 25 / 26,
  type-7 "main" (op 0x7b zone 231 / 235 / 239) -> "loop" -> mov1 for ever, then 'ffin' back over 60 ticks. No op
  0x7e: lighting from the zone's weather at the scripted clock. No crossfade between nations; music 0x78 stays.
- Text FUN_1022e7b0, every frame from the cursor: group 8 title 0x1d / 0x28 / 0x33, body 0x1e / 0x29 / 0x34; body block
  centred in (0, 0, W, H) (lines left-aligned, 16 + 4 px), title centred with its top at H/2 - h/2 - 24.
- Client: flyover::FlyoverData::load_file(24 + n, "mov1"), Flyover::unfaded (no fade op in these movies), restart
  when mov1 ends; the black fade is drawn under the 2D layer. Kept behind the confirm (FUN_10233b50).
  Not modelled: op 0x51 screen flashes.

### CPU (creator race step, live, 1280x800 window on a 144 Hz monitor; main and render threads)
- Before: ~100 % of one core. The pose upload was the cost: 56580 per-corner vertices (2.5 MB) a frame through
  mapped_at_creation buffers took 33 ms (16 ms with queue.write_buffer; this GPU's mapped memory writes at ~250 MB/s),
  plus per-frame skinning, texgen, bind groups and buffers, all at the monitor rate.
- Now: pose (skin, texgen, upload) only when the 30 fps performance frame changes; bind-pose tangents and inverse binds
  cached (SkinPrep); passes per vertex and drawn indexed (11708 vertices, colours 8-bit), one vertex / index / uniform
  buffer through a StagingBelt, texture bind groups cached; the creator screen redraws at 30 fps (WaitUntil in
  about_to_wait) *(inf: 30 fps = the client default)*. After: 12.9 % (race step), ~17 % with the nation flyover.
- Test runs use FFXI_AUDIO_DEVICE=alsa:null: cpal's thread spins at ~100 % on that device; excluded from the numbers.

## HUD, targeting, lock-on, movement, GPU-side rendering (2026-10-21)

Decompile: ~/ffxi/lab/creatorwork/ghidra/all.c / all.asm. Research notes with addresses: scratchpad hud_research.md (items
1..7) plus sub-agent reports summarised here. Inferred items are marked *(inf)*. Live checks on lnxtest01 / Lnxtest
(zones 230 and 101); screenshots in scratchpad hud/h4_*.png.

### Keys
- Numpad * (DIK 0x37): key functions 0x44 and 0x30 read the same press (FUN_1011ff30 mode 1 does not clear it).
  FUN_101516d0: no target (DAT_10565d34 +0x20 == 0) or own status 0x21 -> SE "0002" and the text command "/heal";
  else FUN_10151910 toggles the lock (FUN_10150da0). Skipped while an event holds the player (FUN_10085990).
  egui has no numpad-multiply key: it comes from the winit KeyCode (script step `act:numpad*` for drives).
- /heal (cmd 0x30, FUN_1007adb0): only with status 0, 7 or 0x21; c2s 0x0E8 {u32 Mode}: "on" 1, "off" 2, else 0.
- "/" (VK 0xBF) opens the chat input from the input object's WM_KEYDOWN (FUN_101aa930 -> FUN_101ac1a0), and the
  WM_CHAR that follows types the '/'. egui reports numpad / as Key::Slash too; the last winit key decides.
- Escape in events (key function 0x0f): only while DAT_10473068 allows it. Ops 0x2e / 0x42 set DAT_10475cb0 (and
  DAT_10473068 once loaded, DAT_104760f8); load end copies cb0 in (FUN_100ad000); 0x052 mode 0 sets it unless
  (DAT_1046df04 | event mode) & 0x2000, and op 0x38 sets DAT_1046df04 = val | 0x2000. No window: SE 0003 and abort
  (0x05B EndPara 0x40000000); prompt: cancel only when allowed; choice: allowed -> 0xff + SE 0003, else SE 0004.
  0x1f7 (San d'Oria opening) runs 0x42 first and never 0x2e (unit test client::event::opening_not_cancellable).
  Live: Arpetion's 0x294 has no 0x42, Escape on its prompt cancelled it and the server released it.

### Chat log (logwindo, FUN_1015b250 family) and input line
- Text width = clamp((W - 512) * pct / 100 + 352, 352, W - 160), pct config 0x9c = 100; frame x 16, width text + 14,
  bottom H - 16, height 134 + (lines - 8) * 16. Each new wrapped line adds 300 ticks (config 0x11 = 5 s) capped at
  8 lines (config 0x0f), the count drops one line per 5 s down to 0 (config 0x10), Escape (key 0x95) takes one off.
  Kept open while typing only while at 2 lines or fewer (open ticks <= 2 periods, asm 0x1016280f; corrected
  2026-10-23), and during events by FUN_1015ba60's rule (see "Messages" below).
- Frame height eases 8 px a tick while more than 16 px off, else 2 px *(inf: per 1/60 tick, retail per frame)*.
- Lines 16 px apart, newest at y1 - 20, clipped to [x0, y0 + 3, x1, y1 - 4] (UiBatch now carries scissor rects);
  slide-in 16 px over 20 ticks; with the input open the text rect loses 16 px at the bottom.
- Caption: windowps sprite by default chat mode (276 "Say") at (x0 + textw - 352, y0) while lines > 0.
- msgline: (16, H - 38), (textw + 14) x 22, windowps 37, text at +2 / +2 *(inf: +2 more for the caret gap)*.
- Lines are wrapped once when they arrive (they used to be re-wrapped every frame).

### Top bar, Network, compass
- titlewin (16,16) 112x30 and helpwind (130,16) stretched to W - 260 with caption sprite 361 at x1 - 0x50. The bar
  shows the focused main-menu item's first string (a group 0xe id: "322" = "Equip weapons and armor."), scrolling
  1 px a tick after 60 ticks when too wide. The items' second strings are not group 0xe ids (20 there is a region
  help line), so titlewin stays empty for now. Both bars are off in events (matches the retail cutscene shot).
- netstat (W - 128, 16): S / R = datagram bytes sent / received per second over 3 s (FUN_100f9ba0); the percentage
  +12 every 3 s while a server datagram came within 10 s, else -6 (FUN_100f9b20); netmeter sprites in order 12,0..11,
  base 0/9/5/1, a frame every (11 - pct / 10) draws, taken at 30 draws a second *(inf)*. "S" / "R" are group 0xc
  0x5c / 0x5d, drawn as literals. Counters: NetStats in client/net.rs (whole UDP payloads, header included).
- Compass DAT_10621980: centre (0x58, region-0 top - 0x2a); region 0's top (UI object +0xa0, refreshed by
  FUN_10159080) is taken as the log's current animated top, so it rides on the log and sits at the bottom with
  the log empty. Disk sprite 0 through a 2x2 matrix (scale 0.32, turned so the camera heading is up, tilted by
  cos 1.0996); letters at radius 140; time "%d:%02d" in fontshp at (cx - 50, cy + 26); day orb framesus table
  0x10379950 at (cx - 68, cy + 26); weather badge usgaiji at (cx - 68, cy - 42) for weather 4..19. Opens over 24
  ticks; hidden in events. No-compass zones: ships and airships list of FUN_100abd90. The disk's texture-north
  alignment and the letter mapping (s4 N red, s1 E, s2 W, s3 S) are *(inf)*. Radar dots and the pointer are not drawn.

### Targeting and lock-on
- Target changes go through FUN_10151280 (client/target.rs set): SE "0009" from none, "0010" when switching; a
  different target ends the lock. The target goes when its entity does (FUN_10151bb0).
- s2c 0x058 {UniqueNo @4, AssistNo @8}: our id and a modelled actor -> set the target; lock when it is a monster
  *(inf: +0x120 bit 13 = the 0x00E monster bit)* and auto lock-on (config 7 = 0) is on. Verified live: engaging a
  Wild Rabbit locks on and shows windowps 212 ("Locked") over the target window.
- Locked camera (FUN_1001e030): look-at = midpoint of player (+ bone 13 y) and target (+ bone 5 y, x0.8 for a
  Tarutaru player), eased 25 % a tick; eye kept within 3 atan(4 / (near + d)) of the target-to-player line and its
  distance eased into [near + d/2, far + d/2], near = (7.2 + 0.125 d) * 0.35, far = (8.6 + 0.125 d) * 0.35; wall ray
  from player y - 1 with 0.2 margin. After unlocking the free camera ignores turns for 20 ticks. CPU bones for the
  two look points are only posed while locked. FOV = 2 atan(192 / 350) (config 0x49 = 350) for the free camera too.
- Locked movement (FUN_100a6210): side keys circle the target at the same radius, forward / back change it down to
  0.8 *(inf: the two radii)*, the player faces the target. /lockon (cmd 0x59) toggles.
- Cursor: anc_l sprite (6 frames, 4 ticks each, tint 0xe0808080) with its tip on the target's attach point.

### Name plate and arrow anchor (FUN_100d23f0 / FUN_1002b5e0)
- Both use skeleton attach point 2 (actor +0x8a6 = +0x8a7 = 2, set by the constructor FUN_100c3d20): the attach
  table follows the 0x29 bones ({i16 bone, f32 rot xyz, f32 trans xyz}, 26 bytes, count u16), parsed now as
  Skeleton::points. Point 2 is special-cased as actor position + point2.translation x model scale: rest data, no
  pose, so the plate no longer bobs with breathing or kneeling. PCs: bone 0, (0, -h, 0), h 2.0 Hume / Mithra, 2.4
  Elvaan, 1.3 Taru, 2.6 Galka (unit test dat::character::plate_points). Lift for bone-0 points: status 5 / 85 -1.3,
  63..83 -0.5 (FUN_1002b7a0; the mount sub-cases and the +0xa4 gate are not decoded).
- About 40 NPC / monster models pick another point in their init scheduler (ops 0x9e arrow / 0x9f plate); those
  follow the animated bone in retail. Not implemented: they use point 2.
- FUN_10084b20: the plate's letters hang down from the anchor (top edge on it), centred, -0.5 px; the arrow's tip
  is on the same point, truncated. Name pulse (FUN_100819d0): the target's alpha x (sin(ticks * 16 deg) * 32 + 96) /
  128.

### Zone name and system messages
- Zone-in prints one log line, "=== Area: <name> ===" (group 10 0x62, name = group 5 file 55465 by zone id),
  FUN_100f8730 from the 0x00A handler FUN_100f7de0 by LoginState (u32 @0x80: 2 / 0 / 6 the area; 1 MYROOM the area
  for maps 0x118 / 0x11d, else group 9 0x126 + floor; 3..5 nothing). No banner, no fade.
- s2c 0x053 {para @4, para2 @8, Number @0xc}: file 7031 (language slot 0x65) through the battle expander; LSB's
  logout countdown is #7 ("Executing logout in N seconds.", mode 0x88) and the shutdown one #35.
- The client's own bracketed debug lines are gone from the log (log::info instead). Retail prints nothing for an
  unknown command, a missing /attack target, zone lines or 0x00B; a failed known command prints ">> line" and group
  9 0x78 "...A command error occurred." (mode 0x9d). Cancelling a logout runs /logout again (0x0E7 Mode 0 toggle;
  Kind 1 logout, 3 shutdown; "on" 1 / 3, "off" 2).

### Remote actor movement (0x00E handler LAB_10099c80, FUN_1008b610, FUN_1008b680, FUN_100c4390)
- A position update is a goal, not a snap: slide straight to it over (counter delta + 8) ticks, counter = u16 @0x18
  & 0x1fff (a smaller value restarts it); more than 12 units away snaps. The drawn position eases 0.125 a tick toward
  the slide. A drawn step under 0.01 a tick counts as standing; facing follows the step (-atan2(dz, dx)), else the
  server heading, turning 0.125 a tick. LSB sends a roaming mob every 400 ms with a 53-tick counter step, so the
  61-tick slide never ends between updates: live trace (FFXI_TRACE_MOVE=1) shows a rabbit at a steady 2.1-2.4 u/s,
  ratio 0.53-0.59, no stops (scratchpad hud/mvstat.py on h4_mv101.log). Rates are applied per 1/60 tick with powf so
  they hold at any frame rate *(inf: retail assumes one tick a frame or more)*.
- Ground: NPC ids without u32 @0x18 bit 15 and kind (@0x30 & 7) not 2 / 3 follow the collision floor every frame
  (FUN_1008dfb0 entity level, FUN_1017a400: a ray from y - 2 down 50, else from y + 2 up 15, nearest hit to the
  start, all faces). Before, only packets snapped, and monsters spawned before the zone loaded kept LSB's navmesh y
  (a few tenths high). The near-camera front-face variant (FUN_100a3560, depth <= 10) is not separate here.
- Not done: retail's per-actor ground shadow (FUN_10032f80, "system kage tex"), root xz rotation by 3pi/2 for bone 0
  translations and scale channels in posing (neither changes these models' heights).

### Frame limiter and present mode (not retail)
- config.json `frame_limit` (30, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 300, 360 or 0 = unlimited; default
  300; FFXI_FRAME_LIMIT overrides for test runs), also on the lobby Config screen. Every screen paces from its frame
  start (the creator's 30 fps hold is gone). Present mode: vsync when the limit is at or under the monitor rate, else
  Mailbox, else Immediate. Game ticks stay 60 Hz and motions 30 frames a second; only drawing runs faster.
- `anisotropy` (default 16) for scenery, model and creator samplers.

### GPU-side rendering
- World actors: bind-pose corners with two weighted influences uploaded once per model (ZoneRenderer::new_skinned);
  vs_skinned skins from a per-actor pose region (model matrix, alpha, (quat, translation) per bone) in one storage
  buffer at dynamic offsets. The motion keys and skeletons go up once per model / motion file (render/anim.rs); a
  compute pass evaluates every actor's motions (rest pose, latest track wins, the 16-tick cross-fade slerp) into
  those regions each frame, at the fractional frame, so motions interpolate between keys at any frame rate. GPU vs
  CPU pose: under 1e-3 (unit test render::anim::gpu_pose_matches_cpu); offscreen select-shot with FFXI_GPU_SKIN=1
  vs the CPU path: 1 pixel over 8 levels apart.
- The creator dancer: shapes, bind tangents and per-vertex influence lists (up to 75 per vertex, so lists in a
  storage buffer) upload once; per frame only the joint matrices (cur x bind_inv) and the pass uniforms. vs_skin does
  the skinning, the D3D8 lighting, ramp, env / phong texgen, aniso and the planar shadow; the passes and states are
  build_draws'. The channels are sampled at the fractional frame every frame. "_sort" triangle orders and the idle
  camera's bounds still come from CPU positions once per 30 Hz frame. With the old texture path the creator shots
  match the CPU version except a few shadow-edge pixels (scratchpad hud/crs, crdiff.py).
- Scenery: one vertex and one index buffer per zone (draw_indexed with base vertex), bind groups only when the
  texture changes, file order kept (zone shots before / after identical).
- UI: one persistent vertex buffer, written only when the batch changed.
- Textures: DXT1 / DXT3 IMG data kept and uploaded as BC1 / BC2 (sRGB) when the device has
  TEXTURE_COMPRESSION_BC; DXT3's PS2 alpha (0x80 opaque) is doubled in the 4-bit alpha in place. Full mip chains:
  level 0 is the file's own blocks, lower levels box-filtered in linear light from the decoded RGBA and re-encoded
  (bounding-box BC1/BC2 encoder); alpha-tested images keep their coverage at the 0.3 cut-off per level (alpha scaled,
  smallest scale that reaches level 0's coverage). Palette images get RGBA mips. Trilinear + 16x anisotropic. East
  Ronfaure: 45 zone textures, 53.5 MB with mips vs 112.5 MB as RGBA without (logged at zone load). The IMGs carry no
  mips of their own. FFXI_TEX_OLD=1 restores the old upload for comparisons (hud/h4_ground_old.png / _new.png).

### Send cadence (FUN_100f7270 / FUN_100f7ae0 / FUN_100fac80)
- One datagram every 400 ms (config +0xd0 = 400; s2c 0x005 PacketCnt would change it, LSB never sends it), queued
  or not; each carries a fresh 0x015 (one rewritten slot, built every frame by FUN_10096c00) and every sub-packet
  the server has not acked yet (sync = the seq of its first send; acked once the server header u16 @2 reaches it).
  Commands go out on the next datagram. Network S standing: ~165 B/s (was ~250 with the old 250 ms flush), in
  retail's 114..180. Not done: the adaptive +50 ms step when the server lags, the 1000 ms per-id refusal.

### CPU (release, 1280x800 window, eGPU RX 6800 XT, cpal thread on alsa:null excluded)
| place | before | after |
|---|---|---|
| East Ronfaure (101), 10 actors | 77 % of a core, 114 fps (vsync), 6.6 ms CPU a frame (actors 6.0) | 19 % at 296 fps (limit 300, Mailbox), 0.61 ms (actors 0.12, encode 0.35) |
| S. San d'Oria (230) auction house, 17 actors | 101 %, 74 fps, 13.4 ms (actors 11.8) | 33 % at 296 fps, 1.1 ms (actors 0.27, encode 0.64) |
| Creator, race step | 12.9 % at 30 fps (creator agent) | 23.5 % at 300 fps (~0.8 ms a frame) |
- Motion name lookups are cached per actor (actmotion::FindCache) until its model or file list changes.
- The cost was CPU skinning plus per-corner vertex rebuilds and uploads for every actor every frame (~0.7 ms an
  actor), then the per-draw buffer binds of the scenery. F12 shows fps, the limit and present mode, CPU ms a frame by
  span, entity / actor / posed / drawn counts, scenery draws and UI vertices; FFXI_PERF=1 logs the same every 2 s.

## Cutscene fixes after the 0x1f7 validation, lobby fades, HUD party / target, no egui fallback (2026-10-21)

From the validation report (scratchpad cutscene_validation.md) and the decompile (all.c / all.asm). Inferred items
are marked *(inf)*. Live checks on lnxtest01: the new character Lnxcsfix (Hume M WAR, San d'Oria) played 0x1f7 once
(scratchpad csfix/run2/*.png); NPC talks on Lnxtest (csfix/talk3_*.png); lobby shots csfix/lobby1, lobby2.

### Event actors (FUN_100ad000 / FUN_100ad460 / FUN_100ad230 / FUN_100ad280, the 0x00E handler)
- c2s 0x016 CHARREQ: body u16 ActIndex, u16 0 (FUN_100ad230). LSB answers with a full 0x00E (0x0F, 0x57 for equipped
  looks), and for our own index a full 0x00D (mask 0x1F) plus 0x037.
- The event waits before its VMs exist: in block order, the first bound actor (event id or 0xfffe; FUN_100ad740: 0x7ffffff0
  / 0x7fffffc0 = us, party slots not modelled, else index = id & 0x3ff) that has no actor, a despawned one (+0x124 bit 12)
  or no full update (+0x141) is asked for once (bitmap DAT_104755d8) and the walk stops there; then it waits for the
  model (+0x120 bit 9). FUN_100ad280 clears the bitmap every 120 ticks (real ticks; at 300 fps a per-frame clamp to 1
  would re-ask five times too often). No give-up. Requests therefore go one at a time: live 0x1f7 asked for 29 actors
  and started after 14.8 s. Ours, not retail: every missing bound actor is asked for at once (LSB answers only the
  client's 400 ms datagrams, so one at a time froze the player ~12 s before the San d'Oria opening 0x1f4); ready
  after 3.6 s live (2026-10-22).
- Event bodies' walk / run ratio (sync) counts VM ticks: it updates only on frames where the VM ticked, with that
  many ticks. Per frame at 300 fps it flipped between 0 (no VM step) and a capped run (a whole tick's step over a
  1/300 s frame), and the legs blurred. FUN_1008cce0 sets the actor's speed / speed base to body +0x16c / +0x168 x
  0.8 (_DAT_1031e8a4); the reference unit (units/s vs tenths) is not confirmed *(inf: 4.0 units/s)*. A model that fails to load, or an entity with no look, counts as loaded *(inf)*. The
  Mode & 0x100 checks (state 7, busy model) are not modelled.
- +0x141: a first 0x00E counts only with mask & 7 == 7 (& 0x17 for kinds 1 / 7, 0x1009a097); 0x00D needs 0x1F
  (0x100982f6). A partial first update is dropped and the actor asked for again (+0x12c bit 15, FUN_1008d280).
- Status u8 @0x20 bits 0 / 1 / 2 -> actor +0x120 bits 13 / 14 / 18 (0x1009a397, also 0x1009a45a / 0x1009a4f9: updated
  with mask bits 1 / 2 / 4). LSB cutscene_only = 6 sets 14 and 18.
- Drawing (FUN_1008cdc0): a bound actor while the event runs is hidden only by +0x120 bit 0x20000 (ops 0x22 / 0x4e);
  otherwise by bits 14 / 15, and, unless it takes part (+0x120 bit 8, set during the wait and kept while bound),
  by (DAT_1046df04 | Mode) & 0x02 for player characters and & 0x10 for NPCs and monsters (bit 0x800000). Bits 14 or
  18 make it untargetable (targeting, Tab, talk).
- Bind (FUN_100bb300): +0x120 bit 0x20000 = bit 14 | bit 15 (so cutscene-only NPCs start hidden), bits 0x200000 /
  0x80000 cleared, body +0x168 = +0x16c = 5.0; the model colour goes back to 0x80808080 unless Mode & 0x200
  (FUN_10095c00).
- Despawn of a participant (+0x120 bit 8) waits for the end (0x1009a04d); the event's end removes it.
- End (FUN_100ac9c0, FUN_1008d710): bound actors are drawn from their own +4 again (nothing in the event writes it;
  ours: the VM bodies are a draw override, the server position is never touched); with (DAT_1046df04 | Mode) & 1 the
  player is put back at +0x24 (its position at bind) and the camera behind it (FUN_1001d950); & 0x8000 snaps actors to
  the ground (not used here). Bound player characters get +0x1f8 = 300 / +0x1fa = 1: 300 ticks later FUN_1008d280 sends
  0x016 for them, then every 600, until a full 0x00D clears it. The reading that FUN_10094ea0 then holds the player's
  movement (FUN_100954f0) is WRONG: retail players are never frozen after an NPC talk (confirmed by the user).
  The client keeps the request but does not hold movement; what +0x1fa really gates is unresolved.
- Event time and volume: op 0x77 sets the weather even with hour 0xff (the clock then runs); op 0x78 drops both. The
  end fades masks 1 and 4 back to 1.0 over 30 ticks (taken with DAT_1046dacb == 0).

### The VM runs at 60 Hz
- Retail runs the VMs once a frame with dt = frame ticks clamped >= 1 (FUN_10014000), i.e. one tick a frame at 60 fps.
  Ours called it every frame with dt.max(1), so at the 300 fps default limit scripts ran five times too fast while
  scheduler cameras and message timers ran in real time. In the live 0x1f7 the gate knights walked their 50 units in
  under 8 s and had gone past the s006 camera (and hidden themselves) before the shots. Now the client steps the VM in
  whole 60 Hz ticks (up to 4 a frame). Every live check before this fix had the same speed-up.

### Event bodies as drawn (FUN_1008cce0, FUN_100c4390, FUN_1008b680, FUN_10089850)
- Bound actors draw from the VM body position; the drawn rotation eases 12.5% a tick toward the body direction (toward
  the travel direction while moving); 0x36 / 0x37 snap it (FUN_100917e0). +0x12c bit 1 (0x70 / 0x76 wait on it) while
  the gap is over 16/4096 of a turn.
- Motion ratio: the model's speed reference is 0.8 (_DAT_1031e8a4) x body +0x16c (5.0 at bind; op 0x91 sets it =
  val x 10), the body speed +0x168 x 0.8 goes to the other model speed. A set_speed 13 walk settles at 0.325: the walk
  motion at 0.975 (motion::move_ratio_ref). Bound actors use status 0 for their motion *(inf: +0x170)*.
- A new scripted direction more than 1/8 turn from the last (+0xe4), not while walking, plays the step scheduler
  '@tl0' (old - new in (0x200, 0x800) of 4096) or '@tr0' (FUN_10089850, from FUN_1008b680); FUN_10089810 stops both when
  0x70 / 0x76 finish. Played only where the model has them.
- Our player draws from its body too (facing, walk); the 0x015 position stays the server's.
- 0x1e / 0x4a on an unresolved target do nothing; on an unbound actor 0x4a / 0x4b turn the actor itself (+0x18).
- 0x33 (+0x120 bit 0x200000) gates the ground snap of 1f walks and 0x36 / 0x37.

### Messages
- 0x2b / 0x49 / 0xb0 show nothing when the speaker does not resolve (FUN_100ae0c0); 0x48 clears DAT_10470548 and
  DAT_10472bb2 (FUN_100b0570; the earlier note "0x48 keeps the previous one" was wrong).
- The event message window (FUN_10131e30 / FUN_10131d20): only the open message, wrapped, at most 4 lines a page (a 5th
  makes a keyed page); framed at the log's place with height lines x 16 + 4, or, while FUN_10132030's +0x48 is on, at
  (x, y) without the frame. 0x67 = (1, val@1, val@3), 0x68 = (0, 0, 0); text codes `02 lo hi` / `03 lo hi` set
  DAT_10476f98 / 9a and place it (FUN_100bf790). Negative x / y are -176 - W * v / 100 and -32 - H * v / 100. The place
  goes through FUN_10159920 mode 0, taken as plain pixels (as the creator's origin) *(inf)*: the narration sits at
  (80, 340).
- Correction (2026-10-22, traced by a sub-agent): event text is written to logwindo and the log is drawn during events.
  The message object (FUN_10122b50 / FUN_10122fc0, rem4line at +0x83c) cuts it into 4-line chunks (FUN_10123060);
  FUN_10131e30 wraps each at the log width (DAT_105dc3e4 +0x32), counts its lines (rem4line +0x4e, list +0x14, count
  +0x24) and writes it to the log ring (FUN_10124050) with entry +0x12 = 1 ("the open message's" *(inf)*), first serial at
  +0x50. rem4line ("menu    rem4line", DAT_105657b8) is only an overflow window: its show path FUN_10131d20 is called
  only from the log's add-line FUN_1015b8e0, when the log is at its max lines and its oldest visible row is the
  message's; it then sits at the log's top-left (height +0x4e x 16 + 4, asm 10131dc2..10131deb) and the log's text rect
  loses min(lines, 4) rows (FUN_1015ba60, asm 1015be84..1015bed3). FUN_1015ba60 keeps the log open while a window with
  flag 0x1000 is up, while rem4line is open (+0x69), or while the oldest visible row is the message's and the message
  rows showing are <= its line count; otherwise it decays to the min as usual. Ours: pages go to the log as they open,
  the log holds while a message waits with no more rows than its page; placed narration keeps its own frameless
  window; the overflow case is not modelled.
- Mouth: a spoken line (0x2b / 0x1d / 0xb0) starts the speaker's face motion ('mou4', else 'mou6'; FUN_10086710 /
  FUN_100867a0) at once and every 142 ticks, (chars + 20) / 20 times, over the body motion; it stops when the window
  closes (0x23, FUN_100b1140; the 8-tick blend out is not done). Curilla (model 69) and Rahal both have mou4.

### Head look (FUN_100b6980, FUN_100b6a50, FUN_100b6b70, FUN_10086820, FUN_100d38c0, FUN_10029e40)
- 0x1e / 0x4a (and 0x79 sub 0) put the actor in look mode 1 at the target's attach point 6, 0x79 sub 1 at point val@10,
  sub 2 mode 2 along its facing (pitch val@6, yaw val@8 in 4096ths); 0x7b stops (+0x12c bits 0x300, +0x144 = 0xffff).
  event_ops renamed: 0x79 actor_look, 0x7b actor_stop_look (was "actor_clear_move").
- The skeleton keeps the look point in actor space (+0xb0, eased 1/32 a tick) and a weight (+0xbc) ramped 0.04 a tick;
  it ramps down when the point is behind (x < -0.5), off to the side (cos < -0.1) or nearer than 0.3 (constants
  0x1031f240, 0x103261d8, 0x1031ffcc), then the point resets to (20, 0, 0). FUN_10029e40 turns the bones of attach
  points 3 and 7 (one bone for some statuses). Ours: in the GPU pose pass, each of the two bones turns its share (half)
  of the arc from the body's forward to the point *(inf: retail's per-bone aim is not decoded)*. The target point is
  the rest-pose attach point *(inf: retail reads the animated one)*.

### Emotes (op 0x6e / 0x99)
- 0x6e (FUN_100b5db0) waits while actor +0x1b8 is busy, then FUN_1009f200: emote -> motion index by table 0x103504a8
  (13 -> 15; codes 0x7a.. are computed forms, not handled); state 0xc loads group index >> 3 (FUN_100cfbc0), PC looks
  file 0x103523dc[race] + group and skirt 0x103523ee[race x 2 + skirt] + group (FUN_100cfc10); state 0xd plays
  "em0" + (index & 7) (FUN_100d0ea0); state 0xb waits for it, then FUN_100d2340(3). 0x99 waits while it plays. Rahal:
  files 16409 / 16537, em07 (live log). The standard-model table (0x10352414 by FUN_10082d70) is not traced.

### Cameras (FUN_1003be40, FUN_100547d0, FUN_1003c3e0, FUN_10020450)
- Path flag bit 19 puts the current camera (the one drawn last frame) first; bit 20 appends the player's follow camera
  (eye = attach point 2 x (-3, 0, 0), target = position lifted by 0.6 x its height or 1.2, focal 350) *(inf: inputs)*.
  Then lerp for 2 points, the chord-length splines from 3. Bit 20 is not "player-relative" (the earlier MODLOG note
  was wrong). Roll is still not applied (every roll in 0x1f7 is 0).
- 0x46 sub 0 also kills the camera task and puts the camera behind the player.

### Screen fades (0x10057ee6 / FUN_1005f630, 0x10057d12 / FUN_1005f020; render/screenfx.rs)
- Scheduler op 0x0f is a colour multiply of the scene, 2 x RGB / 255 (MODULATE2X; A unused), over dur x speed - 1
  ticks. GPU pass: a full-screen triangle with blend src = DST, dst = SRC and colour m / 2 (unit test multiply_pass:
  the fdo1 / fdi1 midpoint halves the scene). Op 0x10 captures the frame before the cut and fades it out over 60 ticks
  (SRCALPHA); ours renders that frame once offscreen from the last camera with last frame's poses, in its own submit
  *(inf: the capture source)*.
- Schedulers, camera shots and fades live in World (event::Screen), so the fdi2 started with 0x21 finishes after the
  event (live run2 q0..q2). The zoning fade is the same multiply now (and the 2D layer's level), on the GPU.

### Lobby fades (FUN_10233d50 / FUN_10233cf0 / FUN_10233db0, file 33193; FUN_10245fb0)
- 33193 holds 'sfot' / 'sfin' (op 0x51, the 2D layer to 0 / 0x80808080 over 60 ticks) and 'ffot' / 'ffin' (op 0x0f,
  the scene). A screen change fades both out, waits 8 frames (FUN_10233d90), switches and fades both in. The creator
  fades in only its 2D layer at its start (FUN_10233db0); its scene comes in once the dancer shows (FUN_10245fb0 'ffin').
  The select screen fades in once its scene has loaded (FUN_10230a10 loads first). Leaving for the world fades out
  first. UI fade: the UI shader multiplies by the level (colour and alpha) *(inf: op 0x51 as a modulate)*.
- The window is created hidden (no frame presented, so unmapped on Wayland) until the title can draw (UI data and
  flyover, or a failure).

### No egui fallback
- The egui login, title, select / delete / confirm, creator and Config windows, the egui world HUD (target, vitals, KO,
  chat) and name plates, and the character preview model are gone. Without game data one plain error shows. egui stays
  for input events and the F12 overlay. Config has no game screen yet: it buzzes.

### HUD: party row and target window (FUN_101f9560, FUN_10159080 -> FUN_10158e10, registry 0x10366460)
- Row: HP gauge (+0x1a, +7), MP gauge (+0x31, +0x11), name, HP number (+0x58, +1) coloured by HP % (FUN_101f93e0), MP
  number (+0x62, +10, 0x80808080). The MP gauge and number always show (a WAR shows an empty gauge and "0"). TP
  (FUN_101f9940 at +0x30, +10) only with config 0xae bit 0 (FUN_1018e330(0xae, 0, 4, 0, 0, 1, 0, 0): default 0), so the
  retail default has no TP; we drew it. Alliance rows (FUN_101f9750) have no MP gauge, MP number at (+0x68, +8).
- Region stacking: windows whose registry flags have 0x2000 (targetwi: 0x02002401) sit on region 1's window, the party
  window: frame top = party frame top - frame height - 2 (at least 16), so the target window rises as the party grows
  (scratchpad csfix/hud_solo_x3.png, hud_p3_x3.png vs images/26.png). Region masks 0x1036a240: 0x1000, 0x2000,
  0x4000, 0x10000000.

### Corrections to earlier notes
- "The client plays the event once the zone is up, after 1.5 s" (Live NPC talk entry): replaced by the actor wait.
- "Fade schedulers ... drawn as an overlay colour with alpha a / 128": a multiply (above).
- "camera paths with flag bit 20 (player-relative)": bit 20 is the follow-camera end point.

### Open
- Name plates in cutscenes: ours show over visible NPCs; whether retail hides them during events is not traced
  (FUN_100ce4d0's vtable checks).
- The map window (op 0xc8 opens menu_mapframe through FUN_101ee9c0, 0x8b sets marker slot 0 "Ailevia", 0x8a closes):
  only the state and markers are kept; there is no map window yet.
- Not done: camera roll, the mouth's 8-tick blend out, the 0x5c volume subs, 0x5b / 0x66 finishing in one call when
  cached, motion-file model kinds, the alliance windows, the HUD window FUN_10217e90 of 0x46 sub 1.
- After the VM-speed fix 0x1f7 has not been played live again (the one test character has seen it); its timing is
  the headless VM's (unit tests opening_actors, sandoria_opening).

## Top bar fade, menu titles, target gauge, map window, log focus, lock-on release, loading indicator (2026-10-21)

Decompile: all.c / all.asm (FFXiMain). Research notes: scratchpad topbar_rules.md plus sub-agent reports summarised
here. Inferred items are marked *(inf)*. Offscreen: `topbar-shot` (new; bars at a fade state, main menu, party row,
target window, `--map ZONE,X,Z[,owned]`, `--downloading N`). Shots in scratchpad topbar/ (t_*.png offscreen,
live1_* / live2_* on lnxtest01 / Lnxtest in zone 230).

### Top-bar fade (FUN_10159080, asm 0x1015951e..0x10159654; window part fade FUN_10116230, asm 0x101163e7)
- Windows with registry bit 0x8000 (helpwind, titlewin, titlehan, netstat) fade their frame part. Active = a
  titlewin or helpwind text slot not empty (FUN_102179a0). The titlewin HN mode (+0x448), titlehan and the +0x64
  busy icon are not modelled.
- Not active: the manager timer (+0x78) counts down by ticks while > 0; at <= 0 every 0x8000 window gets state 2
  (fade out), count 20, and the timer is 0. Active: only when the timer is <= 0, state 3 (fade in), count 20, timer
  180. So the 180 is re-armed only after the bars went down (unit test hudtop::timer_not_rearmed_while_up).
- Part update: count -= ticks; at 0 state 2 -> 1 (hidden), 3 -> 0 (shown). Alpha 1 / 0 / count * 0.05 /
  1 - count * 0.05. Frame and window sprite drawn with ftol(alpha * 128) << 24 | 0x808080, the frame alpha blended
  (0x44) while alpha < 1, skipped when hidden (FUN_101144b0 at 0x101147a8). The "Help" sprite 361 only while
  shown (FUN_10117b60). Window content (netstat icon, %, S / R; the bar texts) always.
- Manager init sets the timer to 30 and parts start shown: HudTop::enter() on every zone-in *(inf: FUN_10010080
  is the in-game start)*. The rule runs only in game state 0x60; zoning and the zone load are taken as 0x70 *(inf)*.
  The bars' in-event hide is gone (the rule hides them); the compass keeps it. 60 Hz whole ticks.
- Live (zone 230): 1.5 s after closing the menu the bars are up without text (live1_4), at 4 s gone, only the
  netstat content left (live1_5, as images/31.png). At the script's "world" phase the bars were already down:
  that phase starts after the zone fade-in, which outlasts the 30 ticks.

### Text slots and menu titles (FUN_10217230 class, FUN_102178e0; FUN_10115380 / FUN_101152c0 / FUN_10115320)
- Group 0xd (file 55652) loaded beside group 0xe. Item strings: first = group 0xe id (help), second = group 0xd id
  (title), "-1" none; an empty item string falls back to the window header's (element 0).
- Shown slot: 3, else the highest non-empty of 2 > 1 > 0, separately for helpwind and titlewin.
- Slot 1: the focused window's cursor child: the main menu, the Items / Equipment windows (inventor header
  "3" / "2" = "Select an item." / "Items"; equip header "-1" / "20" "Equipment", items 243.. help), map0.
  Per-item overrides (+0x4c / +0x50) are not traced.
- Slot 0: the action menu (FUN_10127790): help = group 0xe by the command table (u16 at 0x103634fc + 12 * cmd);
  title = group 0xd 7 "Battle Mode" (engaged) / 6 "Ready Mode" (a FUN_101c26a0 target: conflict allegiances, not
  modelled) / 5 "Commands". Help 0x1b -> 0x1c and 0x1e -> 0x327 special cases not modelled.
- The main menu's "Commands" caption (menuwind child 15, comwin 16) is right already: type-1 parts are drawn at
  window + raw (x, y) = (0, 0) (FUN_101167b0), and sprite 16's own cell bounds start at (19, -3).
- Live: "Map" / "View map of current area." on the menu's Map item (live1_3, as images/27.png); "Commands" and
  the chat command's help with the action menu (live1_6).

### Target window gauge, HP colour, name plates (FUN_10149900 asm 0x10149976; 0x1009adbb; FUN_10082ec0)
- No entity-kind check: the gauge is hidden by actor +0x128 bit 3 or +0x130 bit 20. 0x00E with mask 0x04:
  u8 @0x22 bit 0 sets bit 20 and clears bit 3; else bit 3 = u8 @0x2B bit 0x20 (u32 @0x28 bit 29). 0x00D clears
  bit 3, so players always have a gauge. Bit 3 also hides the name plate (FUN_100ce4d0 gate 3).
- LSB sends m_flags at u32 @0x21 (u8 @0x22 bit 0 = flags 0x100) and namevis at @0x2B; its NPC data defaults
  (entity_flags 3, name_vis 0, e.g. Home Point #2, Cheval River) set neither, so on LSB NPCs show a gauge (live3c_1).
  Retail's server presumably sets one of them for ordinary NPCs *(inf)*; the client follows the decoded rule.
- HP colour (FUN_101f93e0): hpcol sprite 0 at >= 75 %, 1 >= 50, 2 >= 25, else 3 (strict <, 0-based), vertex 0 of
  cell 0. hpcol 0 is (127, 127, 127) = white, 1 (127, 127, 64) yellow. images/26.png's gauge is about 60 % full, so
  its yellow "658" is band 1, not full HP. The cell loader (FUN_1011adf0, asm 0x1011ae93) adds 1 to RGB below 0xff
  (now in screens::sprite_tint) and maps alpha 1..0xfe to min((a + 1) * 1.5, 255) (not applied anywhere yet).
- Name plates in events: nothing on the plate path (FUN_10080100 -> vtable +0x284 FUN_100ce4d0 -> FUN_100ce480 /
  FUN_100819d0 -> FUN_10084b20) reads DAT_1046df04, the event mode or the event camera; the task runner FUN_10074050
  has no gate either. In an event a plate goes exactly when the model is hidden (FUN_1008cdc0: (df04 | Mode) & 2
  for PCs, & 0x10 for others, unless taking part; then FUN_10091150 deletes the drawobj), which event::hidden already
  does. Participants (the player in most cutscenes) keep their plates. Op 0x38's 0x2000 is not read there. Other
  gates: first person, the +0x8ac invisible bit, +0x128 bit 3 (now done), drawobj state codes 1..4 / 0x54, model ids
  0xb22..0xb31, the /names-off command (DAT_1046ad64), +0xb2 hide bits. Not done: the state codes and model range.

### Map window (main menu item 12: FUN_1013b3f0 case 0xc -> FUN_101ee8f0 -> FUN_101f0af0 mode 0)
- Region table 0x10376010, 14-byte records until scale (+5) is 0: zone s16, sub-map key s8 (FUN_1017a2d0: the
  collision map id under the player, not decoded; key 0 used), map number, flags (low nibble file base, high nibble
  key-item base), scale, key item, file index s16, origin ox / oy s16. File (FUN_101ef690): idx + 0x14c0 / 0xd02f /
  0xd417 by the low nibble, else 0x1592 (m_000_00, "NO MAP"). Key item (FUN_101ee560): < 0 always, 0 none needed,
  else 0x180 / 0x73f / 0x8fd + ki, owned = bit (id & 31) of word id >> 5 (s2c 0x055 GetItemFlag[16] @4, table @0x84).
  Jugner (104): file 5317 (ROM/17/28), origin (-264, -256), key item 0x181 (unit test map::jugner_region).
- The map DAT is a menu DAT: one 2048 x 2048 DXT3 image and a sprite whose cell is the 512 x 512 map-pixel quad at
  the origin. Drawn at (origin - view) * zoom + 0.5, colour 0xa0404040 (ours 0.5 grey, alpha 1), alpha blended
  (FUN_101e6ca0). Not owned: the NO MAP image at (0, 0) and markers shifted by the origin (+0x6c).
- Zoom W / 512 x config 0x8a / 1000 (default 2000); W / 512 when no map is needed (then no marker and no map0, as
  decompiled). View: the player under (W * 200 / 512, H / 2) clamped to [base, base + max(0, 512 - W / zoom)]
  (FUN_101e7a60 / FUN_101e6110); retail animates that scroll, ours starts there *(inf)*.
- World to map (FUN_101ee650): px = ftol(x * |scale| / 5 + 0.5), py = -ftol(z * |scale| / 5 + 0.5).
- Player marker: keytops3 159 (table 0x10375f6c[2]), flags 0x9f (fade in, blink 6 alpha a tick), turned by the
  player's facing (the sprite points east; ours from our own facing, retail's heading sign not decoded).
- "(%c-%d)" = ('A' + (px - ox - 16) / 32, 1 + (py - oy - 16) / 32) in fontshp at (titlewin x0, y1 + 3) = (16, 49)
  (FUN_101f1a60). helpwind slot 2 = the zone name (FUN_101c2090(zone, 1), group 0x14 file 55465); titlewin "Map"
  from map0's header "8" (slot 1).
- map0 (384, 48) right-anchored: Markers / Wide Scan / Change Map (keytops3 2 / 3 / 494), caption child 4 comwin 28
  "Map"; Up / Down along the links, Escape closes both. Offscreen t_map_owned.png / t_map_none.png vs images/32.png.
- Not done: the open / close fades, the recentring scroll, pan / zoom keys (input context 0x26, not traced), the
  sub-map key, Markers (mapv3) / Wide Scan / Change Map, other markers (party, POIs, .MRK files), the Mog House
  branch (myroom), the group 7 messages 97 / 98, the mouse.

### Numpad + and the chat log (FUN_10151550, FUN_1015ba60, FUN_1015ae20; fulllog FUN_10132e60 / FUN_10133890)
- Numpad + (DIK 0x4E, flag 0x80) and Ctrl / Alt + F are key functions 0x11 and 0x78 together; "=" / main "+"
  (DIK 0x0D) is not bound. 0x78 cycles logwindo -> buff -> none with SE "0013"; with a state set and nothing
  focused it only resets to 0 (so after Escape the next press does nothing). Not while typing, in events or with
  the full log open. 0x11 sends message 8 to the active menu (the full log's next tab).
- Focused, logwindo is held at config 0x0f lines (8, frame 134) every frame; Enter (message 5) opens the full
  log, Escape (6) unfocuses. The buff window (state 2) is not modelled.
- Full log: frame (16, 48, text width + 14, H - 64), 23 + (H - 448) / 16 rows (unit test logwin::full_log_keys),
  lines 16 px apart up from y1 - 4 (16 more while typing), the small log hidden meanwhile. Up / Down move the row,
  scrolling at its ends (Ctrl / Alt: scroll only), Left / Right a page older / newer, SE "0001"; Escape closes it
  and refocuses logwindo; an event closes it. Tabs (table 0x1032867c): fep sprites from (x0, y0 - 9), slot s
  unselected A[2s] / selected A[2s + 1], 30 px after an unselected one; the filter-8 tab only with config 0xb4
  (default 0 *(inf)*); filter 0 selected. The hand at (x0 + 3, row bottom - 15) *(inf)*.
- Not done: filters 1..8 (FUN_10125940 not decoded, so numpad + in the full log does nothing), charlnk on Enter
  (SE "0004" instead), the mouse wheel and scrollbar, ~100-line history (ours keeps 512 wrapped lines).
- Live (zone 230): numpad + holds 8 lines (live2_2), Enter opens the full log with the tabs (live2_4), Escape back
  to the focused log (live2_5), Escape, shrinking (live2_6), two presses: reset then focus (live2_7).

### Lock-on and target release (FUN_10151100, FUN_10151bb0, FUN_10152c70, FUN_10089d50 case 3, FUN_1001e030)
- Per frame, in events too: own death unlocks (FUN_10150e30). Our status leaving 1 for anything but 3 unlocks and
  keeps the target. The target goes at once, with the lock (no delay), when: its entity is gone; it is hidden
  (+0xb2 bit 1: server bits 14 / 15 or the event hide); it is attackable (+0x120 bit 13 = 0x00E @0x20 bit 0) and
  dead / despawning (status 3 / 2) or flagged off (u8 @0x2B bit 0x08, +0x130 bit 7, also sends "/attackoff"); or it
  is more than 50 away (FUN_10152ff0(50.0)). Engaged with no target, the lowest-index attackable, living actor we
  claimed (0x00E @0x2C) is targeted without a lock (FUN_101514a0). 0x058 with 0 does nothing.
- Unlock keeps the camera's eye and look: for 20 ticks the eye holds and the look eases 25 % a tick to the player's
  look point; then (look - eye) (d - 6) 0.012 a tick pulls it in, pushed out below 3. Ours: rest = the free camera
  distance (6 by default), handed back to the yaw / pitch orbit once settled or on manual input; the height hold and
  the wall-ray recovery are simplified to a ray from the look point *(inf)* (unit tests target::target_drop_rules,
  target::release_spring). Not checked live (the drives targeted an NPC and then the account was busy).
- Before: the target only went with the entity, and the lock never ended on a kill.

### Party TP (FUN_101f9560 / FUN_101f9750 -> FUN_101f9940)
- Config `party_tp` (default true, NOT retail: retail's config 0xae bit 0 defaults to 0) drives retail's path: TP
  right-aligned at (+0x30, +10), 0x80606060, 0x802060d0 from 1000, for party and alliance rows except trusts
  (FUN_101f8ee0: +0x7a and index >= 0x700). Nothing else in the row moves with the bit (HP at (+0x58, +1), MP at
  (+0x62, +10) party / (+0x68, +8) alliance). Offscreen t_tp.png. Only our own row exists yet.

### "Downloading data ..." (menu chdummy mode 2, FUN_1022eca0 asm 0x1022eda8..0x1022eec3)
- One sprite with the text and beads baked in: keytops3 165 + (n >> 2) % 5 (the dimmed bead pair steps outside in,
  then none), n = draws since the open (FUN_1022e9d0 zeroes it), at (W - 234, H - 48 + 8 - min(2n, 8)) with y scale
  min(n / 4, 1), 0x80808080, no fade. +0x84 / +0x88 screen fractions are 0 *(inf: no other writer found)*.
- Opened in phases 6 (lobby to world), 8 and 7 (zone change, after the server's zone-change message; not on touching
  the zone line), closed in phase 10 step 3 after the zone load (FUN_100135c0). Ours: from world entry and
  NetEvent::Zoning to the end of the zone load; draws are taken at 30 a second *(inf)*; while it shows, only it
  is drawn and at full level (the HUD is not drawn under the black screen; a UI uniform cannot differ per pass in
  one submit). Offscreen t_dl10.png vs images/34.png (retail's capture is ~0.95 scale and a little greyer).
- Correction: the HUD details note "no in-game code shows a loading message ... zoning is a plain fade to black"
  is wrong.

### Open
- The 0x00E gauge / plate bits on LSB (above); the buff window; full-log filters; the map items and keys.
- A live check of the lock-on release after a kill, and of the indicator during a real zone change.

## Player movement collision (2026-10-21)

Correction: FUN_100954f0 is not player movement (it is a small state machine around FUN_1012a090 /
FUN_101c27d0). The player moves in FUN_100a48d0 (input to a move vector) and CXiCollisionActor's update
FUN_100a3560 (vtable 0x10322970 slot 0x20). Other actors (vt+0x304 false) only take the floor ray below.

### MZB collision data (loader FUN_10161130, cell links FUN_10160dc0)
- Table at u32 @8: [0] geometry count, [1] geometry offset, [2] cell-list count, [3] cell lists, [4] grid,
  [5] object records, [6] object count. Object records are 0xc0 bytes (0x80 when the version byte < 0x1a; all
  zones checked are 0x1b): world matrix @0, inverse @0x40, normal matrix 3x3 @0x80, flags @0xa4 (bit 0 mirrored:
  triangles run v0 v2 v1; bit 1 resolved after the others), @0xa8 offset, 4 bytes @0xac, float @0xb0, y range
  @0xb4 / @0xb8, id @0xbc.
- Cell list head: count & 0x7ff, 0x2000 = list switched off (skipped), x @14..22, row @23..31. The grid is 4-unit
  cells centred on the origin: cell = trunc((x + 2w) / 4), row = trunc((z + 2h) / 4), w = byte @0xc x 10
  (checked on zone 101: 94.5 % of entries meet their cell, 0.5 % with z flipped; the rest sit in a neighbour).
- Triangle = v0 & 0x7fff, v1 & 0x3fff, v2 & 0x3fff, normal & 0x7fff. Bit 15 of the four words = a 4-bit surface
  type (FUN_101621a0; zone 101 has 0, 1, 2, 5, 7, 8, 9; 230 adds 6). Type 9 is tested in FUN_1017a6d0 and types
  8 / 9 with status 33 (resting), so they look like water *(inf)*; DAT_1036b2c8 flags types 2, 3, 4, 8 and 9.
  No type changes movement: the sweep never reads it. Bit 14 of v1 is set by the loader for triangles with a
  zero-length edge, and the sweep skips those.
- Stored normals are unit (normal matrix applied), y < 0 faces up, and every triangle in 100 / 101 / 230 has
  cross(v1 - v0, v2 - v0) . n < 0 (after the mirror swap). The parser now keeps normals, types, objects,
  entries and per-cell lists (dat/collision.rs). The previous parser dropped all of these and used & 0x3fff for v0.

### The sweep (FUN_1017ab10 -> FUN_101679c0 -> FUN_10167b10 -> FUN_101671b0 / FUN_101682c0 / FUN_10168500)
- A sphere, radius 0.5 (0.25 = r^2 at +0x3412c), centred 0.5 above the feet. The move from S to E (both lifted
  0.5; E.y += 0.001 when level) gets a frame: z along the move, x = up x dir (up = -Y, or +Z for a vertical move).
- Candidates (FUN_101671b0): faces of the ±1 cells around S, row by row, each list's flag-2-clear entries first.
  An entry's y range must meet [min y - 0.5, max y + 0.5], and the face's bounds in the move frame must meet the
  box |x| < r, |y| < r, -r < z < len + r. At most 0x3fc; a face listed in several cells comes up again.
- March (FUN_101682c0): t = 0, then +0.15 (r x 0.3) up to len. At each point P on the straight line, every
  candidate goes through FUN_10168500; the march stops at the first wall. On the first pass faces whose normal
  points along the move are skipped. Then up to four rounds with all faces at the result; if it still moves on
  the fourth, the move is refused (output = start). Moves of 4 units or more are cut into 1 + floor(len / 4)
  parts, and the first part that touches a floor ends there (FUN_101679c0, _DAT_1032af88 = -0.25).
- FUN_10168500, per face: foot F = P projected on the plane; it counts when F is inside (FUN_101688a0, per axis
  against n, 1e-4) or the sphere reaches an edge (FUN_10168af0), and |F - P|^2 <= 0.249.
  - |n.y| > 0.708 (_DAT_1032af84, about 45 degrees) is floor or ceiling: if it is the nearest so far and some
    vertex is below P.y - 0.3 (not wholly above head height), a face with n.y < 0 sets the result's y to F.y - 0.5
    and marks floor contact. This is the step-up and slope following: the centre rises to 0.5 above the nearest
    point of the plane, so on slopes the feet ride a little above the surface (0.27 at 40 degrees).
  - Otherwise a wall, if it is the nearest so far and reaches above the centre (some vertex above P.y - 0.002):
    when P is on the normal's side, the result = P + d sqrt(0.25 - |F - P|^2) along d = (P - F) / |P - F|, with
    the y part x 0.2 (_DAT_1031f20c). Walls whose top is below the centre are ignored: that is the step limit,
    about 0.5. A riser of 0.5 .. 1.0 can still be climbed when its top face comes later in the same pass than
    the riser (the floor fix then sets y after the wall push): it depends on face order.
- No slide along a wall beyond that push: the march stops where the wall is touched.

### A frame of FUN_100a3560 (player) and FUN_100a48d0
- Input: move = direction x speed / 60 x frame ticks. On the ground (+0x5e0) it runs along the ground normal's plane at
  the same length, n x ((v x n)), and loses any downward part (asm 0x100a53f9..0x100a54b6): uphill it climbs at full
  length, i.e. horizontal speed x cos(slope); downhill horizontal speed x cos(slope) and gravity does the rest.
- Grounded at the start of the frame: fall speed (+0xf4) = 0. Grounded and not moving: nothing happens.
- Sweep from the old position to old + move (grounded = its floor contact), then gravity (asm 0x100a3803):
  g = 0.0408333 (_DAT_10322d74) x the frame's ticks (FUN_10014000, at least 1.0), lowered to the frame's horizontal
  travel when that is between 0.01 (_DAT_1031e888) and g; speed += g, at most 1.0; the first frame of the actor caps it
  at 0.4 (+0x5f4). Sweep straight down by the whole speed (not scaled again); grounded = its floor contact. At retail's
  30 fps (2 ticks a frame) a grounded player probes 0.0817 down each frame, enough to reach a slope after a floor fix.
  Correction: an earlier note had the speed per frame converted as ticks / 2, which scaled the fall twice.
- Floor ray FUN_1017a5d0 mode 0 (FUN_10162470 / FUN_10162d20): from 0.5 above the feet down to 40 below, any face, both
  sides, nearest to the start. No face: the frame is undone and the fall speed zeroed. A face with |n.y| > 0.7 becomes the
  ground normal (FUN_100a8df0); its surface info goes to vt+0x214 / 0x21c. A face more than 0.4 below the feet reports type 0.
- Mode 1 (FUN_10163b70, front faces only) from y - 2.0 (+0x16c; 1.5 in zone 149) down 40 snaps the other actors. FUN_1017a400
  (+0x170 = 2.0, 0.5 in zone 149) is the snap in the remote-actor notes.
- Not done: the actor-body boxes (XiModelActor via FUN_10166760 when DAT_1061dc74 is set), moving platforms
  (FUN_1017a660 -> FUN_1016f800) and the object id skip (+4 vs +0xbc).

### Ours (client/ground.rs)
- walk_player(from, to, ticks) is one frame of the above, with the body state kept in the Ground; a position other than
  the last result starts a new body. mod.rs runs it in fixed retail frames (ground::STEP_TICKS = 2) while we could fall
  (also without input, not in events) and draws the player between the last two steps (drawn_pos), so the physics is
  retail's at any frame rate. The old per-tick scaling probed 0.0004 down a frame at 300 fps and the player bounced
  on every slope (test stays_grounded_on_a_slope). Downhill at run speed one step in three is still in the air (the
  ground move never goes down; retail's rule, asm 0x100a5474: t.y = 0 when t.y > 0, not renormalized), a 10 Hz
  wobble of about 0.02.
- Not retail: the player is drawn through 3-step averages of the physics results (drawn_between_steps). Retail draws
  the collision position each frame (FUN_100a3560 copies +0x5c4 to +0x34 / +0xc4 / +0xd4), so the downhill bob and
  step-ups show there too. The average is exact on a steady slope (one step, 33 ms, late) and cancels the 3-step bob.
  Live hill run in East Ronfaure (41, 201) -> (41, 140) and back: drawn y jerk max 0.051 -> 0.0086, camera look
  0.067 -> 0.0023 (FFXI_TRACE_MOVE now logs the player and camera; scratchpad live/camtrace.py).
- Free camera look point (FUN_1001e030, loop after LAB_1001e5b7): eases toward the head 0.25 of the gap a tick (whole
  ticks, at least one a frame); ours applies it per tick with powf *(inf)*. The eye's distance spring (6 / 100 / 3,
  _DAT_1031f258 / _DAT_1031f234 / _DAT_1031eba8) is not done: our eye is still the yaw / pitch / distance orbit.
- walk(from, to, _) (zone-path, A*) runs the same frames at run speed until it gets there or stops gaining.
- floor() / settle() now take every face (retail's rays do not filter by slope); the old |n.y| >= 0.3 rule, the 1.2 climb
  and resolve_walls are gone.
- Tests on real data: east_ronfaure_hillsides_block (49-64 degree slopes south of the San d'Oria gate, 11 units high:
  walk and 300 held frames gain < 0.8), southern_san_doria_stairs_and_ramps (stairs at (-22, -36), 0.25 risers, -0.25 to
  -3, at full speed; a 39 degree ramp at (80.9, 3, 58) up and down), dat::collision east_ronfaure_mesh, plus synthetic
  step (0.45 climbs, 1.2 blocks), slope (40 walks, 50 blocks) and ledge-fall tests.
- At (80.9, 3, 59) the ramp blocks: the start sits on a seam between two halves of the ramp (some face along z = 59).
  Not checked against retail.
- Live (lnxtest01 / Lnxtest, driven goto runs): East Ronfaure, holding forward 15 s into a 54 degree stone buttress
  outside the San d'Oria walls (foot (95.9, -60, 244), top -64.5) stops at its foot, y -60.0 *(inf: the old |n.y| >= 0.3 rule took these faces as floor)*;
  Southern San d'Oria, the stairs at (22, -36) climbed from -0.25 to the landing at -3.0 at (14.1, -36), then the
  wall. Zoning 101 -> 230 through z2t2z6e1 also went through the new walk. Frame-by-frame heights were not logged live.

## Select Character screen: camera, name list, info panel, Options, prompts, zone names (2026-10-21)

Decompile: all.c / all.asm (FFXiMain), constants read from the unpacked DLL. Research notes: scratchpad select_screen.md
(with corrections below). Inferred items are marked *(inf)*. Offscreen: `select-shot` (now drives client/select.rs:
`--chars N --slot K --hold F --frames F --close --ui list|options|slots|login|delete|reset|settings|ok|rename --delete
--rename K --unusable K --zone Z`). Shots in scratchpad select/ (s*.png offscreen, L*/M*.png live on lnxtest01).

### Frame rate (FUN_100889d0, the frame loop at 0x100124xx)
- The frame loop sleeps until the ticks since the last frame reach DevCtrl +0x30; FUN_100889d0 sets it, always to 2
  (or 4), DAT_10350210 = 2 at load. So retail draws 30 frames a second of 60 Hz ticks, and every "frame" counter in
  menu draw code counts at 30 Hz. This confirms the earlier *(inf: 30 fps)* notes (help scroll, creator).

### Camera (FUN_10238580 setup, FUN_10237a80 move, FUN_10238250 step, FUN_102372c0 refresh)
- Opening: camera slot (+0x14) = cursor = 0 (FUN_102371c0; kept only when DAT_10623ac4 == 1, not modelled), eye =
  slot 0 far + (0,-1,0), target = slot 0 feet + (0,-1,0), both curve parameters 1.0: the full-body shot holds.
- Move to the cursor (force 0 when the cursor moved, returns if it did not; force 1 = refresh, returns when the
  camera's slot is empty or its point was already read). Head = actor vtable +0x1c4 (7, &p) - feet, latched at
  E+0xa8; (0,-1,0) without it. d = new - old, close = |d| in {0, 1, 15}:
  eye mid = close ? (old.far + new.far + oldHead + newHead) / 2 : centre + (0,-2,0);
  eye end = (point valid ? new.near : new.far) + newHead;
  target mid = centre + 0.8 (X - centre), X = close ? (current target + new.feet + newHead) / 2 : feet of slot
  min(old, new) + |d| / 2 (the 0.8 pull applies to both cases: the research note missed it for neighbours);
  target end = new.feet + newHead. Curves P0 = 2 cur - mid, P1 = mid, P2 = 2 end - mid (uniform quadratic B-spline).
- Step: c = L * -1/75 (0x10331e10 is negative), a = L/2 - |P0P1|, b = |P0P1| + 2 a t; t += (sqrt(b^2 - 4ac) - b) / 2a,
  or -c / b when |a| < 0.01; clamp 1. Constant arc speed L/75 a frame over the modelled arc L/2: 38 frames (1.27 s).
- Refresh: +0x28 counts draws; any message to the list zeroes it; past 0x95 the move is retried with force 1 if
  the cursor slot has an actor. Cursor moves call the move only onto a slot with an actor.
- Ring centre dropped onto the ground like the slots. Point 7 = rest bone transform x point offset, facing x size
  scale (as attach_point): Hume M rest (0.165, -1.322, 0) x 0.95..1.05 (unit test select::point7_is_the_chest).
- Client: client/select.rs SelectScene steps whole 30 Hz frames from real time (any frame limit); reset() on every
  opening of Select / Delete (FUN_10238580 rebuilds the scene). Unit test select::glide_is_constant_speed.

### Name list (dbnamese, registry 312 flags 0x1002e: unscaled, frameless)
- FUN_102371c0: window at ((1 - s/2) W - 640, (1 - t/2) H - 480), s = t = 0 *(inf: no writer found)*. Sprite 78 (all
  cells) at the origin; children 1..16 at origin + (304 / 480, 242 + 18 k), sprite 77 each: used 0x80808080, cursor
  0x80805020 at -1 px, empty (state 5) 0x30808080, cursor on empty 0x40402810 at +1 px. Hand anc_s at the cursor
  child *(inf)*. Links from the layout: up / down through 1..16, right / left between columns (links[4] / [5]).
- Names: FUN_102374a0 -> FUN_101c04c0 = the fontshp sprite font (not the game font), fitted to the child width 128:
  pen starts at 8, at the limit the rest becomes an 8 px piece (the ellipsis sprite 0x6c *(inf)*). White
  0x80808080, yellow 0x807f7f00 (E+0x5c must rename), grey 0x80505050 (E+0x60 cannot use).
- The list is filled by FUN_102333b0 from the lobby's character table: status 1 or 2 and a name other than " "
  takes the next entry (empty slots take none), E+0x60 = status 2, E+0x5c = renamef, level 0 shown as 1, area =
  group 4 by zone (zone_no | (zone_no2 & 1) << 8) or "unknown". +0x20 (content IDs purchased) counts every status-1
  slot, named or not; +0x24 (registered) the named status-1 ones. Our List::new does the same from LSB's 0x20
  (world name = LSB main.SERVER_NAME, "Nameless" by default). Unit test select::list_compacts_and_counts.
- Extra draws (FUN_102372c0): lobbywin 143 skipping cell 0 at (0, 0) (the small logo at 32,32); the style frame at
  (0, -16) W x (H + 32), tint 0x20808080 (FUN_1022a290; barely visible, as the 25 % alpha suggests); the info panel
  when the cursor slot is used; the count lines.

### Info panel (FUN_102374f0) and count lines (FUN_102376b0)
- Base y = ftol(-0.8258929 H) negated (H = 800: 660). lobbywin glyph font: name (16, base - 144); labels group 1
  ids 0..4 (World Race Job Level Area) at x 16, values at x 60, rows base - 108 .. - 36 step 18. Values: world name,
  group 6 race - 1, group 3 job, "%d" level, group 4 area.
- Group 4 = file 0xd8aa (55466), loaded by FUN_1014d2b0 for every language: short names ("SSandOria", "Jugner"),
  which suit the glyph font (letters and digits only). Group 5 = 0xd8a9 (55465), the full English names.
- Counts: fontshp at (W - 640 + 400, 35 / 51 / 67), strings in FFXiMain .data 0x1037c1ac / 0x1037c188 / 0x1037c164,
  singular 0x1037c140 when the difference is 1. Other languages draw numbers and a sprite (not done).

### Input and prompts (FUN_102378f0, FUN_102370d0, FUN_10238ea0, ptc6yesn FUN_10231f50, ok FUN_102319a0)
- Enter on a used slot: SE "0002"; then status 2 -> FUN_10233c80(0x74) = FFXI-3116 in the ok window; renamef ->
  ok window with group 8 0xda, then chmkname (help 0x18 = group 0x1b id 4), ptc6yesn type 5 "Register "%s"" +
  0xb0 (No highlighted; No returns to the name), then LSB view 0x28 (char id @28, name @36, reply 0x03; unit test
  net::lobby::rename_lands_where_lsb_reads_it) and the login; names under 3 letters -> FFXI-3110. Otherwise type 0xd
  "Log in with "%s"?" with Yes highlighted. Empty slot: SE "0004". Escape: SE "0003", back to the title.
- Error popups (FUN_10017310): table at 0x10345800, {code, kind, group, id} until 9999; kind 2 shows "Error code:
  FFXI-%04d" (group 7 id 1), a line break and the message (3116 = group 7 0xd, 3117 = group 0x1b 0; lobby codes
  3201..3339 are there too). Codes not in the table: the first line only (FUN_10017380).
- ptc6yesn: centred by FUN_101179a0 ((W - 512) / 2, (H - 448) / 2 added), frame = window + (-256, 0) 512 x 144,
  sprite 95 from cell 1, Yes / No children (sprites 97 / 98, links toggle), warning icon child 3 (hidden for the
  chswin prompts, a5e0 = 1). Text: frame rect clamped 16 px inside the UI area, x0 + 0x30 for all languages but
  Japanese, 28 px off the bottom, laid out by FUN_1011a1a0: a block of 16 px lines centred when it fits, else from
  the corner, wrapped at spaces. Lines by type: 0xd group 10 0x52; 4 group 10 0x29 + group 8 0xae (now also the
  delete prompt, No highlighted); 10 group 0x1b 6; 0xb / 0xc group 8 0xc7 / 0xc9 + 0xec / 0xed (server) or 0xc8 /
  0xca (slot %c), then a blank and the stored file's details (0xee / 0xcb), which need PlayOnline storage and are
  not drawn. menu ok (39542): same centring, frame + (-256, 0) 512 x 208, windowps 104, OK child (142, 182).
- Escape in a prompt answers No *(inf: the result -1 path of FUN_10233cb0 was not traced to a key)*.
- Ctrl / Alt + C (key function 0x3f = DIK_C | 0x10, FUN_102370d0) on a used slot, select only: type 10 "Reset the
  settings file ..." with No highlighted (FUN_10239700 sets +0x14 = 1); Yes -> FUN_101bb470 -> 0xdf / 0xe0. This
  client keeps no settings file, so Yes shows 0xe0 "Unable to delete settings file. File not found." *(inf: the
  file-missing path)*.

### Options (chfwin) and file slots (chswin)
- chfwin (registry 314 flags 0x80002, handler 0x10623af4) opens with the list and stays drawn; flag 0x80000 keeps
  the cursor highlight off while it is not focused (FUN_101144b0). Anchored right: (W - 512 + 368, 84), sprite 216
  ("Options", "+ : Settings Management"), children Load (220) / Save (221); help lobyhelp 0x31 / 0x32 = group 8
  0x102 / 0x103. Opened by message 8 on a used slot (right button inside the list, FUN_102370d0; numpad + per the
  caption and key function 0x11, *(inf)*), SE "0013"; empty slot buzzes. Escape / message 8 close it (SE "0013").
- Enter: chswin (registry 315, sprite 218 "File Slot", children Server, Slot A..D = 222..226) in mode 0 load /
  1 save (FUN_1023a8f0); help = lobyhelp 0x33 + item - 1 + mode * 5 (table 0x10331df0) = group 8 0x104..0x10d.
  Enter -> af0 +0x31 (load) / +0x30 (save) = slot -> the prompt (No highlighted) -> FUN_102398a0 (PlayOnline server
  or the POL HDD files FFXI%d.DAT). Neither exists against LSB / this client: Yes shows retail's not-found results,
  0xef "Unable to save settings file. File not found." / 0xf0 "Unable to load ..." *(inf: the -1 path)*.

### Zone names everywhere
- FUN_101c2090(zone, mode): mode 1 = group 0x14 (language slot 0x10, English file 55465), mode 0 = group 4 with a
  group 0x14 fallback. Map help (helpwind slot 2) uses mode 1; the select Area uses group 4 (short); the zone-in
  "=== Area: %s ===" uses group 5 (55465 too in English).
- topbar-shot --map now looks the name up in 55465 ("Jugner Forest" for 104; t_map_jugner.png) instead of "zone N".
  Missing names log an error: the live map help (once per zone), topbar-shot, the area line (which keeps retail's
  own "=== Area: %d ===" string 0x10356a44), and the info panel (retail's "unknown").

### Live (lnxtest01: Tuxedo, Lnxtest, Lnxcsfix; lobby only, never Yes on login or delete)
- L01 opening far shot, L02 the refresh after 150 frames (5 s) gliding onto Tuxedo's chest, L03 / L04 a neighbour
  move, L06 the cursor on an empty right-column slot with the camera left on Lnxcsfix, L07 numpad + Options, L08
  chswin, L09 / L10 the save prompt and its not-found result, L11 Ctrl+C reset prompt, L12 / L13 the login prompt
  and No, M01 right click Options, M03 click on a slot -> prompt, M04 click No, M06 reopening resets to slot 0's far
  shot, M07 / M08 the delete screen and its type-4 prompt with No highlighted.

### Not done / open
- The delete flow's second confirmation (ptc9dele, FUN_10233c00) is not shown.
- FFXI-3117 (five failed user-file operations at login) has no counterpart: our login is one request, and a lobby
  error still returns to the login screen with the status line, as before.
- The rename flow is implemented but untested live (no character with renamef; the DB was not touched).
- chswin's file details line; other languages' count lines; DAT_10623ac4 (keep the cursor on reopening); the
  creator confirm still uses the old generic yes / no box.

## Config: the option store, the in-game Config menu, lobby Config, Config-tool settings, background resolution, target flash (2026-10-21)

Decompile: all.c / all.asm (FFXiMain) and the US "FINAL FANTASY XI Config" tool (v5.0, decompiled headless). Research
notes with every child, position, help id and address: scratchpad config_research.md (summary) and config/research_a1.md,
research_a2.md, research_c.md; the target flash in scratchpad target_flash.md. Inferred items are marked *(inf)*.
Offscreen: `topbar-shot --config configwi:1,conf2win:3 [--cfgset 0x07=1,...]` (also `lobycwin:5`, `settings:TAB:ROW`, list
pages `name:cursor:scroll`); shots in scratchpad config/c_*.png, live ones in config/L*.png, W*.png, X*.png.

### The option store (gamecfg.rs)
- FUN_1018e330(id, value, kind, key, min, max, default, flag) fills 0x2c-byte records (value +8, kind +0xc, key +0x10,
  min +0x18, max +0x1c, default +0x20, callbacks +0x24, flag +0x28); 198 ids for US (French / German overrides skipped).
  Value and default are equal for every id.
- Set (FUN_1018c7b0 -> FUN_1018e190 -> FUN_1018ecb0): out of [min, max] (signed; max -1 = no range) becomes the
  *default*, it is not clamped; callbacks run when the value changes or flag bit 0 is set. Registering a callback
  (FUN_1018c780 -> FUN_1018e110) calls it at once with the current value. Get FUN_1018c7d0; bit get / set of 0x99
  FUN_1018c810 / FUN_1018c870.
- Kind 4 = cnf.dat per character (FUN_1018e780 / FUN_1018e820), 6 = tig.dat (FUN_1018e3c0), 1 / 2 = PlayOnline-wide by
  key (LSKF1JHT, GOPN1_HF, GOPN1_AO ...), 5 = not saved (0x14, 0x19, 0x32..0x35: server values from s2c 0x0B4).
  Resets: FUN_1018ed00 text colours (0x1b..0x31, 0x9b, 0xb0, 0xb3), FUN_1018ed90 log masks (0xb9..0xbc), FUN_1018edd0
  all.
- Ours: config.json `game` {global (kinds 1 / 2 / 6), characters {name: kind 4}}, keys "0x07", only non-defaults;
  kind 5 lives for the session. Unit tests gamecfg::{table_is_sorted_and_retail, set_follows_ffximain,
  persistence_round_trip}.

### The in-game Config menu (client/configui.rs, configui2.rs, cfgapply.rs)
- Social menu child 7 (FUN_10147fc0 case 7) opens configwi (384, 48) with SE 0002; its 12 rows open Gameplay conf2win,
  Chat Filters cfilter, Font Colors conftxtc, Windows conf5m, Log conf11m, Misc. conf3win, Misc. 2 conf6win, Misc. 3
  conf12wi, Misc. 4 conf13wi, Effects fxfilter, Mouse/Cam. conf7, Global conf4 (FUN_1018f2a0). configwi stays open under
  its page. conf1win / conf1us is never opened (legacy; Misc. 2 took its keyboard rows).
- Events (FUN_10114f40): Up / Down / Right / Left follow element bytes 0x17 / 0x18 / 0x19 / 0x1a (listing links[2..5];
  links[0..1] are prev / next), a link to itself does nothing; SE 0001 when the cursor moves, 0002 Enter, 0004 Enter on a
  disabled child (state 5, drawn with its state-4 sprite), 0003 Escape. Escape closes the top window.
- Every change is applied at once through the setter; there is no OK / cancel and Escape never reverts. Lists
  (cfilter, fxfilter, conf11s) keep a copy stored on close in retail; ours edit the store directly (same result).
- Widgets: two-choice marker windowps 387 at the chosen child's (x + 37, y + 4) (FUN_1018f4a0), three-choice 412
  (FUN_1018f510), 15-px keys 411 at (x, y + 4), auto-disconnect keys 483 at (x, y); sliders: the fill windowps 384 at the
  child scaled in x by the value (FUN_1018f600), 0xa0a0a0a0 while editing; Enter edits, Left / Right step and apply,
  Enter / Escape stop; the hand is hidden while editing. The cursor child stays tinted under the window on top (no
  registry flag 0x80000); the hand is drawn only on top.
- Pages, children -> ids (all in configui*.rs tables):
  - Gameplay: 1 / 2 lock-on 7 = 0 / 1, 3 / 4 SE / music volume 9 / 10 (0..100), 5..7 gamma 0x37..0x39, 8 / 9 autosort
    0x9a = 1 / 0, 10 / 11 sort type 0xca.
  - Misc.: damage display 0x13 (Log 0, Both 1, Screen 2), Show / Hide (not an id: c2s 0x0DC flag 0x04 on close), shadows
    0x3a (High 2, Normal 0, Off 1), weather 0x3b, characters shown 0x3c (slider 25..50), icon type 0x99 bit 2, PC armour
    0xa5.
  - Misc. 2: clipping 0x3e (0..25), footsteps 0x3f, animation rate 0x40 (0..3), keyboard 0x42 (Full / Compact 1 /
    Compact 2, default 2), aspect 0x43 (0..5, "(4:3)" .. "(5:4)" in fontshp at (slider x1 - 20, y + 17)), Movement /
    Commands open k1assign / k2assign, Default = yes/no group 0xb 0x8a then default keys, macro palette 0x9d / 0x9e (child
    12 disabled below 1024 px wide, simplified from FUN_10193e50).
  - Misc. 3: weapon effect 0xbe, style lock 0xbf (c2s 0x053 with a 9 s cooldown in retail), area display 0xc0, target
    expressions 0x99 bit 7, party icons 0xc2, icon timers 0xc3 (range 1..1, so anything but 1 is 0), furniture camera
    0x99 bit 6.
  - Misc. 4: software keyboard 0xc9 (Compact only in Japanese: disabled).
  - Mouse/Cam. (table 0x1036dfc0): 0x90 mouse reference, 0x8f edge panning, 0x91 chase cam, 0x8c / 0x8b third-person Y /
    X invert, 0x8e / 0x8d first-person.
  - Global: language filter 0x18, auto-disconnect 0x41 = 10..60 minutes or -1 (both PlayOnline kind 1).
  - Chat Filters (table 0x1036d9c8, 53 rows of (mask bit, hold bit or 9, label group 0xb)): mask bit < 30 = 0x32, else
    0x33 bit - 30; two states OFF / ON, three OFF / ON / Hold (0x34), Tell OFF / Hold / OFF+icon / Hold+icon (0x47 =
    sound "04p0" on tells), System Lv. 1..3 set 0x35. Badges framesus 89 / 88 / 83 / 136 / 137 at the row, label at
    x + 0x21. 14 rows, Down / Up scroll at the ends, Right / Left page by 14.
  - Font Colors: categories 0x1036dae8 / dba8 / dc68 / dd28 {label, sample, colour, id}; textcol1 rows, textcol2 the
    sample in its colour at (x + 16, y + 20), textcol3 R / G / B (step 4, bars windowps 98 x v / 255 x 1.3 tinted
    0x70800000 / 0x70008000 / 0x70000080), OK writes 0x80RRGGBB, Cancel / Escape drop it; Default = confyn mode 0.
  - Windows: Shared (0xb4 second log window, 0xb5 timestamps, 0x12 frame style = file 14 + n, 0xa4 no open effect),
    Window 1 / 2 (reactive 0x0e / 0xb6, max 0x0f / 0xb7 4..20, min 0x10 / 0xb8, width 0x9c / 0xc5 step 10, resize time
    0x11 / 0xc6 3..30; window 2's -1 follows window 1; max / min pull each other; the counts in fontshp at
    (x1 - 16, y0 - 14)).
  - Log: conf11m -> conf11l (Chat / For Self / For Others / System) -> conf11s rows (tables 0x1036de00..df50) toggling
    the window's 64-bit mask (0xb9 | 0xba, 0xbb | 0xbc); Right / Left switch the window; Default = confyn mode 1.
  - Effects (table 0x1036daa0): 18 bits of 0x92.
  - confyn (384, 140): cursor on No; Yes resets the colours (mode 0) or the log masks (mode 1).
- A window under a later one at the same place (k1assign over conf6win, textcol3 over textcol2) is not drawn *(inf)*.
- Not done: keylayou (the compact keyboard picture under Compact 1), the list scroll bars (FUN_10115210), the style
  lock packet and its cooldown, the Show / Hide packet, the macro palette's position rule, auto-disconnect's
  PlayOnline timer.

### What the ids do here (taking effect at once unless noted)
- 7 auto lock-on; 9 / 10 SE / music under our `effects_volume` / `music_volume`; 0x49 field of view (free and locked
  cameras); 0x0e / 0x0f / 0x10 / 0x11 / 0x9c log window 1 size and timing (LogCfg; fixed size = min lines = max);
  0x12 window frame style (files 14..21, textures swapped in the UI renderer); 0xb4 the full log's second-window tab;
  0xb5 timestamps "[hh:mm]" / "[hh:mm:ss]" (12-hour, local time) on chat lines (*(inf: placement)*; retail colours the
  stamp with 0x31, ours keeps the line's colour); 0x1b..0x31 / 0x9b / 0xb0 / 0xb3 text colours (FUN_101262c0 maps
  them to colour ids 0x54..0x6e, replacing the palette's static entries) for battle and chat lines, new lines only;
  0x32 / 0x33 battle message filters on the client (FUN_100abf20: kind table 0x10350a60, 11 relation tables from
  0x10351bbc; party / alliance relations not modelled); 0x3c nearest-N other PCs drawn *(inf: which are dropped)*;
  0x43 projection aspect when our `retail_aspect` is on (aspect = 1 / ((h / 0.75 w) x 0.791), FUN_100147a0); 0x8b /
  0x8c invert the mouse camera; 0x8f edge panning (pointer within 2 px of the left / right edge turns 1.5 rad/s *(inf)*);
  0x91 chase cam (the free camera eases toward the facing while moving *(inf)*); 0x4a..0x4d move keys and 0x66 / 0x67
  turn keys (keyboard type 2) from Key Assignment; 0x37..0x39 gamma (below); 0x99 bit 7 gates the forced target flash.
  `/targetcharaeffect` (cmd 0x82, FUN_1007d800) toggles 0x99 bit 7 and prints group 9 0x137 / 0x136.
- 0x13 Damage Display: read at print time (MODLOG "On-screen damage numbers").
- Stored and shown only: 0x3a, 0x3b, 0x3e, 0x3f, 0x40, 0x42, 0x99 bits 2 / 6, 0x9a, 0xca, 0x9d, 0x9e, 0xa5,
  0xbe..0xc3, 0xc9, 0x18, 0x41, 0x34, 0x35, 0x47, 0x90, 0x8d / 0x8e, 0x92, 0xa4, 0xb9..0xbc, window 2 ids, 0xac, 0xad.

### The lobby title's Config (lobycwin)
- loby2win's children are Select, Create, Delete, Back (4), Config (5); title index 4 = Config (FUN_102357b0), drawn above
  Back. Our menu index is in screen order, so the title now maps it to the child (CHILD = [0, 1, 2, 4, 3]); before, the
  highlight sat on Back while Enter on it ran Config (and the reverse).
- FUN_10235fa0 opens "menu lobycwin" (39541, (100, 100) unscaled, no repositioning in its open FUN_10195ef0), the title
  inactive meanwhile; FUN_10195ef0 reads 0xa8 / 0xab / 0xac / 0xad / 0xaf and disables tracks 7..11 whose expansion is
  not owned (FUN_1023ae80: bits 0, 2, 3, 4, 11), cursor on 5. Children: 1 volume 0xa8 (SE and music, FUN_10196390), 2 / 3
  logout destination 0xac (stored), 4 aspect 0xab, 5..11 title music 0xaf (5 newest = 0, 6 random = 1, 7..11 = 2..6;
  the music restarts at once). Help: group 8 0xf5..0x101 by child. Escape (FUN_10196310) closes it; FUN_10195fb0
  writes the values back. Kind 6 = our `game.global`.
- Child 5's sprite uses texture "menu    ustats" from file 39551 (see the title music section below).

### The Config-tool values (config.json `tool`; our settings screen)
- FUN_10014a50 -> FUN_100028e0 -> FUN_10002730 -> FUN_10016aa0: HKLM\SOFTWARE\PlayOnline\SQUARE\ (JP),
  PlayOnlineUS\SquareEnix\, PlayOnlineEU\SquareEnix\ + FinalFantasyXI; FUN_10016b00 defaults, FUN_10016c30 reads the
  table at 0x10345650 {type, &global, index}, name "%04d"; a missing value is written back. Floats as raw DWORD bits.
  `-0001sound off` on the command line overrides 0007.
- 0000 MIP filter (1 LINEAR, 0 NONE), 0001 / 0002 window = back buffer size in every mode, 0003 / 0004 background
  render texture, 0007 sound, 0011 environment animation 0 / 1 / 2 = 1 / 16 / 32 steps, 0017 bump mapping (VS / PS 1.1),
  0018 texture compression (category nibble <= value stays uncompressed; 2 = all), 0019 map images, 0020 actor update
  thinning (no UI), 0021 hardware cursor, 0022 opening movie, 0023 simple creator, 0028 gamma base, 0029 SE at once
  (a new one is dropped at the limit, FUN_10037510), 0030 3D LCD, 0034 window mode, 0035 DSBCAPS_GLOBALFOCUS, 0036 font
  (0 DXT3 moji, 1 39543, 2 39544), 0037 / 0038 UI size, 0040 skips D3DRS_WRAP0 = 7, 0042 user directory, 0043 screenshot
  size; 0024 / 0031..0033 / 0039 / 0041 never read; 0044 / 0045 tool only.
- Window modes (FUN_10015830): 0 full screen WS_POPUP | WS_EX_TOPMOST, 1 WS_OVERLAPPEDWINDOW, 2 WS_POPUP at 0001 x 0002,
  3 WS_POPUP at the desktop size; Windowed = (0034 != 0). No Alt+Enter, no in-game change; Alt / F10 blocked.
- Gamma (FUN_10016400): ramp = 65535 (i / 255)^(1 / max(base + g, 0.01)) through SetGammaRamp, full screen only.
- Ours, defaults where they differ from FFXiMain's: 0000 = 1, window 1280 x 800, background and UI 0 = the window's,
  0011 = 2, 0017 = 1, 0034 = 1, 0035 = 1. Our extras on the same screen: frame limit, anisotropy, party TP, music /
  effects volume, `retail_aspect` (3D aspect from 0x43 / 0xab instead of the window's; off by default).
- Live: 0001 / 0002 and 0034 (winit: 0 exclusive at the nearest video mode, borderless where there is none; 1 a
  decorated window; 2 undecorated at the size; 3 borderless at the desktop; test runs are always windowed), 0003 /
  0004, 0037 / 0038, 0028, 0007 (drops / reopens the device), 0029, 0035 (mutes while unfocused), frame limit, party
  TP, volumes. Next zone load: 0000, 0018, anisotropy. On restart: 0036. Stored only: 0011, 0017, 0019, 0021, 0022,
  0023, 0030, 0040, 0043.
- FFXI_CONFIG=path points a run at another config.json (test runs use a copy).

### The settings screen (client/settings.rs; NOT retail)
- Retail set these in the external Windows tool before starting; this screen is our addition, by request. Reached
  from a "Settings" button under configwi and under lobycwin (Down from the bottom row, or the mouse). Tabs as the
  tool's (General, Screen, Textures, Effects, Sound, Misc.) plus Linux; rows from blank config buttons (windowps 105's
  buttonto cells), the config pages' bullets and header strip (windowps 465 cells 2 / 1), the slider track / fill
  (383 / 384) and the game font. Up / Down rows (row 0 = tabs), Left / Right / Enter change, Tab next tab, Escape
  closes; each change is saved and applied at once; the help line says when it takes effect.

### Background resolution (render/scale.rs)
- Retail (FUN_10010080, FUN_10010d20): CreateTexture with D3DUSAGE_RENDERTARGET at 0003 x 0004 (A8R8G8B8 first) plus a
  depth surface; the world draws there, then FUN_1000b4f0 draws it as one full-window quad (-0.5 px, LINEAR min / mag,
  no mip, clamp, MODULATE2X 0x80 = 1.0); then the UI at window size, or into its own 0037 x 0038 target stretched by
  FUN_1000a020. A half-size target exists too *(inf: glow)*.
- Ours: when the background size (or gamma) differs from the window, the scene (zone, actors, flyover, select,
  creator, the crossfade and fade passes) draws into an offscreen target at that size with its own depth, then one
  pass scales it to the window: down = a box of ceil(src / dst) bilinear taps per axis (up to 8), up = Catmull-Rom in 9
  bilinear taps; gamma per channel in the same pass. The projection keeps the window's aspect (or the retail ratio),
  so the image is never distorted. The UI, name plates and text stay at window resolution (or the 0037 x 0038 layout,
  stretched by the UI pass's screen uniform). Unit test render::scale::scales_both_ways.
- Gamma: (base + 1 + v / 100) / 1.5 per channel, so the defaults are neutral, applied to the scene in the scaling pass
  *(not retail: retail ramps the whole screen and only in full screen)*.

### Target flash (client/flash.rs; FUN_100c94a0, asm 0x100c991f..0x100c9c87)
- The actor drawn as the target (FUN_10151430, else DAT_10475550), any kind: on the first such drawn frame with the
  latch (+0x840 bit 2) clear, 3 pulses of 40 ticks; k = (t <= 20 ? t : 40 - t) x 0.05; ambient = mix(ambient, 220 / 255,
  k), fog colour x k; then nothing until it stops being the subject for a drawn frame. Forced (+0x88 bit 0x200, set every
  frame by the sub-target cursor / area previews unless 0x99 bit 7) wraps the count 0 -> 3 and clears when a pulse ends
  at 0.
- Ours: per actor in 60 Hz ticks (not clamped to 1 a frame, so any frame limit gives retail's timing), only while drawn;
  (k, k) goes in pose[4].yz to vs_skinned and lit_actor (zone.wgsl). target::force_flash exists for the future `<st>`.
  Unit tests flash::{triangle_pulses, three_pulses_then_rearm, forced_runs_on_then_clears}.
- : a GPU-skinned Hume at night in East Ronfaure with k = 0 / 0.5 / 1
  (scratchpad config/flash/flash_cmp.png: the peak is clearly brighter at night). Not checked live: the map server was
  down when the live run was due.

### Login errors
- A lobby or login error used to return to a black login screen: the world entry's fade-out stayed at 0. Now the
  fade restarts and the error shows in retail's OK window (menu ok, FUN_102319a0) with "Error code: FFXI-%04d" and the
  table's message for a known code (FUN_10017310), else the text; Enter / Escape / a click dismisses it (live X1).
  Every way back to the login screen (lobby errors, a refused world entry, a disconnect, a logout) goes through
  App::to_login, which restarts the fade; errors are also logged.

### Open
- keylayou, list scroll bars, the Show / Hide and style-lock packets, chat-type filters (server), second log window,
  window open / close effect, shadows / weather / footsteps / damage pop-ups (the features), first-person camera, the
  mouse movement reference, 0x3e clip scale, 0x40 animation step.

### lobycwin title music, the newest icon, cell alpha (2026-10-21)
- Option 0xaf -> (mode, n) table 0x10331ed8 (FUN_1023ae50 -> FUN_10036980): 0 (0, 0) newest, 1 (2, 0) random, 2..6
  (1, 0..4) fixed. Mode 0 starts at n = 4 and keeps the follow-up; 1 and 2 set +0x54 (no follow-up); random n =
  timeGetTime % 5. n -> (bit, entry) FUN_1023ae80; an n whose bit is clear steps down to n - 1 (never up). Entries
  {track, ticks, next} 0x10346580. Per option (child): 5 newest, 6 random, 7..11 = "1".."5":
  - "1" 108 (bit 0, base), "2" 242 (bit 2, CoP, sound3), "3" 179 (bit 3, ToAU, sound4), "4" 140 (bit 4, WotG,
    sound5), "5" 58 (bit 11, SoA, sound9); fixed options loop their track.
  - newest = the highest installed of those: its track for its ticks (242 8100, 179 7140, 140 8160, 58 8160; 60 Hz,
    so 58 runs 136 s), fade over 0x168 ticks, 4 ticks, then 108 looping (FUN_100365e0 states 5..8). Base only: 108.
  - random: one of the five, falling back below a missing one.
  - Choosing one (FUN_101960c0 case 5..0xb) sets 0xaf and calls FUN_1023ae50 at once; nothing happens when the
    track is already the wanted one (DAT_10451008), so newest after "5" stays on 58 without the switch to 108.
- The mask is NOT the server's. DAT_10444110 + 0x10 is the installed-content mask: init 1 (FUN_100149d0), FUN_101b9b80
  ORs a bit per ROMn with both /ROMn/VTABLEn.DAT and FTABLEn.DAT: ROM2..5 -> bits 1..4, ROM6..8 -> 5..7 (only with
  ROM2), ROM9..13 -> 11..15, then 0x700 (Abyssea) when bits 1 and 4 are set. The lobby step 0xd (FUN_100ea960)
  sends it; the server's reply goes to +0x14 (step 0xf). FUN_1023ae80 (title music) and FUN_10195ef0 (lobycwin
  disables child n + 7 with FUN_10115840(5, ..)) read +0x10 only; the title logos (FUN_10235810) need +0x10 and
  +0x14. This install has ROM2..ROM9 -> 0xfff; LSB here sends 0x07ff (login.lua SEEKERS_OF_ADOULIN = false).
- Before: we used the server mask, so "5" was disabled and newest fell to 140 = "4" (the "same theme" report).
  Now dat::installed / installed_mask (App.installed) drive title music, lobycwin's disabled tracks and (ANDed with
  the server's) the logos; the login screen plays the same title music, so entering the lobby no longer restarts
  it. With this install: newest 58 then 108, "1".."5" all enabled and distinct, random one of the five.
  Unit test audio::tests::title_choice_follows_ffximain.
- Child 5's "menu    ustats" (16x16 note icon) is an IMG of file 39551 (0x9a7f, "menu    magiconw" icons), which
  FFXiMain loads once at startup in FUN_10010080 with 82, 81 (table 0x1037de80 rows 0x57 / 0x58), 39542, the window
  style and 39560 (0x9a88). 39551 is now a companion of the lobby UI (dat::ui::ICONS); no name collides with
  39541 / 39542 / 14. (39560, 81, 82 not loaded: nothing of ours references them.)
- topbar-shot: `--config login` draws the credentials window; `--installed MASK` overrides lobycwin's mask.
- Cell colours (FUN_1011adf0, asm 0x1011ae93..0x1011af1d) are now transformed at load in dat::ui::cell_color (and the
  font's gaiji parts): RGB + 1 below 0xff, alpha 1..0xfe -> min(ftol((a + 1) x 1.5), 255). This covers every 0x31
  sprite cell: window style pieces (files 14..21 "win00"), all menu / lobby / icon library cells. Not plain rects.
  sprite_tint no longer adds the 1 itself. Unit test dat::ui::tests::cell_colour_as_ffximain_loads_it.
  Opacity of DXT1-textured cells x 1.53 for alpha 0x20 (window background win00 0: 25 % -> 38 %), x 1.52 for 0x40
  (alt background 9, top / bottom edges: 50 % -> 76 %), 0x08 sides 6 % -> 10 %; 0x70 / 0x7f cells reach 100 %.
  UI stage 0 is MODULATE2X for colour and alpha (FUN_10159ae0), so for DXT1 textures (alpha 1.0) ours now equals
  retail. *(inf, open)*: FFXiMain uploads DXT3 / palette textures with their PS2 alpha (FUN_10039010 gives RGB24
  pixels alpha 0x80), while our parse_img doubles it for the zone's MODULATE4X; in the UI that makes DXT3 cells
  up to 2 x retail's alpha (a full DXT3 texel with cell alpha 0x7f is ~0.80 in retail, 1.0 here). Not changed.

### Portability notes (Windows / macOS, not done; 2026-10-21)
- The stack is cross-platform: wgpu (Vulkan / DX12 / Metal), winit, cpal (WASAPI / CoreAudio), egui, tokio, rustls, msi, unrar. BC textures work on Windows GPUs and Apple Silicon, and the RGBA fallback exists.
- Linux-only spots to gate:
  - Cargo.toml: cpal features "pipewire" / "pulseaudio" become target-specific (`[target.'cfg(target_os = "linux")'.dependencies]`).
  - client/cfgapply.rs: `Command::new("date")` for the UTC offset needs a portable replacement.
  - main.rs: `std::os::unix::fs::OpenOptionsExt` (0600 files) under `#[cfg(unix)]`, a plain write elsewhere.
  - Tests with hard-coded /home/lenon paths (atrac3) skip when the files are missing.
- The Wayland window workarounds (fullscreen re-request after mapping, the full-screen hop to change monitor, delayed size requests) are harmless elsewhere. On Windows and macOS, set_outer_position can move the window between monitors directly.
- Windows can be cross-compiled from Linux (cargo-xwin or x86_64-pc-windows-gnu); macOS needs a Mac to build. Both need a real test machine for audio, full screen and the GPU.
- Rename the crate ("vana-client") before shipping other platforms.


## Lock-on on engage, walking hitches: GPU mips, budgeted uploads, shared models (2026-10-21)

Decompile: all.c / all.asm (lock-on items traced by a sub-agent; notes in scratchpad lockon/). Inferred items are
marked *(inf)*. Profiling logs and scripts: scratchpad stutter/ (walk.sh, spikes.py, load.sh; q_*.log are the clean runs).

### Lock-on
- The auto lock was firing. "lock off" is only logged when the lock was on: the user's first engage stayed locked
  until the kill; in the second the lock went 5 s in with no target drop, i.e. a player input. Ours let Escape,
  a click on nothing, a click on another actor and Tab clear or switch the target, and any of them unlocked.
- FUN_10151f20 (the target-action runner) does nothing while locked (+0x5c & 3) unless in sub-target mode: Tab,
  Escape (key 0x92: bindings N + 0x10, Esc + 0x80), right-button release (0x8c) and the mouse pick (0x8b) are all
  refused. Now target::picks_allowed. A left click on nothing never clears the target (FUN_10151d20 keeps it and
  runs FUN_101524e0(0), the confirm key on it): ours keeps it and talks to an NPC target *(inf: monsters take no
  action on that click)*.
- Only s2c 0x058 locks (LAB_1009e570, unchanged; LSB sends it from CPlayerController::Engage). FUN_101514a0's
  engaged auto-pick and /attack only target. Callers of FUN_10150da0: key 0x30 and /lockon (cmd 0x59) toggle; the
  rest unlock (status leaving 1 / 3, death, +0x22 = cursor moved off the actor to a ground point, invalid target,
  target change, server-forced position FUN_1008b680 *(inf)*, Mog House layout). FUN_10152770 re-locks after
  sub-target selection.
- Key 0x30 = DAT_103619f0 + 0x30*6 = [0xc037, 0, 0]: Numpad * only (not while typing, no Ctrl / Alt); Key
  Assignment never rebinds 0x30 / 0x44. Compact keyboards (config 0x42 = 1 / 2, default 2): FUN_1011fbc0 turns
  the key of config 0x60 (default DIK 0x23 = H) into pad bit 0x400, the bit key functions 0x30 / 0x44 / 0x7e read
  (pad callback FUN_1011dc20), so H is Numpad * there. Implemented. The physical pad button is the 5th entry of
  the pad list (FUN_10004eb0, local_f0[4] = 10) *(inf)*.
- Facing: nothing turns the player toward the battle target while engaged and unlocked; while locked, FUN_100a6210
  sets the yaw only on movement input or auto-run. Ours keeps facing the target every frame while locked (the
  user's spec; differs from that reading *(inf: retail's schedules may turn the attacker)*).
- Config 7 = 0 by default = auto lock-on; cfgapply maps it as LAB_101a2c60 (+0x51 = (v == 0)). Not flipped.
- Logs now say why: "lock on 0x… (engaged, auto lock-on | Numpad * | /lockon)", "lock off (target changed |
  Numpad * | disengaged | dead | target dropped | no target or an event)", and "0x058 assist …". Unit test
  target::tests::engage_locks_by_default.

### Walking hitches: what they were
FFXI_PERF=1 now logs every frame whose start-to-start gap is over FFXI_PERF_SPIKE ms (25) with its spans and what
was loaded or created in it (client/perf.rs note / timed). Baseline (old code + only this logging, same routes):
- Every new actor in range: texture mip chains box-filtered and BC re-encoded on the UI thread, 1..9 ms per
  texture, 25..40 ms a model.
- Its animations packed on the UI thread, then the whole animation-key buffer re-uploaded: sources were keyed by
  Arc address and every respawn loaded the look again, so it only grew (2 MB to 42 MB within 2 minutes in 230),
  and each new model cost 100..470 ms. The GPU then spent as long again before the next frame.
- Each actor had its own copy of a model (no sharing of identical looks); equipped NPCs of one race each carried
  the same ~600k-word motion set.
- The zone swap: 250..300 ms on the UI thread (ZoneRenderer::new: shader compile, geometry, 60 MB of textures).
- Any CPU-to-GPU copy is slow on this machine: gpu-allocator puts CpuToGpu staging in DEVICE_LOCAL | HOST_VISIBLE
  memory (vulkan/mod.rs), i.e. the BAR, and over the eGPU's Thunderbolt link that measures ~240 MB/s (262 KB
  write_texture 1.1 ms, 4 MB 17 ms).

### The fix
- Mips on the GPU (render/mipgen.rs): level 0 is uploaded as the file's own DXT1 / DXT3 blocks (BC1 / BC2; DXT3
  alpha doubled at parse) or RGBA; compute passes per level: cover0 (level-0 coverage at 255 x ALPHA_REF and the
  lowest alpha), down (2x2 box in linear light, alpha histogram), solve (the coverage scale: smallest in
  [0.25, 8], 16 steps, as the CPU did), encode (BC1 / BC2 by the same bounding-box inset 1/16 / nearest-entry
  rules, or RGBA), then copy_buffer_to_texture into the levels. Lower levels stay BC, so VRAM is unchanged. Jobs
  run under 4 M level-0 texels a frame. No mip work is left on the CPU (dat/texmip.rs keeps only ALPHA_REF and the
  DXT3 alpha fix; the old chain lives on as the test reference). Unit test render::mipgen::tests::
  gpu_mips_match_cpu_chain: RGBA levels within 1 of the CPU chain, coverage within 1/16, BC1 levels decode within
  12 of the CPU's (one 565 endpoint step) with a mean under 1.5.
- Copies under a time budget (render/upload.rs): 2 ms of copying a frame in the world (8 ms while the zone
  indicator is up) at the measured rate (EMA of bytes / ms per copy), textures in row bands, buffers in pieces.
- Models: loaded looks are shared by key (World.models, cap 96 kept when unused; one load serves every actor
  waiting); the loader thread packs the animations position-independently (dat/animpack.rs: key offsets relative
  to the animation; shader chan(an, c, frame)) with a content hash, and builds the skinned vertices
  (render::SkinMesh). ZoneRenderer uploads each model once (vertices, then textures band by band) and clones it
  per actor; GpuAnim keeps one copy of a hashed animation block, appends only new words (a larger buffer takes
  the old ones by a GPU copy) and uploads them under the budget; a model is drawn once its words, textures and
  mips are all in (not drawn until then; no stand-ins). A motion file whose words are not up yet sits a frame out.
- Zone swap: the geometry is built on the loader thread (render::ZoneGeometry); ZoneRenderer::streamed uploads it
  and the textures over the next frames; the "Downloading data" indicator stays until ready(). Zone, posing and
  mip pipelines are made at start-up on a worker (Gpu::warm, PipeCache) and shared by every ZoneRenderer.
- The placeholder boxes for players without a model are gone (standing rule).
- Test drives end with "/shutdown on" (c2s 0x0E7 Mode 3, Kind 3; LSB ignores Mode 1 with Kind 3).

### Numbers (lnxtest01 / Lnxtest, 2560x1440 window, limit 144, RX 6800 XT eGPU; in-world time, the drive's own
### screenshot frames excluded; card0 busy <= 2 % and CPU >= 94 % idle before each run, xi_map ~20..60 % of a core)
| run | route | spikes > 25 ms | > 100 ms | worst frame |
|---|---|---|---|---|
| baseline q_base_a | 230 -> 101 | 110 in 118 s (56/min) | 104 | 468 ms (new model: anim re-upload) |
| baseline q_base_b2 | 101 -> 230 | 59 in 129 s (27/min) | 13 | 316 ms (zone swap) |
| new q_new_a | 230 -> 101 | 4 in 147 s | 1 | 113 ms (wait, 1.3 ms of work) |
| new q_new_a2 | 230 -> 101 | 2 in 149 s | 0 | 27 ms |
| new q_new_b | 101 -> 230 | 3 in 128 s | 0 | 37 ms |
- No new-build frame did more than 25 ms of work, zone swaps included. The few spikes left are waits before the
  redraw starts with ~1 ms of work (compositor / driver *(inf)*). q_new_a3 (311 wait-only spikes, fps dipping to
  57 with 1.2 ms of work a frame) looks disturbed from outside and is not counted. Runs while another application
  loaded the GPU / CPU were discarded.
- Still on the UI thread: the first frame after start-up (UI renderer 210 ms, lobby mips), the drive's PNG encode.

### Open
- The lock-on live check did not complete (no monster in /attack range when the drive looked; stopped so the user
  could test). The unit test covers the 0x058 rule; the input refusals are untested live.
- Right-button release clearing the target (key 0x8c) is not implemented (ours drags the camera with it).

## Effect generators: zone, weather, sprites (fx/, render/effects.rs, effects.wgsl) (2026-10-22)

Specs with addresses: ~/ffxi/lab/research/effects/ (generators.md, triggers.md, zone_effects.md, model_draw.md,
d3a_particles.md). Summary of what the client does, and where it departs from them:
- **Libraries** (fx/lib.rs): a DAT as a folder tree; generators, keyframes, D3m / MMB meshes and D3a sprites by
  scope (FUN_10071310: the folder subtree, enclosing folders, then DAT 0). A D3m that is not found falls back to an
  MMB of the same name (FUN_100485c0). Textures: FUN_1003a0d0 (16-byte name from the first IMG of the resource
  folder to the end of that IMG's folder), then the last 8 bytes among the zone and DAT 0 `syst/effe` IMGs.
  MMB type 2 draws nothing as an effect.
- **Start**: at zone load every generator with flag 0x1000, attach mode 0 / 0xe / 0xf / 0x14 and no `weat`
  ancestor (FUN_10046d30). Weather: `weat/<tag>` (fallback `suny`, then `weat`) on zone-in and on 0x057
  (u16 @0x0a Vana minutes x 2.4 s crossfade, weight into update op 0x50; FUN_10183780 / FUN_101804d0).
- **CPU** (fx/emit.rs, fx/mod.rs): FUN_10048740 (generator stream ops 0x04-0x14, 0x17, 0x0a; persistent single
  element; countdown with rand(+0xd4) + 1 + +0xd6), FUN_1004d920 (op 1 and the emission ops of B.4), life
  countdown and the end stream (op 5 restart, flag 0x20000000 hold), child generators 0x3c / 0x44 / 0x53 / 0x6a.
- **GPU** (effects.wgsl): the element update stream FUN_100499d0 (the C.4 ops listed in the shader), then the
  transforms of FUN_10043f00 / FUN_100440d0 / FUN_10044f00 (Euler = Rz·Ry·Rx in row vectors, view matrices b90 /
  d10 / d50 / c10, billboard L·Rx(pi) with the near clamp, sort key, projection depth bias). A CPU reference of the
  update stream checks the GPU in tests (render::effects::tests). Sort keys, expire requests and positions come
  back a frame or two late.
- **Draw**: CMoOT order (keys clamped 0..512, larger first). Blend by +0x16c (unknown modes keep the previous);
  z test LESSEQUAL, z write by flag 0x1000 (never for D3a); stage maths of FUN_1017c840 (MMB), FUN_1003f3e0 (D3m)
  and FUN_1003db60 (D3a); TFACTOR alpha = A x fade only below 1; scenery fog on view depth (black in mode 0x48,
  none for D3a or flag 25). Batched D3a (CMoVuAnmElem): particles from op 0x56 (FUN_10063060), positions in the
  vertex shader (FUN_10062cd0, with the inverse view rotation and the (2n+1)/3 drawn count as coded).
- **Inferred / left out**: our random sequence is not retail's MT19937 (only the value mappings are); sun / moon
  (modes 0xe / 0xf) lack the rotation of DAT_105ec978; shading is done in gamma space (textures uploaded as plain
  UNORM) and, since 2026-10-23, blended in gamma space as D3D8 did (the world scene below); batched sprites use
  normalised UVs (retail draws raw UVs there; differs only for D3a with byte 7 = 0); no ZBIAS 15, no MMB
  draw-distance cull, no zone-0xed MMB offset; weather density (config 0x3b) is taken as 1.0; the covered (roof)
  flag is not computed; orientation modes with bit 0 (camera basis) use the plain angles; ops not handled yet are
  logged once at debug level (point lists, paths, ground snap, sounds, point lights, distortion, rings, lens flares,
  class-specific ops). Scheduler-started effects (battle, spells) come next (triggers.md).
- Checks: `zone-shot 232 --effects N --weather fine` (sea layers, entrance shadows, cloud domes, sun, tower and
  wall lamps); live in zone 101 (cloudy dome).

## Effect schedulers: melee sparks, casting, spells (fx/sched.rs, fx/tasks.rs) (2026-10-22)

From triggers.md. Every 0x028 action goes to `Effects::on_action` besides combat.rs (which keeps the motions,
reactions and battle messages):
- Categories 1 / 2: the cmd_arg scheduler ("atk0") from the attacker's motion files newest first, then its model
  files (FUN_100cc440), else DAT 0; actor1 = attacker, actor2 = first target; op 0x24 runs the swing combat.rs
  picked. Categories 3-6, 11, 13-15: file base (FUN_100a7380) + the first record's animation, loaded in the
  background, then FUN_10083d60(caster, 0) and its "main". Starts (7-10, 12): the cmd_arg scheduler on the
  caster itself, "ca??" through FUN_10083d60 (stored 4cc; the next start or the finish runs "st??").
  Several records outside 14 / 15 run at speed 0.7 (FUN_10055cc0). 0x038 runs its 4cc on the actor.
- Tasks (CMoSchedularTask): countdown -= ticks, ops run while it is <= 0, each adding wait x speed. Ops: 0x02
  generator clone on the pair (life = duration x speed, 1 when 0, unless persistent or flag 0x1000; flag 0x1000
  soft-stops older clones on the pair), 0x1e kill, 0x2d soft stop, 0x03 / 0x73 child, 0x3b and 0x3c
  call-and-wait, 0x09 on actor2 swapped, 0x57 / 0x85 on actor1, 0x5f kill tasks, 0x24 / 0x5d / 0x5c swings
  (+0xa0 = 10: the parent resumes 10 ticks early), 0x15 / 0x16 / 0x22 / 0x23 dolls (clones move to the doll),
  0x17 / 0x18 undo, 0x31 / 0x32 record loop, 0x30 per target, 0x19 effect file by id, 0x3d..0x3e random op,
  0x6b stack ops with the scheduler variables (0x21..0x39), 0x64 / 0x65 conditional clones of the 0x69..0x6a
  blocks (else after 0x67).
- Actor lookups: part files (vtable +0x1ec), then motion and model files; Character now records `files` and
  `part_files`; actor files load in the background when a model arrives (Effects::preload).
- Attachment (FUN_1003afd0): modes 1-8, 0xd, 0x10-0x13, 0x16-0x1b with points from the rest pose; 9-0xc (bone
  matrices) use the point and the actor's rotation *(inf)*; two-point modes set +0x90 and scale A x. Special
  points 0x30 (actor position *(inf: retail rays to the ground)*), 0x31 / 0x32 (body point 0xd..0x14 nearest the
  other actor), 0x33 (nearest the eye), the rest point 0 *(inf)*. The matrix is taken at start and again every
  frame only with generator op 0x11 (lerped by its k, translation bit 0, rotation bit 1).
- Child clones 0x53 / 0x6a: Euler(element rotation) through the emitter rotation (and c10 for 0x6a) at the
  element's world position, scales from the element; 0x44 keeps the clone's own placement.
- Not done: sound ops 0x0a / 0x0b / 0x53 / 0x60 / 0x78 (no positional SE playback yet), colour fades 0x29 /
  0x2a, op 0x80, weapon ops 0x75 / 0xa3 and their !w schedulers, per-race weapon-skill files (FUN_100d0570),
  element ops 0x25 / 0x33 / 0x46 (per-frame child emission), the config 0x92 effect filter, scheduler-owned
  children waits, stores 0x14..0x18.
- Checks: tests `fist_hit_sparks` (Hume fists on an orc: hit1 g010..g013 at tick 45 = dada 40 + 5; the orc back:
  hit2 g020..g023), `cure_cast` (ner2 circle, stwh, shwh sparkle, tgt0); `fx-shot 232 --category 1` and
  `--category 4 --anim 144` (Fire: flare, ring, burst).
- Melee timing (combat.rs): battle messages at the swing hit (dada) + 4 ticks, the damage reaction at + 5 (atpr
  wait 4 before op 0x2b; dam0 op 0x01 wait 1), as triggers.md 2.2 has it; it used to be both at the hit.
- Weapon skills on PC skeletons (FUN_100d0570): tables at 0x10353be4..0x10353cb8 by race code 0..8 (read from
  the DLL): kind 3 anim < 0x100 file 33227.. + anim, its second file (body info byte 9 > 0; 0x...c30 table when
  it is 1, else c32), anim >= 0x100 61451.. + anim - 0x100, kind 14 58041.., kind 15 88035... The files go on
  the actor list as motion files (actmotion Kind::Skill) and main runs once they are there. Retail drops them at
  the task end (+0xac = 0x100a7440); ours stay until the next skill *(not traced)*.
- Covered (FUN_100136a0 / FUN_101845c0): a ray from just above the eye 100 down, else from just below it 15 up;
  the nearest face's collision object +0xa4 bit 2 sets it. A change stops (elements killed) or restarts the
  transient weather emitters with op-1 flags 4 / 0x4000000; new weather sets skip them while covered
  (client/ground.rs `covered`, fx `set_covered`).

## Effect sound (fx/sound.rs, audio voice handles) (2026-10-22)

From ~/ffxi/lab/research/effects/footsteps_sound.md (sections 2, 3):
- Sound elements (kind 0x3d, CYySoundElem): SeSep data @8 = se id, @0xc bit 31 = loop. Emission ops 0x4c (near
  op+8, far op+4, volume 127; a persistent sound generator stops emitting; world generators height mode 1), 0x6b
  (the source rides the nearest point of an Sph capsule chain: points from block u32 @0x10 + 0x10 at 0x20 bytes
  (x, y, z, r), segment pairs from block u32 @0x14 + 0x10; pulled in by the radius, zero inside *(inf: the clamp,
  FUN_10054fb0 is garbled)*), 0x8a keep, 0x92 / 0x9e height mode 1 / 2; update op 0x43 volume = round(key x 127)
  at the day fraction. Generator sound elements never run out of life; a removed one fades its voice over its
  life (FUN_10035cd0).
- Scheduler ops 0x0a (actor1), 0x0b (actor2), 0x53 (*(inf)* actor2; retail: the action target nearest the
  player): a snapshot of the actor position, far f32 @0x14, near f32 @0x18, flag 0x100 = height as is (corrects
  triggers.md, which had the fields swapped and the sound following the actor). 'shit' / 'skaz' digit variants
  are not done.
- FUN_10035e50 per frame (manager FUN_10035410): listener = the camera eye; far 0 -> 30, near 0 -> 3; height x3
  (mode 0); linear volume between near and far, beyond far stop (or volume 1 with keep); behind the camera (view
  z > 0.1) x 0.65; pan = 64 + 64 clamp(focal / max(|z|, 1) x / 512, -1, 1), focal = 192 / tan(fovy / 2). Voices
  start with a 3-pass grace, update pan / level when they change, persistent ones restart when over (loop by
  re-trigger), others end with their voice; at most 12 distinct sounds start a frame.
- audio: `play_voice(handle, id, level, pan)` (refused at the busy limit), `voice_level`, `voice_pan`,
  `voice_stop(handle, ticks)` (cmd 0 / 5), `alive()` (handles the mixer still plays, refreshed every mix).
- Not done: footsteps (FUN_100d2970 needs the posed foot points 8 / 9 / 0xa / 0xb every frame; our poses are on
  the GPU), the race death voice op 0x78, the one-at-a-time se ids 2239 / 2063.

## Lens flares (CMoLfdElem; render/flare.wgsl, vs_flare / vs_glare) (2026-10-22)

From ~/ffxi/lab/research/effects/classes2.md section 1:
- Kind 0x39 takes a D3a in the flare layout (byte 4 != 0): groups {u8, u8, quads, u8, f32 t, 12 bytes} then the
  quads' vertices; drawn instead of the common draw (FUN_10053920), last (sort key -10000).
- Visibility FUN_1006b350 / FUN_1006b580 / FUN_1006b900, as a compute pass on this frame's scene depth: the
  direction normalize(parent translation - eye) x 65535 through rot(V) and the projection (behind the camera: no
  flare); a box of +-W/32 x +-H/32 pixels around it, clamped to the viewport (0 on screen: nothing) and squeezed into
  cw16 x ch16 of a 16 x 16 grid; samples at even rows and columns pass where the depth is still at the clear value;
  vis = 4 x samples (0..256). Retail renders black / white quads into a scratch target and reads it back the same
  frame; ours samples the depth directly *(equivalent by construction, not compared on screen)*. Our sky and cloud
  domes write no depth, as retail's.
- Sprites FUN_1003ddb0: centre of group = light + (mirror - light) x t (mirror = (W - sx, H - sy)); vertex x
  file x W/16, y file x H/16 (anisotropic); alpha min(255, A x vis >> 8); colour diffuse x texture x 2 (TFACTOR rgb
  not applied), alpha x TFACTOR x 2; additive, no depth test.
- Glare FUN_10014280 / FUN_100125c0: g = max(0, 1 - r x 2/3) of the light's distance from the screen centre (in
  half-screens), f = g x 0.5 x (vis >> 3) / 128, colour = op 0x4b's (0 -> 0x80808080) x f per byte, added to the
  whole view after the effects. Op 0x4b's two floats have no reader.
- Check: `zone-shot 102 --weather fine --hour 9 --eye 0,-5,0 --target 60,-50,10 --effects 30` (lf02).

## D3b morph elements (CMoD3bElem; fx/lib.rs Morph, vs_morph) (2026-10-22)

From classes2.md section 4: chunk 0x25 (byte 2 bit 7 alpha test, low 7 bits signed = K keys; V @4, N @6, index
table @8 (+ byte 3 << 16), T @0xa, corner colours @0xc, corner UVs @0xe (+0x10000 when not past the colours),
texture name @0x10, K blocks of V positions and N packed normals from 0x20; position then normal indices). Weights
+0x194.. from update ops 0x1e..0x22 (life t, or the day fraction with parameter bit 4, repeated (param >> 5) &
0x1ff times), divided by their sum on the GPU (FUN_1003ea60; no weight -> last frame's largest gets 1.0); corners
blended in vs_morph; colours doubled at load; stages as D3a (texture x diffuse x 2, x TFACTOR x 2 / x 4) with the
common fog and z write; alpha test GREATER 0x60. Not done: normals / lighting, op 0x6e (camera-distance
crossfade), the texture warp (elem+0x140..0x148), the distortion-morph variant.
- Moon (2026-10-22): update op 0x45 sets the frame to the phase FUN_10184dc0 (((Vana day of now + 12 h) / 7) % 12)
  and rotation z = pi/2; op 0x4e sets the colour bytes (R, G, B, A) from an 8-entry weekday table at op+8 (day &
  7, FUN_10184d70), op 0x4f multiplies the colour by a 12-entry phase table / 128 (FUN_1003d290, truncated); both
  lerp from the previous entry in the day first minute. Emission ops 0x4e / 0x4f add a point of a Vtx list (u32
  count, vec4 points from +0x10; random / by emission count). zone-shot takes --moon PHASE.

## Footsteps (FUN_100d2970; client/footsteps.rs, render/points.rs) (2026-10-22)

From ~/ffxi/lab/research/effects/footsteps_sound.md section 1.
- Actor bytes from the 0x45 info chunk bytes 0..2 (Character.cib; first file wins, the feet part file overwrites
  1 and 2): leg mode +0x878 (2 = also points 0xb / 0xa, 3 = steps standing too), footstep kind +0x879 (<= 0: none),
  shoe +0x87a. Hume M feet 8136 = (0xff, 1, 1); orc 1300 = (0, 9, 0).
- Gate before reading: kind >= 1, status 0 / 1 / 4 / 0x1c / 0x1f / 5 / 0x55, within 30 of the eye. Points 9, 8
  (0xb, 0xa) of the posed skeleton minus the actor root, in world axes, come from a compute pass on the frame's
  pose buffer (render/points.rs), read back one to three frames late. A step = some point's height going from < 0
  to >= 0 (several in a frame = one step).
- Surface: the floor face under the feet (Ground::surface: the nearest face at or below p.y - 0.5, its type = the
  four bit-15s; 0 when more than 0.4 below, FUN_10162470) and its covered flag (actor+0x17c).
- Sound: name '0', '0'+surface, kind char ('0'+k, 'a'+k-10 from 10), '1'+shoe; looked for in the zone's 'fses'
  ('fser' in rain / squl / bolt when not covered) subtree (FUN_10071510), else DAT 0 'foot' (se 12). Played at the
  actor, pan input 0 (always centred), level 101 walking (ratio < 0.5) or 127. Test port_sandoria_step: fses/0112
  = se 101001 at 95 (run) / 76 (walk) for the 1.5 worked example.
- Motion gate (inferred): retail tests the base motion against the idle 'idl?' (+0x7d8 != +0x7c8) and a few
  flags (scheduler op 0xbc's counter, a landing); here moving = movement ratio > 1e-4, or leg mode 3.
- Live (zone 101, Lnxtest running 4 s): 11 steps "0212" (grass, kind 1, shoe 1), about 2.75 a second.
- Dust (FUN_100d41a0): generator "w" or "0", '0'+surface, "00" (type 5) in the same folder's subtree, unless leg
  mode 3; cloned on (actor, actor), started, one emission, then ended (life 10000, elapsed 10001). The fefs
  generators are attach mode 1 at caster point **0x34 = actor+0x63c, the foot contact** (FUN_1003abe0; 0x35 =
  actor+0x64c the wade point, 0x36..0x3c = skeleton points 0x7e, 0x7f, 0x66..0x6a); not op 0x8e. Zone 232
  w100: batched D3a hi11, 20 particles, R 0.09, speed 0.0005, base (0, -0.0015, 0), gravity (0, 0.00005, 0),
  scale 0.1, additive 0x12262626, life 60 (a faint puff; invisible on pale stone). Retail keeps one clone per
  actor and restarts it *(inf: a new instance per step)*. East Ronfaure (101) has no grass dust. Test walk_dust;
  fx-shot --dust NAME bursts one at the caster.
- Footprints on surfaces 3, 4, 8 (not leg mode 3): 'fmrk' from the actor's part files (vtable +0x1ec), else DAT
  0 syst/effe; scale B = the actor size; source actor set; one element straight from the template (FUN_1004d920,
  no generator update). DAT 0 fmrk: world mode, D3a fmk1 life 180, bias -0.1, rot (-pi/2, 0, pi/2), scale
  (0.15, 0.19), alpha blend 0x44, colour 0x4e141414, alpha keyframe 'fmrk' = (0, 0), (1, 0) (a fade from the
  base, see "Keyframe sampling"), op 0x8f (area-light tint, not done), op 0x8e. Only DAT 0 carries an fmrk in
  this install.
- Emission op 0x8e (0x1004e7af): FUN_100d3750(actor) builds rows side = down x front, down = -vtable +0x210,
  front = side x down (front first = the actor's (0, 0, 1) through its rotation, FUN_10027400), all normalised
  (FUN_10026680; cross FUN_10026720 = a x b); elem+0x60 *= it, pos = actor+0x63c, +0x187 = 0, so FUN_10043f00
  (rebuilds only when +0x187, or without +0x108 when the generator has an attach matrix) keeps it. Here: element
  words GROUND (54..62) and state bit GROUND, multiplied into the local matrix in the update shader; the element
  header is 64 words. *(inf: the sign of +0x210: the floor normal turned to +y, so flat ground gives identity,
  which is what fmrk's flat-lying rotation needs.)* Not modelled: an update op that rewrites rotation or scale
  sets +0x187 and drops the ground frame (fmrk's stream has none).
- Not done: the zone flag +0x1cf alt name (surface 3 -> 2), the model-scope fses retry, the culled-pass skip
  (+0x840 bits 20..21), 'fvib', the i900 wading ripples (point 0x35 not filled), the footstep-effects option
  DAT_10444158 (taken as on).

### Keyframe sampling in the update stream (FUN_100499d0, jump table 0x1004d08c = op - 1) (2026-10-22)
- FUN_10053520 / FUN_10053740 (linear / eased, by S[s+1] & 0xf) sample with the **first key's value (+0x34)
  replaced by the third argument** for the call; FUN_10053550 / FUN_100537f0 do not. FUN_1004d530(R, x) =
  override with R[2] = S[s+2] (0 when R[0] = 0); FUN_1003d040 = override with S[s+2], then fmod(v, 2) pi wrapped.
- Life-keyed with the S[s+2] override: 12-14 (rot, wrapped), 15-17 (scale), 18-1b (colour, byte), 1c/1d (U/V =
  fmod(key, 2)), 1e-22 (morph weights), 24, 30-32 (then + S[s+2] again, into the op-02 velocity slot), 35-37,
  38-3b, 5b/5c.
- Override with a literal 0, plus S[s+2]: 0f-11 (life), 6b-6d (day fraction).
- No override: 54/55 (U/V = fmod(U + key dt, 2)), 56-58 (rot += wrap(fmod(key, 2) pi) dt, wrapped), 5d/5e, 5f;
  day fraction: 3c-3e, 40-42, 43, 49, 4a-4d, 61-63 (as 56), 66-68 (rot = wrap(fmod(key, 2) pi)); 3f uses the day
  fraction repeated (S[s+1] >> 5) & 0x1ff times, the others take the plain day fraction.
- Effect: a keyframe starting at 0 with a base from the emission (e.g. fmrk alpha 0x4e) fades from the base;
  render/effects.wgsl key_t / key_0 / key_n / key_g / key_rep, mirrored in fx/elem.rs update_ref.
- Drive note: --quit logs out only after the --screenshot shot; a script `shot:` step does not count.

## Distortion / heat haze (CMoDist, kind 0x22; render/effects.rs, vs_dist) (2026-10-22)

From classes2.md section 2.1. Emission op 0x32 (vtable +0x34 = FUN_10051dc0) sets +0x194 = op+8 (element word
DIST_OFF); update op 0x24 sets it to key(t) (first value S[s+2]; 0 without a keyframe). Draw (FUN_100418b0): the
cross (0,0,0), (-1,0,0), (0,-1,0), (1,0,0), (0,1,0) through +0xa0 (our draw matrix A); a fan whose view translation
is shifted by (off, off) (x right, y up) samples the scene where the unshifted points project; centre = element
colour with alpha x fade, tips 0x00808080; colour = texture x diffuse x 2, alpha = diffuse x 0x80 x 4; SRCALPHA /
INVSRCALPHA, z test, z write by flag 12, no fog. The effect pass is split at each distortion element and the colour
target copied first (retail copies the diamond's screen box into a 256x256 texture; here the whole target at full
size, the 256 cap being a 2002 limit). DAT 214 (zone 114, Eastern Altepa; classes2.md calls it "zone 214")
`f_ri/weat/heat/dis1`: 21 live elements at 13:00 heat, alpha 0x09..0x1d, off 0.02 (checked with the shift x25: the
background ghosts up and right). Not done: CMoVuDist (batched), DistModel / DistRing / DistMorph (op-1 flag
0x100000 on kinds 0x1f / 0x24 / 0x25).

## Effect area-light tint (emission 0x8f / 0x90, update 0x69) (2026-10-22)

From ~/ffxi/lab/research/effects/area_light.md. The env: 0x8f = FUN_101819a0(caster vtable +0x188) = the area the
caster stands in (actor+0x5e4, from the floor collision object's u32 +0xb0; placements carry it at MZB +0x4c; keys
in `<zone>/<id>/<tag>/HHMM`), else the zone; 0x90 and 0x69 = DAT_106175ac, the zone. Of its current 0x2f payload:
c = (actor sun + actor ambient) / 255 (FUN_1017fee0 + FUN_1017ff00), c2 = (actor moon + ambient) / 255
(FUN_1017fef0); c2 when flags bit 0 is clear (FUN_10181af0) and dot(c2, c2) > dot(c, c). R, G, B *= c, ftol, 0 if
negative, & 0xff (wraps above 255); alpha untouched. Update op 0x69 (every frame): R, G, B = the slot's bytes 2 /
1 / 0 x the zone c (no nearest-light rule; generators.md was wrong). Here: Env::area_light, Lighting.area, Frame
area_light, the update Params `area`. Test port_sandoria_area_light: 232 fine 12:00 (1.6196, 1.6235, 1.5451),
00:00 (0.9255, 0.9333, 1.1412). Area environments (later the same day): ZoneWeather.areas from `<zone>/<id>/<tag>` and `<zone>/<id>/weat/<tag>`,
collision Object.area (+0xb0), Ground::area (the floor face under the actor), per-frame tints by area
(World.area_lights), Actors::area_light, Instance.area_light for streams with 0x8f; zone 232 `ev01` (flags bit 0,
so never c2): 12:00 (0.9843, 0.9529, 0.8588), 00:00 (0.9020, 0.8706, 0.7765). Not done: the event op 0xae sub 7
override (entity+0x208), the 120-frame refresh (looked up when asked), the `subl` environment of zones 0xb6 /
0x84, the weather crossfade of area envs. Byte 0x0c of the payload is red (MODLOG's earlier "B,G,R"
note on the env colours is wrong; weather.rs rgb() reads R, G, B).

## Effect spline paths (Vtx 0x3e, CMoSpline; fx/spline.rs, update op 0x34) (2026-10-22)

From ~/ffxi/lab/research/effects/splines.md. Vtx data: u32 N, from +0x10 N vec4 points (w = 1). FUN_10062c80 builds
a CMoSpline once per Vtx (FUN_10062bd0 / FUN_10062600; closed and uniform branches dead): cubic Hermite, Bessel
tangents, chord-length knots: c_k = max(|Q_k+1 - Q_k|, 0.01), c_-1 = c_0, c_N-1 = c_N-2, phantoms 2Q_0 - Q_1 and
2Q_N-1 - Q_N-2; a = c_k-1 / (c_k-1 + c_k), b = c_k / (c_k + c_k+1); m_k = ((1-a)^2/a)(Q_k - Q_k-1) + a(Q_k+1 - Q_k),
m_k+1 = (1-b)(Q_k+1 - Q_k) + (b^2/(1-b))(Q_k+2 - Q_k+1); segment = first with u <= its accumulated c/C, else the
last at t = 1. Emission 0x54 ([2] KeyFrame 4cc, [3] sampler, [5] Vtx 4cc): S[s] keyframe, S[s+1] sampler, S[s+7]
path, S[s+3..5] = p(0), pos += p(0). Update 0x34: u = t, or key(t) without override and fmod 1 above 1; pos +=
p(u) - S[s+3..5]; S[s+3..5] = p(u). Here the segments go to the GPU (update binding 8, `paths`, header + 4 vec4 a
segment; the slot holds first vec4 + 1). Test worked_paths: DAT 202 'lin2' and DAT 265 'a7' match the spec's
numbers. zone-shot `--start NAME` starts a zone generator: 102 `--start hat2` pigeons fly along 'lin2'.
Corrections: S[0] is the keyframe (spec C.4 said spline); gen+0xa0 = op[1] of 0x54 is the KeyFrame; op 0x15's
corners are its own op[1] (always 0: the unit cube). Not done: who starts f_la/mode/hato (flags 0x40000000, not
auto), scheduler ops 0x26 / 0x27 (CMoPathDriveActorTask), CMoPathObject.

## Effects filter (config 0x92; FUN_100ac4b0 / FUN_100ac560, fx/tasks.rs effects_allowed) (2026-10-22)

From ~/ffxi/lab/research/effects/effect_filter.md. Scheduler op 0x02 for an action task (task+0x90), off the
unattached-persistent path, asks FUN_100ac4b0(caster, target, cfg 0x92) with dolls as their owners; no means no
clone (running ones stay). Bit 0 ("All effects during battle") limits the filter to the player's status 1 or 3
(DAT_1046e0f8[DAT_10473572] +0x16c); table 0x10324fb4 rows: bit b = 1..16 hides when caster class & (1 << ((b-1)/4))
and target class & (0x10 << ((b-1)%4)) (you / monster / party / non-party PC -> the same four, labels group 0xb
message 148 + b); bit 17 "Screen shaking" has no row (the op 0x5f quake reads it via FUN_100ac540, except while an
event holds the player). Classes (FUN_100ac560; target the same << 4): 1 the player (vtable +0x304 = FUN_100a2cd0)
or the player's pet (FUN_101c2990 = DAT_1046e0f8[player+0x1f6], +0xa0 its actor); 2 entity +0x120 bit 13 (u8 @0x20
bit 0 of 0x00D / 0x00E); otherwise 4 when in your party's roster (FUN_100e5dd0 / FUN_100e5ec0, same party number) or
a dynamic-range entity (ActIndex >= 0x700) whose master (0x067 sub 3 @0x0C, +0x2a0) has +0x1cc & 4; 8 when not in
the roster. Also read by the Geomancer Indi aura (FUN_1008dfb0: the entity as both caster and target, 0x00D u8
@0x42). Here: Actors::fx_class, Effects::filter, 0x037 u32 @0x34 >> 3 = our pet's ActIndex (World.my_pet). Not
done *(party members count as others: the roster, s2c 0x0DD, and the 0x067 masters are not tracked)*: the quake
(op 0x5f not implemented), the Indi aura. Test filter_rows.

## Point lights on scenery (CMoPointLightProgElem -> the light table -> zone.wgsl) (2026-10-22)

From classes2.md section 3 and ~/ffxi/lab/research/effects/point_lights.md.
- MZB (FUN_10172530): version byte 3 > 0x11 and u32 @0x18: a light list of 256 records x 0x4c, key at +0; a
  placement record (100 bytes, version >= 0x12) has 4 refs at +0x54..+0x60, 1-based into that list (runtime
  +0xe4..+0xf0). Here Placement.lights holds the keys; ZoneGeometry gives each key a slot of a 256-entry table
  (FUN_101737d0 / FUN_10171830: one entry per key) and each instance its 4 slots + 1 (instance stride 80, attribute
  8). Keys starting with 'c' are skipped (FUN_10175ac0: they light actors and moving parts only).
- Kind 0x47 elements (FUN_10055340): key = the generator's 4cc (a parent element's +0x194 when run by an element:
  not done); FUN_10171720 finds the table entry (none: the element does nothing). Emission op 0x58: +0x19c range
  base, +0x1a0 intensity base, +0x1a8 = op+0xc + 1, +0x1a4 = op+0x10 + 1; binds 0x7c / 0x7d take +0x1a0 / +0x19c as
  base. Update 0x5b / 0x5c (life key, first value S[s+2]) -> +0x1a0 / +0x19c, 0x5d / 0x5e (plain) -> +0x1a4 /
  +0x1a8, 0x49 (day) -> +0x1a0. Each drawn frame (fade >= 0.01), FUN_10055450 / FUN_10171640: position +0x54 raw,
  range +0x1a8 x +0x19c, I = max(0, +0x1a0) x +0x1a4 x fade, colour rgb / 128 (black when I <= 0), Att2 = 1/I; the
  dtor makes it black. Here the effects update pass writes ZoneRenderer.lights (binding 9: two vec4s a light,
  position + range, colour x I); a dropped element sends Write::LightOff.
- Scenery (fixed function, D3D8 POINT): per vertex sum of C x I x max(N.L, 0) / d^2, none past the range, added
  inside lit = clamp01(vc x (ambient + sun N.L + points)). Lights are a frame late (scenery draws before the
  effects), as in retail. zone-shot renders its frame twice for that. Zone 232 at 00:00: the lamps light the wall
  and floor warm around pl01..; at 12:00 (pttm off) nothing.
- The device asks for 16 storage buffers per stage (the effects update pass binds 9; wgpu's default is 8).
- Actors (FUN_100c94a0, later the same day): collision object +0xac..+0xaf (version >= 0x1a) are 1-based MZB list
  refs (Object.lights, as keys; keys with bytes 1..3 'lgb' skipped by FUN_1017aba0); the table holds every MZB list
  key (ZoneScene.light_keys). Each frame the floor under an actor gives its four slots (Ground::lights, packed in
  pose header word 4.w); vs_skinned picks the strongest w = 1 / max(d^2 Att2, 1e-4) within range at the actor's
  position (an unlit lamp competes too) and adds C x w x max(N.L, 0) as a directional light from the lamp inside
  the actor lighting. The light buffer's second vec4 is now (rgb / 128 or 0 when unlit, I). Not checked on screen
  (no lamp near the test character); retail copies the lights at the ground probe (every 121 updates), here every
  frame; the per-material light factor f (FUN_100216a0) is not applied.
- Not done: moving parts (FUN_1017ce60, no 'c' skip), the bump-map vertex-shader path (config 0017),
  the slot leak for refs to 'c' / unknown keys, the windowed-mode gamma boost (FUN_1000b290), the two-area-light
  scenery mode, unnormalised normals of scaled placements.

## Effect fixes from the San d'Oria gate lamps; per-pixel point lights and soft alpha (ours) (2026-10-22)

Zone 101's gate tunnel at night (f_ro/mode/ligh/g_li): lights glr1 / glr2 / gll1 / gll2 (kind 0x47, 8 / 20, op 0x49),
glow meshes gl01..gl09 (MMB 'ltla', texture 'light   lanp', 70-tick lives overlapping, update 0x0e / 0x3f (key
'kogl') / 0x2e (30, 0)), glow sprites lk01.. / glk1.. (texture 'light', 256 x 256 DXT3 with alpha levels 0 / 34 /
68 / 102 dithered), and gin1: MMB 'san_' (2214 vertices, no texture, argb 30d08030, flags 0x08001000), the inner
tunnel's lamplight baked into vertex colours over the whole tunnel.
- A model's effect files asked for before the first zone set the DAT index were marked loading for good, so every
  scheduler lookup on the player retried forever (no swing, no hit reactions on the player; crits from DAT 0 only).
  lib_for now marks a file only when it can start loading it.
- Ops 0x2e / 0x48 (FUN_100499d0 case 0x1004a0a2; 0x1031e5b8 = 0.0, 0x1031e5fc = 1.0): d == 0 sets the fade 0 (no
  expire). 0x2e: d < rec[1] keeps it, rec[1] <= d <= rec[2] fades, else 0 and expire on rec[3] bit 0, so near is
  tested first (30 / 0 = shown inside 30). 0x48 ('H'): below rec[1] 0 and expire on rec[5] bit 0, fade in to rec[2],
  kept to rec[3], fade out to rec[4], else 0 and expire.
- D3DRS_ZBIAS 15 for the effect draw: -15 x 5e-6 in NDC depth in to_clip, as Wine's d3d8 maps it *(inferred: D3D8
  left the scale to the driver)*. Without it wall decals z-fought.
- Not retail: scenery point lights per pixel (zone.wgsl point_lights from the fragment shader, the placement's slots
  passed flat): the D3D8 term with the distance floored at 1 and a (1 - (d/R)^4)^2 window instead of the hard cut.
  Per-vertex lighting drew triangles of light on the tunnel's coarse walls.
- Not retail: effect textures dithered between at most 8 alpha levels (DXT1 on / off, DXT3 4-bit; dither = at least
  15 % of covered texels a local alpha extreme on both axes: retail 30261006_0 light is 64 x 64 DXT3 with 7 levels,
  20 %; hit sparks under 10 % stay sharp) get colour spread two texels
  into the clear ones and the alpha blurred (3 x 3 box three times) at upload (render/effects.rs soften_alpha); the
  dither between the levels showed as grain at today's resolutions.
- Left as retail: gin1's baked shell still draws triangles of light (Gouraud over large triangles) and the ltla
  glows cut straight into pillars. zone-shot now keeps one effects renderer for both frames (meshes upload once);
  FFXI_FX_OPS=NAME, FFXI_FX_TEX=SPRITE and FFXI_FX_MESH=1 (with FFXI_FX_LIST) dump streams, a sprite texture and
  the effect meshes.
- Parked (the user chose retail's look for now; not done, may come back): B, real lamp lights in place of baked
  shells. Lamps = zone glow meshes textured "light..." (the ltla glows), merged by position, skipping any within
  ~4 yalms of a kind-0x47 data light (the door lamps). Each gets a light 0.7 yalm out from its wall (Ground::raycast
  in 8 horizontal directions finds the wall) whose colour, range and live intensity come from the nearest data
  light's table entry on the GPU, so dusk / dawn and fades follow the data. Untextured additive MMB shells in the
  same generator folder as such glows (gin1 'san_') are not drawn. The extra lights (nearest 64 to the camera) go in
  a second storage buffer on the zone globals (binding 2: count, then position + template slot), summed per pixel
  in point_lights and offered to actor_lamp. C, soft particles: depth_view gets COPY_SRC; the effects draw copies
  the scene depth before its first pass and binds it as group 2 (texture_depth_2d, a 1 x 1 cleared dummy without
  one); fs fades the alpha of flag-27 elements (sort key + 400: glows and light volumes) by
  sat((scene distance - fragment distance) / 1 yalm), distances from NDC z with the draw's near / far. Only flag 27,
  so footprints and other flat decals keep their look.

## Event schedulers run in the effects engine; the level-5 crystal scene (2026-10-22)

- Event opcodes 0x45 / 0x62 / 0x9f / 0xbb / 0xc5 / 0xcd / 0xd0 / 0xd5 start the same CMoSchedularTask as the
  effects (the stop family 0x52 ... ends it). The event screen runner kept only ops 0x04 camera, 0x0f fade and 0x10
  crossfade; now every event scheduler also goes to fx (Effects::on_event_scheduler / stop_event_scheduler, a file +
  4cc task on the event's actor pair), which runs its generators, sounds and motions. Inferred: the camera ops stay
  with the runner until the two are merged into one task list.
- Zone 230 event 0x7553 ("The crystal resonates deep within your soul!", level 5): zone_env #541 (op 0x34,
  FUN_10184380, area environment: not handled yet), npc 0x25f moved to (-400.0, -19.1, 372.4), motions wap* from
  motion file #3860, sched_play_c7ef #61 'str0' = DAT 51244: generators g101 (screen quad) and gds0 (screen
  distortion rings); cameras s063..s070, s159 from file #760 (DAT 71107) looking at (-400, -23, 372).
- The set itself is npc 0x25f's model 2959 = DAT 0xca53 + 1459 = 53254 (FUN_100c3220's mapping matches
  dat::npc::model_file), absent from every VTABLE of this install: models 2957 and up (and 2442, 2443, 2477) are
  missing, so the install is older than the server's data. Needs the game files updated.
- zone-shot --sched FILE:TAG starts an event scheduler in the effects; FFXI_NEAR=X,Z prints the zone's placement
  bounds and the placements near a point.

## Game file updates (PlayOnline patch protocol; src/patch.rs) (2026-10-22)

Sources: the retail installer bundles PlayOnline (FFXISetup.exe runs PlayOnline/PlayOnlineViewer.msi: pol.exe,
polcore.dll, app.dll 1.18.0.0, extracted to ~/ffxi/lab/pol/bin). Its code sections are packed (Ghidra finds 6 / 45
functions), so the protocol facts come from Kuluu's reverse engineering of the self-updated viewer 1.18.15e
(github.com/jondwillis/kuluu-ffxi, ffxi-install / ffxi-pol; GPL-3.0: read as notes, not copied; their sources cite
polcore FUN_1003968a.. frames, FUN_10042805 / FUN_10042d91 coders, app FUN_101b68d4 host, FUN_1029c3a8 port). Not yet
checked against our own decompile *(inferred until then: unpack polcore.dll 1.18.0.0 and confirm)*.
- Server: pc%03d.pol.com, port 53000 + title; FFXI is title 1: pc001.pol.com:53001, plain TCP, no login. Tags in
  every client frame: platform "W20\0", title "0001".
- Frame: u32 total length, MD5(frame[8..])[0..4], "POLP", u32 kind, body. Kinds: 1 hello (body: tags), 2 manifest
  (status byte 2 then .slc), 3 file request (offset, len <= 0x10000, tags, u32 path length + 1, path, NUL), 4 chunk
  (offset, len, extra, extra bytes, data), 5 reject, 6 close, 7 version (tags + a 0x40-byte version block; all zero
  asks for the current version, labelled "empty"), 8 version reply (release time, flags, label + client address in
  0x40 bytes, u32 length, version string).
- Manifest (the Viewer's patch.cfg): `file <path> {`, one line per version `stamp len sum md5 <stamp>/Direct/<path>
  .slc len [<stamp>/Indirect/<path>.olc len]`, `}`, until `end`; paths may hold spaces. Signature: length, bytes
  summed as signed 8-bit (wrapping i32), MD5 word 0 little-endian as i32.
- .slc: u32 bit count, LSB-first bits; flag 1 = copy (distance 16, length 8) from the output, 0 = literal byte. .olc
  against the previous version: 0 literal, 10 output copy, 11 copy from base at output position + i16 offset. A
  Direct download starts with a flag byte: 1 stored, 3 zlib, else .slc.
- Per file: newest matching version current = skip; older match with a delta at every later step and the deltas
  smaller than the whole file = apply them; else the whole file. Every step is checked against its signature.
- Ours (`vana-client update [--dry-run] [--code] [--connections N]`): data files only by default, since this client
  reads FFXiMain.dll at decompiled addresses; writes vana-update.cfg (the manifest applied), not the Viewer's
  patch.cfg / patch.ver.

## Retail FFXiMain 30261006_0 as the baseline; tables found by signature (dat/tables.rs) (2026-10-22)

- The client reads its FFXiMain tables by content now: each has a short hint, unique in both the 2019 base and
  30261006_0, confirmed by an MD5 over the table (or its stable start). Found once per build (keyed by length and
  sampled bytes). Hints and MD5s from lab/current/signatures.py. Every address found matches the hand mapping
  (lab/current/remap.py) in both builds.
- Fixed distances, true in both builds: model key 1 = key 2 - 0x328 (a second copy of key 1 exists further on);
  the seven mode tables are the lowest L at [L+0x400, L, L+0x800, L+0xc00, L+0x1000, L+0x1400, L+0x1800]; the
  eleven filter tables are filter kinds + 0x400 + i * 200 (30261006_0 lists them at file offset 0x343a4);
  count formats at COUNT_FMT - 0x24 k.
- Language slots (FUN_10250e50 -> retail FUN_1025b580, table global DAT_10623ed4 -> DAT_10669464): SE inserted
  slots. 2019 slot s maps to s for s < 0x20, +1 below 0x29, +2 below 0x55, +3 below 0x97, +4 after. From both
  tables' file ids; checked on retail callers (0x3b -> 0x3d, 0x57 -> 0x5a, 0x5a -> 0x5d, 0x5c -> 0x5f).
- Title loby2win draw is retail FUN_10240030 (2019 FUN_10235810). It still formats "- %d" / ",  %d" with the
  year (0x1038869c / 0x10388694) but no longer draws the digit sprites; the art reads "(c) SQUARE ENIX" with no
  year. Button sprites (0x103885a4 / ac) and logos (0x103885b8) are unchanged.

## Tables copied from FFXiMain, ported to 30261006_0 (2026-10-22)

Functions matched across the two decompiles (17,194 pairs), data addresses taken from matched instructions,
tables compared over their full length with pointers resolved (lab/current/tables_port/). The current decompile
needs FixVt.java after analysis (3,478 vtable-only methods, 17,877 functions in all).
- Group 0xb (client messages) gained labels: 2019 id -> current: below 6 same, 6..=39 +2, 40..=123 +4, 124.. +5.
  Groups 1, 7, 8, 9, 0xa, 0xd, 0xe, 0x1b unshifted; 0xc +1 from 0x7a.
- Log window tabs (FUN_10138800, list built by FUN_10138700 in .data 0x1036f940): 11 sprite pairs at 0x10333b4c;
  pairs 7 / 8 are filters 9 / 10 (Assist J / E, inferred from /assistj 9, /assiste 10 in the channel table
  0x10355588), drawn 7 then 8 only in Japanese.
- cfilter 0x10379a28, 55 rows: no mask bit 51 (was 49), no hold bit 11 (was 9), system level r - 51. New rows mask
  bits 49 / 50, hold bits 9 / 10. conf11s 0x10379ef8..: window-mask bits 12 / 13. Font Colors 0x10379b50..: 13
  rows in category 0, new config ids 0xcb / 0xcc. k1assign 0x1037a128 (labels only), k2assign 0x1037a218: 29 rows,
  row 28 = key function 0xb4, config 0x86, default DIK 0x10. fxfilter labels from 153; yes/no text 0x8f.
- Misc. 4 (FUN_1019bdd0): children 4 / 5 / 6 set 0xce = 1 / 2 / 0, 1 and 2 kept only while FUN_101bced0() > 0
  (inferred: a gamepad count). configwi row 13 and lobycwin child 13 start the external gamepad config tool
  (FUN_101bcb60): not done.
- Title music: SEQUENCE 0x10351718 adds (34, 7740, 108); FUN_102455a0 adds (bit 0, entry 6); TITLE_MODE 0x1033cc48
  8 entries. FUN_100377a0: newest = pick 5, random = timeGetTime % 6. lobycwin child 12 = track 7.
- Chat: /assistj /aj /assiste /ae (list 0x1037ade8), 0x0B5 kinds 0x22 / 0x23 (FUN_10078be0 / FUN_10078d80; the name
  to handler link inferred from the handler table 0x10356e34 order).
- PC model slot ranges (pointers 0x1035f378, FUN_100d2f60): 48-byte records, 6 ranges; armour gained 64 models, main
  / sub a 300-model range. Item ranges (0x10372150): new 0x7800..0x7c00, General reader, files 55555 / 55675.
- Key functions: 0xbb (callbacks 0x1036db18); 0xb7..0xba test a second pad word (inferred: gamepad).
- Config ids (FUN_10195460): 202; new 0xcb..0xce; defaults changed for 0x0f, 0x13, 0x33 (0x1800), 0x47, 0x86, 0x8b,
  0x8c, 0x8f, 0xb7, 0xc0, 0xc2, 0xc3; 0xaf max 8; 0xab default computed from the desktop size (inferred; kept 0).
- Unchanged: motion, effect, audio (atrac3, mixer), dancer, inventory, map, HUD, weather, title buttons and logos,
  message group -> slot table (0x10371da8), name vtables, language tables.
- The updater now patches code files too; the start-up check also compares code files against the stamped
  manifest, so installs patched data-only get the current FFXiMain.

## Event ops 0x34 / 0x35: another area during an event (2026-10-22)

- 0x34 (FUN_100b3df0) and 0x35 (FUN_100b3ed0) take three frames (counter 0, 300, then work) and call FUN_10184380
  with the value: the zone loader, so the event's world becomes that area (file zone::file_id(area)) with its
  scene, collision, effects and environment. 0x34 has a branch to FUN_10184600 (environment reset) on
  DAT_10476f10, which nothing seen sets; 0x35 lacks it.
- Zone 230 event 0x7553 (RoV 1-1, "The crystal resonates..."): 0x34 #541 (DAT 641, a 6 MB stone chamber), the
  scene, then 0x35 #230 brings Southern San d'Oria back; op 0x77 / 0x78 set and restore the time and weather.
- Ours: the op sets World::area.want; the App loads the area like a zone (or, for the zone's own id, swaps back the
  kept renderer, collision, weather keys and effect library: Effects::switch_zone) and the op's wait ends then.
  A zone load drops the switch. The level-5 scene checked live by the user.

## Event sound volume and the scheduler stream (2026-10-22)

- Event op 0x69 / 0x6a (FUN_100b5c30 / FUN_10034be0; XiEvents 0x0069 agrees): mask 1 Effect (world sounds,
  DAT_1034656c), 2 System (interface sounds, DAT_10346568), 4 Zone (the zone's looping ambient voice
  DAT_10450fd8, started by FUN_10035350 from a sound object, level DAT_10346570), 8 Master (all of them and the
  stream DAT_10450fe8, FUN_100352d0), 0x10 a flag for special chat-message sounds (DAT_10346578). The music is not
  among them: openings mute the world (San d'Oria 0x1f7: 0x69 1, #5 at 0x37f9, back at 0x3daa) and keep the music.
  Ours had mapped 4 and 8 to the music, so the opening flyover had no music.
- Scheduler op 0x60 (FUN_10057000 case 0x60): the named SeSep (type 0x3d) played as the stream through
  FUN_100360e0 (centred, not attenuated); only mask 8 changes its level. Used by e.g. hrp1 / hrp2, fan2 / fan5, s021
  in DAT 30834; not by the San d'Oria opening's flyover schedulers (ops 01 / 04 only).
- Interface sounds (menus, targeting, prompts) follow mask 2 / 8, world effects mask 1 / 8.
- Creator: on the nation step the dancer is torn down (FUN_10246f10(.., 0)), so its foley stops.

## DAT overlays, merged per chunk (ours; 2026-10-22)

- Chunk kind names (FFXiMain table 0x103542a8): 0x05 Generater, 0x1c Mzb, 0x20 D3s (texture), 0x2e Mmb, 0x2f Weather,
  0x31 Msb (menu sprite sheet), 0x36 Rid, 0x3d Sep, 0x4a Sph, 0x5d Bp (bump map).
- What the packs change against the 30261006_0 stock files (chunks keyed by folder path, 4cc, kind, n-th): AshenbubsHD
  Prime (Nov 2021, 33,671 DATs) D3s only plus 3 Sep that are stale; Amelila upscale 01 / 02 D3s plus a few stale
  Mmb / Weather / Sep / Mzb / Rid; Amelila megapack 2019 D3s and 278 Bp, and many chunks stock no longer has (the
  files were restructured since); Remapster and xiPrime menus D3s + Msb. So only D3s / Msb / Bp are taken from mods.
- 239 AshenbubsHD files and 109 Remapster files are at paths no current VTABLE / FTABLE entry points to (dropped or
  moved since 2021; Remapster's extra maps), so nothing loads them.

## Event ops 0x73 / 0xad / 0xc4 / 0x7d: actors run action files (2026-10-22)

- FUN_100b4590(off, kind) (0x73: off 0, kind 1; 0xc4 via FUN_100b4550: off 1, kind 0x10 + sub) and FUN_100bb340
  (0xad: u8@1 high nibble must be 0, low nibble = kind): val id, actors A and B (both with models, +0x120 bit 9),
  then A model vtable slot (B model, id, "main"). Lengths 0xb (0x73), 0xc (0xad, 0xc4).
- Slots (character vtables 0x1032db40 / 0x1032e890; base 0x1032cfe0 has stubs): each adds a base to the id and
  calls FUN_100a98d0 (loads that file and runs "main" on A -> B): kind 0 +0x2d0 item (id < 100: 0x1330, else 0xdca5),
  1 +0x2c4 magic 0xaf0 (FUN_100a9700 also prints text through FUN_10127c00; events pass none), 2 +0x2cc 0x113c,
  3 +0x2d4 0x1394, 4 +0x2d8 0xc7ef, 5 +0x2dc 0xdd6d, 6 +0x2e0 0x1071b, 7 +0x2e4 0x11323, 8 +0x2e8 0x11423,
  9 +0x2ec 0x19031. The same bases as the 0x028 action categories (fx effect_base).
- 0x10 +0x2f0 / 0x12 +0x2f4 (FUN_100a9a30 / FUN_100a9a90): vtable +0x3e4 (FUN_10085b50) builds an action of one
  target with a zeroed record (DAT_1032cfac, 11 zero dwords), category 3 (weapon skill) / 0xb (mob skill),
  animation = id, played through +0x2c0. 0x11 +0x2f8 is FUN_100d67b0 only in vtable 0x1032e890 (others stub):
  unless +0x2a4, a category 1 action and a melee swing (FUN_100d6850).
- 0x7d (FUN_100b8b20): the player runs file 0x13f8 + val "main" on itself (FUN_10062e20). Length 3.

## Sub-areas: building interiors (2026-10-22)

From the current decompile (spec in the session scratchpad, subarea_spec). Zone renderer DAT_10631f24.
- Triggers: zone RID records 'm' / 'M'. u32 @0x0C is a rect class (we had read it as rot.x); only class 0 records are
  hit-tested (builder FUN_10173c10); @0x2C param = the sub-area id (0 = unload). Hit test FUN_101760f0: the point in
  T(-pos) RotY(-yaw) S(1 / |size|) inside [-0.5, 0.5]^3 *(inf: the bounds, from the corner table)*; first match in
  list order. Region::contains now takes |size| (negative sizes failed). Zone 230: 18 ids 326..344 except 334;
  m6eb (class 0, param 335) just outside door _6e1.
- FUN_10014390 each frame: the player's position -> FUN_10181540 -> FUN_10178a20(zr, 1, id): ignored while loading
  (+0x44) or for the loaded id (+0x17c, -1 none); 0 unloads (FUN_10178ac0) and sends 0x0F2(1, 0); another id loads
  only if some m record (any class) names it (FUN_101779b0). No hysteresis: an interior stays until another trigger,
  a param-0 trigger, s2c 0x05B / 0x065 mode 7 about us (FUN_1009ee60) or the zone unloading.
- FUN_101787c0 (async): unload the old, read file_id(id) (zone mapping), run its chunks into zone slot 1, then
  0x0F2(1, id), +0x17c = id, door state, FUN_1017a630, areas, +0x50 = area 'ev10'. FUN_10178900(id, from_event) is the
  synchronous form: event op 0x75 sub 0 (no packet) and zone-in with 0x00A SubMapNumber (+0x60, when nonzero and not
  MapNumber +0x42; sends 0x0F2(1, id)). Op 0x75 sub 2 sends 0x0F2(2, val); FUN_10178780 is the busy check.
- c2s 0x0F2 (FUN_100fc680): u16 State @4, u16 SubMapNumber @6. LSB stores it as loc.boundary and returns it in 0x00A.
- Placements: MZB record +0x50 = the sub-area it stands in for (house_05_in -> 335, a bare stand-in room). Render
  type (FUN_1017a630): 1 = hidden when link == +0x17c. Unloading does not run FUN_1017a630, so stand-ins stay hidden
  until the next load (copied).
- Collision: the sub-area MZB's collision joins the zone grid (FUN_10167be0, cell entries flagged 0x1000, removed by
  FUN_101679a0); objects with +0xbc == +0x17c are skipped by the walks (FUN_10182700). Object light refs remap into the
  zone table (FUN_1017a5e0).
- Interior DATs (e.g. 435) hold MZB + collision, MMBs, textures, schedulers, 'effe' generators and an 'ev10'
  environment; no RID; placements already in the zone's world space.
- Ours: client/subarea.rs (state, background load), Collision::merged, ZoneRenderer::set_sub / clear_sub (own
  buffers, texture slots after the zone's, stand-in instances zeroed on the GPU), Effects::load_sub_area
  (Owner::SubArea; its auto-start generators start as a zone's *(inf)*), area lights take the sub-area's
  environments. zone-shot --sub. Checked live: Lnxcsfix (GM 5 for !pos) in m6eb loads 335, 24 generators start, the
  floor and room inside are the interior's.
- Not done: scenery lighting per area. FUN_10183da0 lights each instance by its area object (inst+0xe0, from MZB
  +0x4c: none = the zone's lights, else that area's lights, ambient and fog) and pieces with word +0x22 by 'ev10'
  while +0x50 is set; ours lights all scenery with the zone's. Door collision during a load (FUN_10176dd0) not copied.

## Scenery and actors lit by their area's environment (2026-10-22)

- FUN_10183da0 (scenery draw), per instance: when the model's word piece+0x26 is 0 or no 'ev10' area is set
  (zr+0x50), the instance's area object inst+0xe0 decides: none = the zone's lights (+0x39180), ambient (state 0x8b),
  fog colour / start / end (0x22 / 0x24 / 0x25); else that area's: FUN_10188ca0 lights, FUN_10188e00 ambient,
  FUN_10189120 fog colour and range (all with group 0, the bg keys). Models with the word set while 'ev10' is set
  use 'ev10''s lights, ambient and fog colour but the zone's fog range.
- inst+0xe0 = FUN_101889c0(inst+0xdc), the area object named by the placement's 4cc at MZB record +0x4c
  (FUN_10186cb0); area objects come from the zone's (and a sub-area's) area folders (FUN_10186c60).
- The word: MMB model header u32 +0x10 (FUN_10170150 region, asm 101701d4..10170216): low 28 bits the vertex count,
  bit 31 -> +0x20 (D3DRS_ALPHABLENDENABLE 0x1b in the draw), bit 30 -> +0x22, bit 29 -> +0x24 (compared with the
  texture to pick the cull mode 0x16: set = none), bit 28 -> +0x26 (the 'ev10' flag). Ours keeps the high half as
  Model.blending, so the flag is blending & 0x1000.
- Actors: the actor update (100a5xxx) builds its light with FUN_1018b9a0(lights, mode, vtable+0x188 area) and the
  draw (FUN_100cb...) its ambient / fog with FUN_1018b9e0 / FUN_1018ba20 and the same area: the area the actor
  stands in (its floor's collision object +0xb0). FUN_1018b9xx swap `this` to FUN_101889c0(area) when it is nonzero.
- Areas in use (zone-info prints them): 230 ev01 x182, ev02 x17 (stand-ins); 231 ev01, ev02, ev03 x199, ent4; 232
  ev01, ev02; interiors ev10.
- Ours: Placement.area, Instance.area (slot), GpuVertex.flags bit 0, an area table (render AREAS = 64, AreaGpu: bg
  sun / colour / ambient / fog, actor sun / colour / ambient) written per frame from ZoneWeather.areas (the
  sub-area's first *(inf: lookup order)*), Uniforms.area_info.x = the 'ev10' slot while a sub-area is shown;
  ActorGpu.area from Ground::area, carried in pose header vec4 5 (POSE_HEAD 6). An area with no environment is lit as
  the zone *(inf: FUN_101889c0 finding nothing)*. zone-shot fills the table at --hour; FFXI_NO_AREAS=1 skips it.
- Seen: the 335 interior is dim and warm instead of sunlit; ev01 archways in 230 darker than the open street.
- Open: bit 31 is alpha *blending* in retail; ours alpha-tests those models (the reference viewer's rule).

## Event ops that store values: 0x9c, 0x64, 0x65, 0xaa, 0x72, 0xb7 (2026-10-22)

Handlers from the current build's dispatcher switch FUN_100bc280 (case -> FUN_ map in the session scratchpad,
dispatch.c): 0x9c FUN_100ba100, 0x64 FUN_100b75c0, 0x65 FUN_100b7860, 0xaa FUN_100babe0, 0x72 FUN_100b84b0, 0xb7
FUN_100afee0 (also 0x31 FUN_100b5630, 0x4c FUN_100b6950, 0x4d FUN_100b6a00, 0x4f FUN_100b69b0, 0x8e FUN_100b6980, 0x8f
FUN_100b6a30, 0xb4 FUN_100b0a00, 0x82 FUN_100b9040, 0x69 FUN_100b7a60, 0x6a FUN_100b7af0, 0x20 FUN_100b34c0, 0x30
FUN_100b5620, 0x75 FUN_100b8660).
- 0x9c: set@1 = 2 for language 1..3 (2 / 3 count as English since the European versions went), else 1. Len 3.
- 0x64: set@1 = ftol(sqrt((v7 - v3)^2 + (v9 - v5)^2)) with 32-bit integer squares (asm 100b75c0). Len 0xb.
- 0x65: set@1 = ftol(3D distance actor@3 -> actor@7 x 1000.0 (double 0x1032a110)); positions +4 / +8 / +0xc, or the
  event body's +0x140.. (actor+0xd4 -> +0x260) when +0x120 bit 7; 0 when either actor is missing. Len 0xb.
- 0xaa: val@1 = Vana'diel time in earth seconds (hour 0x90, day 0xd80, week 0x6c00, month 0x19500, year 0x12fc00):
  @3 year + 0x376 (886), @5 month + 1, @7 day + 1, @9 weekday, @b hour, @d minute (t % 0x90 * 60 / 0x90), @f moon step
  (t / 0x5e80 % 12). Len 0x11.
- 0x72: sub 0 starts loading file 0x1b79 (val@2 < 100) or 0x1b7d; a failed load jumps 10 (over the sub 1); len 4. Sub 1
  (yields until loaded, DAT_104886f5): row = file 0x1b78[zone] (or 0x1b7c[zone - 100], both 256 bytes, loaded at
  start, FUN_100954a5); bytes at row * 0x1950 + (val@4 % 0x870) * 3 -> DAT_10488800..: zone work 0x1002..0x1004 (zone
  work base DAT_104887f8, FUN_100af4c0). Both set +0x25a. Files: 0x1b79 38 rows, 0x1b7d 52 rows. Values are weather
  ids (230 day 0: 1, 2, 255): the forecast NPCs (event 0x278 in 230).
- 0xb7: sub 0 writes val@8 into actor@2's VM (+0xd4, needs the actor) at the operand @6 *of that VM's current op*
  (FUN_100af310 with ECX = the target VM, asm 100aff55); 1 str@2 = actor@4's name (+0x7c, or +0x214 when +0x210 != 0;
  "" when missing); 2 set@2 = server id (0); 3 set@2 = ActIndex (FUN_100afb00's second result; 0xffff); 4
  DAT_10480a94[val@2] (0..8) = ActIndex, a table nothing in the build reads (cleared to 0xffff at 144153). XiEvents
  words subs 3 / 4 differently; the decompile above is what we follow.
- Ours: Host::language / index / weather_day (client/event.rs weather_day reads the files); test weather_table.

## Event op 0x31: walk to a point in 3D (2026-10-22)

- FUN_100b5630. Sub 0 (len 10): FUN_1008af20 on the actor (stops its '@tl0' / '@tr0' steps, model vtable +0x29c),
  then the thread slot (+0x20 + thread * 0x20) gets x val@2, height val@6, z val@4 (x 0.001: +0x28 / +0x2c / +0x30)
  and a frame limit val@8 (short +0x38). Sub 1 (len 2): d = target - body (+0x140..), body dir (+0x154) =
  atan2(-dz, dx); limit < 1 -> done where it is; |d| <= speed (+0x168) x frame ticks x _DAT_1032b160 -> snap, done;
  else step along d. Then limit -= frame ticks (ftol), +0x25a = 1 (ends the frame) and actor+0x124 |= 0x20000. Both
  subs end with the ground snap (FUN_10181290 with range 50 for the player, else FUN_10181300) unless actor +0x128
  bit 14 or +0x120 bits 20 / 21 (ours: Body.no_ground). The limit bounds the time, not the speed.
- Ours: Thread.frames, move_3d, Effect::StopTurn. Zone 231 event 0x26 walks for 61 / 301 / 481 frames.

## Event op 0x82: region test on the event actor (2026-10-22)

- FUN_100b9040: u32@1 (raw, FUN_100afac0) is a RID region 4cc found by FUN_1017ad70; the VM's own actor's model
  position (+0xa0 -> +0x34..+0x40) is tested with FUN_1017ad80 (point twice). Inside: len 7; outside, or no region /
  actor / model: jump to u16@5. Ours always fell through; the headless host keeps that (Host::in_region Some(true)),
  the live host tests Region::contains of the zone RID record whose tag matches. Lower Jeuno (245) uses e6t0 / e6t1.

## Scenery level of detail and the MZB id (2026-10-22)

From the draw-state spec (scratchpad drawstate_spec/SPEC.md). Correction to "Scenery and actors lit by their area's
environment": FUN_10183da0 draws door groups only (its one caller is FUN_1017ade0); the static scenery draw is
FUN_1017c9c0 (from FUN_1017b510 and the MZB tree walk FUN_1017c8b0). FUN_1017c9c0 lights by the instance's area
(inst+0xe0) the same way; the 'ev10' bit-28 path is door groups only, and only door meshes carry bit 28.
- Slots, FUN_1017afe0: a placement name ending in h / m / l (last byte > 0x20, index > 0) looks for siblings by
  exact 16-byte name: high = name+h, else name+m, else itself; medium = name+m, else itself; low = name+l, else name+m,
  else itself (inst+0x18 / +0x14 / +0x10).
- Pick, FUN_1017c9c0: d^2 = |camera - placement translate|^2; d^2 <= r+0x38 ^2 high, <= max(r+0x38, r+0x3c)^2
  medium, else low. No setting scales it. Door groups always take high. Typical (10, 100) in cities, (10..70, 100)
  in La Theine, where every terrain tile and tree has _h / _m / _l meshes.
- MZB record +0x34 is a group id: '_' doors and '@' platforms form groups (FUN_1017a950); placements with any other
  nonzero id are not drawn by the scenery pass (Southern San d'Oria's eml0..emlc with ids ent0..entd).
- Ours: Placement.id / lod / lod_names / drawn, Instance.band (near^2, far^2; far 0 = always); each distinct LOD mesh
  gets the placement's instance with its band and vs_main collapses instances outside it.
- Still to do from the spec: alpha blending for bit-31 models, the 0x60
  alpha cutoff on '_' placements only, back-face culling (mirrored = scale.x * scale.z < 0), draw distance.

## The Mog House (MYROOM) (2026-10-22)

From the Mog House spec (scratchpad moghouse_spec/SPEC.md).
- 0x00A handler LAB_100facf0: MapNumber @0x42 gets + 0x1bc when >= 0x100; LoginState @0x80 1 = MYROOM. Zone load
  FUN_10094190 loads the scene by map number (FUN_1018b3a0 -> FUN_10178df0, file = n + 100 below 600, else
  + 0x144f7): MyroomMapNumber @0xaa in the Mog House, else the adjusted MapNumber; never ZoneNo (so zone 256 Western
  Adoulin is scene 0x2bc, file 83891, not 356, the Jeuno Mog House). NPC names, events and dialogue keep ZoneNo. No
  sub-area in the Mog House. Floor = (u8 @0xa8 >> 1) & 3; MyRoomExitBit @0xae (LSB: the exit quest's mhflag bit,
  Adoulin always 9).
- Rooms: 0x121 / 0x122 / 0x123 nation houses (files 389 / 390 / 391, bare; furniture comes from items), 0x100..0x102 /
  0x120 rent-a-rooms, 0x124 Adoulin, 214 / 745 / 199 / 219 Whitegate and the [S] cities, 0x267..0x26a 2F, 0x2d4 Mog
  Garden map, 0x2d9 Feretory. Each RID holds one record, `_720`, the door; no zone lines.
- Music: slot 6 = 0x7e chosen before everything while in the Mog House (not map 0x2d4). Zone-in camera
  (FUN_10021680 / FUN_10096640): (-4, -2, 0) in the player's frame, (-3, -1.5, 0) in the Mog House.
- The door: a client-only actor (FUN_101d2cc0; id 0x1000000, index 0x900, kind 3 so an XiDoorActor, no name plate)
  "Door: Back to Town" at RID `_720`. LAB_101d6040: with an exit bit the menu "mogdoor" (title group 9 0x45, rows
  child n + 0x14: 0xb8 stay, 0x46 / 0xf1 area entered from, 0x125 change floors (dim unless 2F unlocked: s2c 0x067
  @0x27 bit 0, not parsed), 0xb9 select an area: the list DAT_1037ec30[bit] in the same window, zone names); without
  one the prompt group 0xc 0x2f "Exit your Mog House?" (0x2e in an exit-list city). A choice sends c2s 0x05E RectID
  'zmrq', MyRoomExitBit @0x16 (FUN_10097bc0: on 1F only the house's own bit), MyRoomExitMode @0x17 (0, 1..4, 0x7d /
  0x7e floors, 0x7f Mog Garden). The door's opening swing before it is not played yet (door system).
- Zone 230 group 0xc is now loaded with the lobby message groups.
- Scene changes within one zone number (city <-> Mog House) reload the scene; until it is in, nothing settles,
  falls or fades in on the old ground (World::in_scene).
- Not done: furniture (Mog Safe / Mog Safe 2 items with exdata installed bit; s2c 0x020 exdata), s2c 0x02E and the
  myroom / mogext moogle menus, 2F unlock and owner from s2c 0x067, the door animation.

## Doors (2026-10-22)

From the door spec (scratchpad doors_spec/SPEC.md).
- 0x00E SubKind 2 (u16 @0x30 & 7, handler 0x1009cb37): actor kind 3, door id = u32 @0x34 (every update, LSB
  door_id), animation u8 @0x1F -> +0x16c. First model init FUN_1008f740 makes an XiDoorActor (vtable 0x1032fd58,
  plate draw empty: no name plate) and FUN_100acba0(id): RID record by id, collision closed, door group by id (zone
  slot then sub-area slot), schedulers from the folder named id: open / clos (with a group), else evst / eved.
- State in force (+0x170) = +0x16c, or the event's +0x174 (ops 0x4c 8, 0x4d 9, 0x4f val + 0x12, 0x8e 0x2d, 0x8f
  0x2e) while the actor is bound and an event runs. FUN_1008c6c0 on a change: 8 / 0x1a / 0x1d open + collision off;
  0x2d open, collision unchanged; 9 / 0x1b / 0x1e / 0x2e close + collision on (at the start of the swing). First sight:
  into / intc (ints / inte) when present, else the animated open for an open state (230 has none, so a door spawning
  open swings open).
- Door groups (FUN_1017a950): placements with MZB u32 @0x34 = an id whose low byte is '_' or '@', record order, at
  most 4 parts; the name plays no part. Drawn with the high LOD slot.
- Scheduler ops 0x0d (rotate, FUN_100439c0) / 0x0c (translate, FUN_10043bb0), actor1 a door: target vec @8, part
  u32 @0x14, duration i16 @6 x speed; linear from the part's current value, exact at the end; angles wrapped to
  [-3.1415, 3.1415]. Part world = base * T(t) * Rx(a.x) * Ry(a.y) * Rz(a.z) (column vectors), pivot at the model's
  origin. 230 doors: both leaves +-1.3962 rad about Y over 70 ticks, sounds 9021 / 9022 (op 0x0b at the door).
- Collision: each '_' RID record becomes a 12-triangle box (FUN_10171930: S(|size|) RotY(yaw) T(pos), corners,
  normals and triangles in the spec), object flags bit 1, linked into the cells under it (FUN_10167ad0; one cell, or
  every 4-unit cell when size.x or size.z > 7). FUN_1017ac40 sets list bit 0x2000 (skipped by every walk) while open.
- Ours: Collision::add_door_boxes (Object.door), Ground::set_door / live (open doors' boxes skipped in heights,
  rays and the sweep), ZoneGeometry::door_parts and Instance.band.z (part slot), a GPU part table (render PARTS,
  PartGpu: rotation and translation from / to / (start tick, ticks)) composed in vs_main against u.area_info.y (the
  tick clock), ZoneRenderer::move_part (the CPU keeps a copy to know a part's current value when a new op starts),
  fx DoorMove from ops 0x0c / 0x0d, Effects::door_scheduler (folder by id, zone then sub-area library), client/doors.rs
  (state table, first sight, changes, event overrides), door entities in the fx placement table (their sounds), and
  talk() treats a door as having a model.
- Not done: the y + id generator intensity (FUN_100acd30; none in 230), solid doors during a sub-area load
  (FUN_10176dd0), elevators ('@' groups, kind 4, mv?? schedulers, op 0x1d), ships (kind 5), the Mog House door's swing.

## Scenery blending and the alpha cutoff (2026-10-22)

- FUN_1017c9c0 per model: ALPHABLENDENABLE = MMB header bit 31 (SRCALPHA / INVSRCALPHA from the frame's state block),
  ZWRITEENABLE off and ZBIAS 8 (1 on ATI) for those; ALPHATESTENABLE = inst+0xd6 != 0 (placements named '_' or '#'),
  ALPHAFUNC GREATER, ALPHAREF 0x60, alpha = tex.a x vertex.a x 4 (MODULATE4X). Blended models are drawn inline in
  instance order, unsorted.
- Ours: Draw.mode (bit 0 '_' cut out, bit 1 bit-31 blend), drawn opaque, cut out, blended (a blended sub-pass after
  each slot's opaque draws, close to retail's inline order), blend pipelines without depth writes and a constant
  depth bias of -8 *(inf: the D3D8 ZBIAS equivalent)*; the cutoff is keep-if > 96 / 255 (was < 0.3 discarded, and
  bit-31 models were cut out instead of blended). In 230 bit 31 sits on the ground tiles (mid_*, lef_*, rig_*).
- A dark hollow in La Theine near (-10, 35) looked like a pond without water. It is not a blending or LOD issue; the
  zone's spring effects (f_la/effe/izum: izu1 / izu2 water meshes, colour 0x30, alpha ~0x30) sit elsewhere ((13, 104),
  (-59, 136)). Whether retail shows water in that hollow is not checked (open).

## Scenery back-face culling (2026-10-22)

- FUN_1017c9c0: D3DRS_CULLMODE CCW for instances with inst+0xd4 = 0, CW for mirrored ones (inst+0xd4 set when the
  placement's scale.x * scale.z < 0); models with MMB header bit 29 draw with CULL_NONE (record +0x24 compared with the
  cached state; the earlier "vs the texture +0x40 / +0x44" reading was a decompiler artefact).
- Ours: front faces clockwise (as the character pipeline), each mesh's instances split into a normal and a mirrored
  range (Draw.instances / mirrored), cull back / front / none by Draw.cull (bit 29 clear); scenery pipelines are a 4 x 3
  table (opaque, cut out, blend, blend + cut out) x (none, back, front). 230 has 136 mirrored placements (e.g. the
  left leaf of every double door); the 335 interior's upper wall ornaments, hidden behind a back face before, show.

## Target window name (2026-10-22)

- The target window (object DAT_10578478, vtable 0x10334f18, draw FUN_101501a0; the old build's FUN_10149900) copies
  the actor's display name each frame (+0x214 when u16 +0x210 != 0, else +0x7c) and draws it with FUN_101c9fa0(W - 13,
  L + 5, T + 6, name, drawobj +0x78 = the name-plate colour, centre 1, split = no gauge drawn).
- FUN_101c9fa0: cut to 24 bytes; fit = FUN_101c96d0 (leading glyphs whose advances, width - 1 each, total <= max);
  fits and no ':' -> one line at y + 6 (centre). Else the break: after the first ':' before fit (unless right at it),
  else the last ' ', '-', '@', ''' or ':' scanning fit + 1 down to fit / 4, else fit. Split (no gauge): line 1, then the
  rest as its own name at (x + 4, y + 12), max - 4 (centre off). Gauge: cut at the break when the rest fits in max - 4,
  else at fit; line 2 through FUN_101c9d50 (the ellipsis fit, old FUN_101c04c0) at (x + 4, y + 12), max - 4.
- "Door:Count's Manor" (fit 14 of 99): "Door:" / "Count's Mano..." with a gauge in NPC green (seen live), "Count's
  Manor" whole without one or in white.
- Pets (master found): pet name, then "(" master ")" on line 2. Event mode: an event string, colour +0x68, split 0.
  Not done: the pet line, the event mode.
- Names when 0x00E has none (asm 0x1009b666..0x1009b7b5, FUN_10093430): index >= 0x700 with mask 0x08 from @0x34;
  index < 0x400 with @0x34 == 0x01 the text at @0x35; else the zone list by full server id, else "NPC".

## Objects: no target gauge, no name plate (2026-10-22)

- The 0x00E look handler (jump table 0x1009ce88 on u8 @0x30 & 7; EBX = 1, EDI = 8 set at 0x1009c8fd) ORs EDI into
  actor +0x128 (bit 3) for SubKind 2 doors (0x1009cbc4), SubKind 3 / 4 elevators and ships (0x1009cd26) and SubKind 0
  model ids 0x32..0x3b, 0x32e..0x331, 0x3c0..0x3c9, 0x737..0x746, 0x974..0x97d, 0x9ba..0x9c3 (0x1009ca3c..0x1009caff).
  Bit 3 hides the target gauge (FUN_101501a0: +0x128 bit 3 or +0x130 bit 20) and the name plate (FUN_100ce4d0), so the
  target name wraps in split mode. Nothing in this path clears it.
- Ours: Entity.object (sticky), forcing no_gauge / no_plate. The earlier note that stub-model NPCs (50 / 52) show a
  plate while targeted is ours, not retail's: they have none.

## Targeting keys (2026-10-22)

From the targeting spec (scratchpad target_spec/SPEC.md; current-build addresses, old -> new in
~/ffxi/lab/current/tables_port/funcmap.json).
- Tab / Num0 (key fn 0x84) and with Shift (0x97): FUN_10158ca0. No valid target: FUN_10083620, the nearest actor in
  front of the character's own facing (local forward >= 0, distance^2 < 2500). With a target: screen order,
  FUN_10083d00 right / FUN_10083b40 left, key x_view / max(|z_view|, 1) x focal (DAT_104568fc+0x2f4), candidates
  within 50 of the camera, smallest step under 3600; none that way wraps to the far end the other way (Tab: the
  leftmost); nothing else: the nearest. No line of sight, no height test, behind-camera actors included.
- Filter FUN_10083430 (no prompt): no name test; rejects self, actors not drawn (+0xb2 != 0, +0x8c < 0), dead or
  despawning monsters (status 2 / 3), event-untargetable (+0x128 bit 4, event ops 0x12..0x15: not done), Mog House
  layout furniture (+0xc0 == 2), and two options off by default (config 0x97, 0xaa). Doors are candidates.
- Enter with no target: the same nearest pick, only targeted; with none, "menu playermo" (the action menu).
- F8 (0x4d) nearest monster else non-PC; Ctrl+F8 (0xab) monsters only; F10 (0x52) / Alt+F8 (0xac) NPC or monster
  else non-PC; F9 (0x4e) nearest PC. All FUN_10083f20 in the nearest frame. F1 / F2..F6 / F11 / F12 (self, party,
  alliance) wait on self targets and the party list.
- Ours: client/target.rs cycle / nearest / nearest_kind / enter_nearest. Entity kind bits inferred from ours: PC by
  id, door, monster, else NPC.

## Head look toward the target (2026-10-22)

From the targeting spec section 3 (FUN_100d5b10, FUN_1002a140, FUN_1002ac60, FUN_1002b140).
- Who: the local player looks at its target; every other actor at its face target, 0x00E / 0x00D
  (u32 @0x18 >> 17) & 0x7fff with the position (actor +0x1f8; LSB facetarget). No client-side "look at the player".
  Looker status 0, 5, 0x2f, 0x30, 0x55 or 0x3f..0x53 only; target type +0xee not 3 / 4 / 5 (ours: not a door); aim
  at the target's point 3, 1.2 higher (y - 1.2) when it is hidden. Event looks (ops 0x1e / 0x4a / 0x79) win.
- Weight K+0xbc ramps 0.04 a frame (30 fps: 0.02 a tick, inf), fading with no target, a target at local x < -0.1 or
  nearer than 0.3 across; at 0 the look point K+0xb0 goes 20 ahead, in world space. The look direction about P3
  eases 1/32 a tick, keeping its distance. Ours runs both twice as fast (user's call: retail is too slow).
- Turn: d = normalize(P4 - P3) (point 4: a body point, bone 0 except the Taru's head bone 6); per bone (point 3's,
  then point 7's) the look direction is clamped to a tangent ellipse in LookAt(0, d, +y): |X| <= a, |Y| <= b at
  depth c, limits from the skeleton at the end of the point table + 0x48 + 12 i; every PC skeleton head
  (0.24, 0.16, 0.5), neck (0.16, 0.06, 0.5). Q_i = arc(d, dir_i) x weight; the residual goes on to the neck. The neck
  turns by Q7, the head by Q7 Q3 (locals turned against the pre-look neck). One bone (head) when riding or sitting
  (status 5, 0x55, 0x2f, 0x30, 0x3f..0x53); point 4 a unit higher on a chocobo / mount.
- Ours: client/actmotion.rs Look (world point), render/anim.rs look_clamp and the two-bone turn in pose_actors
  (test head_look_limits: side look gives neck 17.7, head 43.4 deg). Not done: scheduler op 0x89 suppression, the
  +0x120 bit 5 freeze, the event rate scale E+0xe0.

## Mouse cursor (2026-10-22)

From the cursor spec (scratchpad cursor_spec/SPEC.md; current build).
- Config 0021 (DAT_10456bf8, "hardware cursor", default 0): 0 = the game draws its own sprite cursor (FUN_10124eb0,
  after all window layers, UI pixels) and hides the system pointer; != 0 = Windows cursors from the game folder
  (LoadCursorFromFileA, 18 at 0x1034fbb0: mousenor, mousehit, oarw_* (loaded, never selected), arw_n..arw_nw).
  mousenor.cur is referenced by nothing.
- Sprite table 0x1036e4c8 by kind (+0x4c): 0 anc_m1, 1 / 2 anc_m2 (with an additive "photobut" glow), 3 anc_scro,
  4 kaipage x2, 5 kaipage x2 tint 0x80202080, 6 anc_m1; all in 39542. One sprite step per drawn frame (30 fps: per
  2 ticks, inf), cycle 0..5 then 0 again (a null sprite resets the counter). Rotation for 4 / 5:
  x' = x cos - y sin, y' = x sin + y cos, then scale; position - 0.5.
- Kind (FUN_10125360): field 2 over an actor (FUN_10083750) else 0; a focused window 1 over an enabled item; 3 / 6
  scroll thumb / autoscroll; 4 left-drag walk, 5 right-drag camera with angle atan2(dy, dx) - pi/2 from the press
  point and +0xa6 = arw index by 45 deg bins (10 n .. 17 nw); +0xa6 only changes in state 5. Windows cursor: 0 for
  kinds 0 / 3 / 6, 1 for 1 / 2, +0xa6 for 4 / 5. Screen edges don't change the cursor.
- Shown on motion (window messages: only past 2 px) or a mouse button; hidden by other key functions (not during a
  drag), with the UI hidden, in movies, and at start until the mouse moves.
- Ours: dat/cursor.rs (.ani / .cur), client/mousecur.rs. Our mouse scheme differs (both drags turn the camera, both
  buttons run), so a drag shows the camera arrow and both buttons the move arrow pointing ahead. Not done: kind 1 over
  menu items, 3 / 6, hiding with the UI.

## Clock grid position and map sub-keys (2026-10-23)

From the grid spec (scratchpad gridpos/SPEC.md; current [2019] addresses there).
- FUN_102201f0: "%d:%02d  %s" (two spaces) at the time's place; the suffix (FUN_10220350) is cached and rebuilt
  when its countdown drops below 0, then every 31 ticks (DAT_10385ed0 = 30); opening the compass clears it.
- FUN_101f7600(1): "" with no player or with zone flag 0x100 (0x00A LoginState 1, 4, 5); "(?-?)" with no record
  for exactly (ZoneNo, sub-key) or one needing no map (key item byte 0, 0x180); else "(%c-%d)" from the map pixel
  (Region::px, origin, 16 / 32 cells). No key-item check: unowned maps show the grid.
- Sub-key, FUN_101811d0: a vertical ray from y - 0.51 10 down, else from y + 0.51 15 up; the hit face's MZB object
  flags @0xa4 give ((f >> 26) & 3) << 3 | ((f >> 3) & 7) (collision +0x34154). Checked: Pso'Xja keys 1..16 with no
  key-0 record, Palborough Mines 1..3, Jugner 0. The region table's key is this (the old "not decoded" note is wrong).
- /clock (text command 0x3c) toggles the time line: not done.
- Ours: Ground::map_key, map::Region::find_key / here, map::grid_text, hudtop's cached suffix. The map window and
  overlay now open the record under the player (else key 0), so the 40 zones without a key-0 record open a map and
  multi-floor zones show the floor you are on.

## Window open / close animation (2026-10-23)

From the popup spec (scratchpad popupanim/SPEC.md; current [2019] addresses there).
- Registry at .data 0x103722d8 (0x2c-byte records from "menu    menuwind": name[32], handler, u32 flags @0x24,
  u8 @0x28, layer @0x29, i16 @0x2a; FUN_1015e950 copies flags to window +0x34). Flags 8 = no open / close
  animation (20 of 370 windows: inline, fep, macro palettes, loby1win / loby2win, ptcbgwin, dbnamese, ...), 0x20
  frameless, 0x100000 scaled. Unlisted windows (e.g. "menu    yesno") are 0 (inf).
- N = DAT_1036c190 = 25 ticks; config 0xa4 sets it 0 (no animation). Open (FUN_1015e530): frame part +0x26 = N; close
  (FUN_1015e6e0 -> FUN_101187f0): +0x10 = 0xe, +0x64 = N when it opened animated.
- While opening or closing only the frame draws (FUN_101177d0 at LAB_10117ac8): no window sprite, children, text or
  hand cursor. f = (N - c) / N (0 -> 1 opening, 1 -> 0 closing), linear in ticks.
- FUN_1011a250 (framed): the style frame at scale f^2 about its centre (x - (w >> 1)(s - 1), truncated), full colour;
  then an echo at 1.6 f, alpha blended, alpha ftol(96 - 128 f) clamped unsigned to 0x80 (opaque past f ~0.76: the
  last three 30 fps frames of an open). FUN_1011a150 (frameless): the window sprite likewise, echo alpha 128 - 128 f.
- Keys still act during the open (FUN_10118260 tests only the closing state). No sound from the window system.
- Ours: game_ui.rs menu_registry, win_gate / zoom_frame / end_frame; the named-window helpers (window, window_alpha,
  window_off, menu_sel, lobby_window and so yes / no and OK prompts) return None while opening. Not done: frameless
  windows (bare_window, the select list), the 9-tick press pulse on confirm (FUN_10119fb0), the in-game prompt as
  the real "menu    yesno".

## Tab candidates: the per-frame visibility cull (2026-10-23)

From the cull spec (scratchpad tabcull/SPEC.md).
- The filter's +0xb2 test is what keeps Tab on screen: bit 0 is cleared each frame by the actor update
  FUN_100c63d0 and set again when the skeleton bounding box (6 floats at the end of the point table, L + 0x18 i;
  i by status: 2 dead, 1 engaged / sitting / riding, else 0) is missing, wholly behind the near plane (0.1), past the
  draw distance, not faded in (+0x59c == 0), off the viewport after projection (corners pulled to -P._22), or fails
  FUN_1006c280's occlusion test: the rect at zmin + 0.25 (zmax - zmin) depth-tested against the scene, any passing
  sample visible; each actor every 4th frame, a hidden one reported hidden once more on its way back. Doors: a 0.6
  cube at the door position, every frame. +0x8c is the farthest corner's depth (the sort key), not a visibility test.
- FUN_10082d70 sorts ascending (near first). Config 0x3c (characters displayed) only skips mesh draws past the cap;
  actors past it stay targetable.
- Ours: client/mod.rs box_on_screen / box_corners / box_index, World::visible; render/occlusion.rs (an 8 x 8 sample
  compute pass over the scene depth every frame, read back a frame or two late; no hysteresis).

## In-game yes / no prompt: comyn (2026-10-23)

From the yes / no spec (scratchpad yesno/SPEC.md). Correction to the 2026-10-22 popup notes: the prompt is
"menu    comyn" (FUN_10210e30 on DAT_10664520), not "menu    yesno" (that string is not in the current FFXiMain;
39542's yesno layout is dead data). FUN_10210f10 only swaps the callback / texts of a prompt already up.
- Log out (FUN_101609f0), Shut down (FUN_10160960), the Mog House door (LAB_101d6040) and conf6win's key reset all
  open it. Frame 252 x 88 at layout (130, 208), window sprite windowps 336 from cell 1; Yes child 1 (windowps 68) at
  (+58, +66), No child 2 (windowps 69) at (+154, +66), 88 x 16; selected child at (-1, -1) tint 0x80805020 with the
  anc_s hand.
- Registry flag 0x1000: stacked on the log (FUN_1015f790): frame top = log top - 88 - 2, at least 16. The log holds
  its lines while a 0x1000 window is up (FUN_101626e0).
- Text inside: (x0 + 4, y0 + 4), wrapped to w - 8, 16 px lines, 0x80808080 (FUN_10211050). Callers pass no help-bar
  text; the help bar shows comyn's (empty) item text while it has the focus.
- Default No (FUN_10118e90(2)); only Left / Right move (its links); Enter answers (SE 0002), a move plays 0001,
  Escape is No. The main menu stays open under it, unfocused: highlight kept (socialme lacks flag 0x80000), no hand.
- Not done: conf6win's key-reset prompt still draws 39542's yesno (configui2.rs).

## UI hide, screenshots, Pause (2026-10-23)

- Key function 0x56 (Scroll Lock / ], FUN_10160710): menu manager +0xb4 toggles the UI; nothing draws (cursor
  too); Esc shows it again (consumed); any game-state change (zoning) shows it.
- Key function 0x55 (Print Screen, FUN_101d0c70 state machine -> FUN_101d0ab0 / FUN_101d0ea0): only with the UI
  hidden, in the field (game state 0x60), map closed. Shutter SE "0016" (FUN_10037fb0), then the capture next frame.
  Name "%c%c%c%02d%02d%02d%02d%02d%02d%c": the first three letters of the player's name (zone object +0x3c3a4, the
  chat name), local yy mm dd hh mm ss, 'a' + a same-second counter % 10; ".jpg" in the working folder, or
  "screenshots\%s.jpg" under a setting (DAT_10456ae0 / DAT_10456a84). Success: group 9 0x42 (with a remaining-count
  part from group 10 0x6e, 200 - n); failure: SE "0004" and group 7 0x8d. Ours: PNG in the game folder's
  screenshots, the count part left out, group 7 not loaded.
- Key function 0x13 (Pause, FUN_101609f0): the Log out prompt (comyn), in the field, not engaged, UI shown, no event.
- Correction to "First person": the eye uses attach point 12 (point 6 resting / seated) through vt+0x1c4, not bone
  12 (FUN_10020ba0); point 12 is on the root at head height.

## Party roster and F-key targets (2026-10-23)

From the party spec (scratchpad party/SPEC.md; current addresses).
- Roster G+0x57d8 (G = DAT_104dfd98): list 0, 20 records x 0x70, record 0 ourselves (refreshed with our name, id,
  ActIndex and zone by FUN_100e8b80). Kinds 0 / 5 both use list 0.
- s2c 0x0C8 GROUP_TBL (LAB_100e9ee0): 20 x {id, ActIndex u16, flags u8, zone u16} from @8; present ids updated or
  allocated (FUN_100ea140: first free, else the oldest update), zone and marker set on a change; every other record
  zeroed (the only removal). 0x0DD (100ea390): updates only listed ids (ours = record 0), marker +0x30 = ZoneNo @0x20
  (LSB: only when elsewhere); own GAttr G+0x97f8. 0x0E2 (100ea570): same, creates, zone set, marker 0 (LSB never
  sends it). 0x0DF (100e9cc0): HP / MP / TP / jobs of listed ids. A nameless member asks c2s 0x076 {u8 0}.
- Slots DAT_10663ad8 (18 x 0x7c, FUN_10201fb0 / FUN_10202b80): self first in party 0, our party number's members
  0..5, other numbers 6..11 then 12..17; leaders per party (flag 4) and the alliance leader (flag 8); "ptw0" iff our
  party has no leader, else "ptwN".
- F1 / F7 (0x46 / 0x4c) target ourselves (we are an ordinary entity in retail); F2..F6 (0x47..0x4b) slots 1..5, F11
  (0x53) slot 6, F12 (0x54) slot 12 (FUN_102033c0: used, in our zone, no marker, model loaded; no distance). A re-pick
  of the same target plays "0010" again. Lock-on refuses self; Enter on self opens playermo.
- Ours: client/party.rs (Roster, Slots, tests), target::pick / slot_target, self_entity for the target window,
  invalid() keeps a self target. Not done: the party windows' rows (only our row draws), party name colours, battle
  music count, the effects filter's party class.

## Macros (2026-10-23)

From the macro spec (scratchpad macros/SPEC.md; current addresses). Corrections to earlier notes: 40 books
(mcr.ttl books 1..20, mcr_2.ttl 21..40), and on the palette Down = next set, Up = previous.
- Files under <game>/USER/<content id, lowercase hex>/ (LSB: content id = char id). Container (FUN_101b9a10 /
  FUN_101b9bc0): u32 1, u32 (unwritten), MD5 of the payload, payload. mcr.dat / mcr<N>.dat (N = book * 10 + set):
  20 x 0x17c {u32, 6 x 61-byte lines, 10-byte title}; a record without a NUL in its title's 9 bytes or a line is
  cleared. Missing: mcr.dat gets 19 defaults (titles in FFXiMain, lines group 9 0x87..0xb5), others are cleared and
  written back. mcr.sys u32 = book * 20 + 2 * set. Read at every field entry, written when leaving and on a set switch.
- Keys: Ctrl / Alt + 1..0 (0x6d..0x75, 0x6c / 0x63..0x6b, 0x62) run macros 0..19 at once; Shift + Ctrl / Alt + digit
  (0x59..0x61, 0x58) picks set 1..10. Gate FUN_101a33e0: field, UI shown, no sub-target, no event hold, alive, no
  other window. The palette (Ctrl / Alt held 0x76 / 0x77) shows at the next whole-second tick: display only.
- Runner FUN_101a1cc0: one line a frame (ours a 2-tick frame) through the text-command path in macro mode; the
  first empty line ends it; "//" lines skipped; "/wait n" (1..60) waits unsent; "<wait n>" (0..60) sends the line cut
  at the tag, then waits. A new macro replaces the running one; nothing but leaving the field stops it.
- /macro set|book N (cmd 0x16): three tokens, out of range wraps once.
- Ours: client/macros.rs (files, defaults, runner, tests), keys and /macro in mod.rs. Not done: the palette windows,
  <t> / <pos> / <st> expansion, auto-translate bytes, the editor.

## Combat motions: swing holds, reactions, the kill, lock camera height (2026-10-23)

From the combat spec (scratchpad combatmo/SPEC.md).
- Every swing scheduler holds both actors from swing start: op 0x59 on the attacker (u16 @6, about hit + 30: no hit
  reactions, server status changes wait) and op 0x20 on the target (until hit + 4). Status changes (0x00E / 0x037)
  apply to the applied status (+0x170) only when the hold ends; death, target drop and the sheath read it. So the
  mob falls at hit + 4 and the target / lock drop then; our disengage waits for the swing to finish, then out N runs.
- Reactions (FUN_100cbd20 / FUN_100cbe30): a hit reacts only at hit distortion (result scale & 3) above 0; a miss
  plays nothing; guard / block gdi, parry pyi; none while dead / riding or in the reactor's own op 0x59 window.
  Retail mixes them as overlays in regions 5 / 6 at 25 / 50 / 100 % by the level (2 cycles, 24-tick fade); ours still
  plays a full-body one-shot (not done: the overlay layer).
- A newly started scheduler motion wins over a running one-shot (ours restarted the swing from frame 0 when out N
  started mid-swing: the "double" last swing, and the sheath hidden behind it).
- Locked camera (FUN_1001ee60): the target height is a snapshot at lock-on of the actor's look point (+0x8a8: init op
  0xad's, else point 7), x0.8 for a Tarutaru player; the player's attach point 13, posed, every frame. We read bone 5
  / bone 13 every frame (bats bobbed the camera).
- Not done: multi-hit rounds replay the swing per hit; the weapon attach switch on sheathing.

## Text commands (2026-10-23)

From the text command spec (scratchpad textcmd/SPEC.md); client/textcmd.rs.
- FUN_10080dc0(line, mode): mode 0 menus, 1 typed, 2 macro, 3..7 sub-target re-runs. Trim; a line without '/' gets the
  chat mode's prefix; the tag pass (FUN_10076ab0); a 150-byte cut (FUN_100769f0, never inside a character); tokens
  (FUN_10076830: spaces, tabs too outside typed lines, quoted tokens run to the matching quote and lose it, at most 17);
  the name looked up exactly and case-sensitively in the table at 0x10355408 ({char name[0x14], i16 id, u16 flags},
  first entry "/ver", then "/say"; 277 entries ending with an empty name, id 0x1d4, then a negative id). There is no
  /y (only /yell); /h is /help. An unknown command prints nothing; a handler failure prints ">> expanded line" + group 9
  0x78 (menu lines only 0x78), log mode 0x9d.
- Flags: 0x02 allowed with the sub-target cursor up, 0x08 name tags, 0x10 UI free (group 7 0x42 while the full log is
  open), 0x20 dead, 0x40 mounted, 0x80 event, 0x100 target command (<t> <me> <bt> <pet> <scan> <ht> <lastst> <ft> stay
  literal for the resolver), 0x1000 no gate. State gate FUN_10081230: group 7 0x4f dead, 0x4b mounted, 0x4d fishing,
  0x43 event. Status gate FUN_101cc0d0(mask): 0x10 healing 0x51, 0x20 sitting 0xa0, 0x01 mounted 0x4b, 0x02 fishing
  0x4d, 0x04 synthesis 0x4e, 0x08 zone flag 0x100 0x44.
- Target resolver FUN_1007a1c0(text, fallback, wide): the tags, then a name normalised by FUN_10077c30 (3..15 ASCII
  letters -> Xxxx) over PCs 0x400..0x6ff only (NPCs and monsters can't be named), with `wide` also 0x700..0x8ff when
  ours / the party's (inf: +0x12c bit 22 / FUN_1008e260), a numeric server id in internal modes, else with `fallback`
  the current target.
- /ma /nin /so (FUN_1007b710, gate 0x7f): spell name case-insensitive exact (55702), type filter 4 / 5, learned bit
  (s2c 0x0AA, 128 bytes) and FUN_1022b7d0's level rule, then the target (no fallback, wide); 0x01A category 3, u32
  spell id @0xc, ground offset floats @0x10. /ja /pet (FUN_1007b900): ability records 0x200..0x3ff (not +0xe
  hidden) then 0x400..0x5ff, category 9, param id - 0x200; a job ability must be in 0x0AC's job ability bits (bit =
  id - 0x200; pet bits are LSB id - 496, not checked: inf). /ws (FUN_1007b820): engaged only (group 7 0x52), records
  0..0x1ff, no learned check, category 7; retail sends ids above 0xff as id + 0x2000, LSB reads the id as is, ours
  sends it unchanged. /attack on|off|<target> (group 7 0x95 not attackable, 0x0F when engaged), /assist (0x0C, nothing
  while engaged), /target, /check /checkname /checkparam (0x0DD kind 0 / 1 / 2), /ra (0x10), /help (5 on self),
  /random (0x0A2), /echo (mode 0xce), /sit (0x0EA). Echo: linkshell "[1]<Name> text", yell "Name[Zone]: text".
- Spell records: file 81 chunk 0x49, 1024 x 100 bytes; ability records chunk 0x53, 0xb00 x 0x30 bytes; each record
  unmasked by FUN_1021e870 (rotate by (popcount b2 + b12 - b11) % 5; bytes 2, 11, 12 untouched). Spell: +0 id (= its
  record index and name index), +2 type (1 white .. 8 trust, ids 896..1023), +4 element, +6 targets, +8 skill, +0xa
  MP, +0xc cast, +0xd recast, +0xe 24 x i16 job levels (job 0..23, -1 none), +0x5c job point mask.
- LSB's !addalltrusts adds with sendUpdate false: no new 0x0AA until the next zone-in, so the client refuses them
  until then (retail would too). Trusts can't be called in towns (the server refuses).

## Dolls take only their task's clones (2026-10-23)

- Scheduler ops 0x15 / 0x16 / 0x22 / 0x23 (FUN_10057fb0 cases; doll FUN_100abea0) call FUN_10062d50 with ECX = the
  running task (asm 0x10058b82 MOV ECX,ESI): it walks the actor's attached list and moves only CMoGeneratorClones
  whose +0xf0 is that task. Ops 0x17 / 0x18 move them back (list +0x68, FUN_1003b7b0). Ours moved every instance on
  the actor: a trust's appearance scheduler (eifa, from the cast finish's shfa) dolled the caster and took the
  cast's own fa01 / fa02 / 5061 with it, so stfa's stops (0x5f nefa, 0x2d fa02 / fa01, 0x1e 5061) missed them and the
  casting sparkles never ended.

## Spawn-in: 'spop' (2026-10-23)

From scratchpad spawnin/SPEC.md (the 0x00E handler is FUN_1009b550, pushed at 0x1009443c; not a function in all.c).
- 0x00E (and 0x00D) mask 0x04, u32 @0x28 bit 2 -> actor +0x12c bit 23 (LSB PetNewFlag; trusts always, pets on their
  first spawn, mobs with specialSpawnAnimation). Only FUN_1008f740 reads it: the model is made by FUN_100c56e0(pos,
  actor, 'spop'), whose colour starts at 0x00808080 (alpha 0) unless +0xee is 0 (PetFlag @0x28 bit 6). FUN_100d1070
  runs model +0x8c4 ('spop') through vtable +0x298 (FUN_100cee40: the actor's files, then DAT 0) once every file is
  in, retrying each frame. Trusts carry their own 'spop' (Kupipi: in DAT 99909, running file 3721's main: sound,
  16 generators, the fade); DAT 0's runs file 0xaf0 + 0x268 = 3416: op 0x29 alpha 0, ops 0x59 / 0x07 / 0x2e motion
  locks 80 ticks, a doll, sound 7088, ds00 / ln00 / gp00, op 0x29 fade to 0x80 over 40 ticks from t 31.
- Op 0x29 (FUN_1005f6c0 / FUN_1005f820): the real actor's (doll unwrapped) model colour from its current value to
  u32 @8 over dur * speed ticks, linear per byte; dur 0 = at once. 'kesu' is this op (wait 90, dur 90 to alpha 0);
  our hand-made 90-tick kesu fade is gone.
- Effects on a hidden actor: the test is +0xb2 bit 1 (server hide bits), not alpha; a spop actor at alpha 0 still
  takes its generators.
- Despawn (mask 0x20): the client plays nothing; the fade is the server's earlier 0x038 'kesu'.
- Not done: ops 0x07 / 0x59 / 0x2e motion locks (model +0x80 / +0xfc counters, actor +0x11c).

## Magic and ability menus, the target prompt (2026-10-23)

From scratchpad magicmenu/SPEC.md and icons/SPEC.md; client/magic.rs, target.rs (Sub), textcmd.rs (`<st>`).
- Windows: mgcmenu (main menu child 3: 8 categories, list mode, Trust row 8), magselec (cast mode, no Trust), abimenu
  (child 5), abiselec (action menu Abilities), "menu    magic" / "menu    ability" (12 rows at children 1..12, icon at
  the child, name at + 18; colours 0x80808080 / 0x80404040 refused / 0x80803040 cursor row), subwindo (magic "MP
  Cost" 9 0xe6 / "Next" 9 0xe7 "%d:%02d" right-aligned at +0x69). Openers: action menu 8 Magic (all but Trust, cast
  mode), 0x19 Trust (type 8), 7 Abilities (abiselec); Ctrl / Alt + M (list mode, no Trust), J / W / B (job abilities,
  weapon skills, pet commands). Empty: 9 0x4a "No spells learned." / 0x4b "No spells available.". Help bar: the row's
  description (55734 spells / 55733 abilities), title 0xd 76 "Magic List" / 68 "Abilities".
- Rows: known spells (0x0AA) / abilities (0x0AC, bit = table index) in id order (retail's order with no mix.dat /
  aix.dat; not read yet). Cast mode (FUN_1022c3a0) needs 0x119 seen (DAT_10485824 bit 0), FUN_1022b7d0, SCH / GEO main
  for record +0x44 bits 8 / 0x20; blue magic left out (needs s2c 0x044's set spells, not read).
- Icons (FUN_10220d40 -> FUN_10220cf0 table DAT_10666e88): kind 5 "menu    magiconw" (file 39551), kind 6 "menu
  magico2w" (file 39560, loaded by FUN_10010080). HD (config 0x99 bit 2 clear, the default) and the HD value
  (spell +0x42, ability +4) not 0xffff: kind 6 (value - 1000) from 1000, else kind 5 (value); otherwise kind 5 (spell
  +0x40, ability +3). 16x16 units, drawn 0x80808080, never greyed.
- Enter (FUN_1022c7a0): FUN_1022cf30's line (fishing 0x4d, mounted 0x4b, healing 0x51, sitting 0xa0, zone flag 0x100
  0x44, recast 0x59 / trusts 0xbf, 0x119 missing or job / level 0x57, MP 0x58), then the prompt for record +6's target
  flags (failure 0x16), the list hidden; confirm runs `/magic "<name>" <id>` (mode 0) and closes the list, cancel shows
  it again. Abilities (FUN_102241a0): +0xe != 0 opens that sub-type list; traits 0x5e; recast 0x5c; WS not engaged
  0x52, TP < 1000 0x5f; target failure 0x1b; `/weaponskill` (type 3), `/monsterskill` (0x14), else `/jobability`.
  Esc on an ability sub-list reopens its parent type (table 0x10386159). Spell recasts: our 0x028 category 4, header
  info seconds, 0 = record +0xd * 15 ticks. 0x119: 31 x {u16 seconds, u8, u8 timer id, u16, u16}.
- Target prompt (FUN_10159440): DAT flags -> include / exclude kinds by table 0x103720c0 (1 self 0x200, 2 0x100, 4
  party 4, 8 alliance 8, 0x10 PC 1, 0x20 monster 0x10, 0x40 NPC 2, 0x80 NPC minus 0x1800 pets / trusts); first pick
  the current target if it fits (not an enemy when self is allowed), else us, else the nearest candidate; the target
  and lock are saved and restored. Keys: Enter confirm (SE 0009), Esc cancel, Tab / F8..F10 among candidates of the
  kinds, F1 / F7 us (inc 0x200), F2..F6 / F11 / F12 slots (inc 4 / 8); works locked on. Arrows: the saved target's as
  usual, the pick's 10 px higher tinted by range (spell +0x45 via table 0x103644a8; red past range + 0.5 or at 25,
  yellow past range - 0.5, blue), forced flash unless red. Bars: helpwind slot 2 0xe 0x4f / 0x51, title the name.
- Text tags: `<st>` 0x7f, `<stpc>` 0x1d, `<stnpc>` 0xa0, `<stpt>` 5, `<stal>` 0xd in a target command (or flag 0x08)
  open the prompt with the text before the tag; confirm re-runs it with the pick's server id (quoted name for flag
  0x08); refused in an event (7 0x43) or zone flag 0x100 (0x44); nothing fits 0x46 and a macro stops. While the prompt
  is up a typed line needs flag 0x02 (else 7 0xa4), macro lines wait. A /ma line's dry run gives the arrow its range.
- Not done: mix.dat / aix.dat order and the sort windows, the Right-key magselec pop-out, blue magic sets, mouse in the
  lists and prompt, the mount list, subwindo pet lines and charges, the cast bar. (`<stpt>` / `<stal>`: see "Party-list
  cursor" below; correction: they do not open this prompt, and their flags 5 / 0xd are never read.)

## Name colours: the rule (2026-10-23)

From scratchpad ncol/SPEC.md (FUN_1008db40; 0x00D FUN_10099ae0, 0x067 FUN_1009cee0, 0x037 FUN_1009d0f0 decompiled
there). In order: event op 0xae override (+0x1d8 < 0x17); SubKind 2..4 -> 4; applied status 2 / 3 -> 9 grey; PCs on
a team (allegiance 2..99 but not 0x28..0x2b) team colours 0x12..0x16, party members cyan; GM level 3..7 (@0x23 & 7) ->
0x0c..0x10; the monster flag (or PvP allegiance 8 / 9) with a claimer: a player's pet cyan, claimer in our party 6
red, in another alliance party (c6 >> 1) + (c7 >> 1), else 7 magenta (red with our +0x18c shared-claim flag); anyone
in our party but us 1 cyan; yell flag (0x00E: LSB CallForHelp) 8; LFG / LFG master / auto-party 2; anonymous 3; PCs
and allegiance-1 actors without the charm flag 0 white; no charm flag (@0x27 bit 3): 5 monster, else 4 NPC; charm +
@0x28 bit 6 or @0x2B bit 1: cyan only for a trust (@0x28 bit 0) whose owner index (0x067 type 3, +0x2a4) is ours,
else 4; otherwise (a player's pet) 1 cyan. Pugils came out green because @0x25 (hitbox 23 -> 0x17) has bit 3 clear;
LSB trusts keep @0x25 = 0x08 and their owner in @0x2C, so ours read as "claimed by us" (red).
- Ours: the monster flag from @0x20 bit 0 everywhere (auto lock-on, Enter, F8, the action menu); the rule above
  without the override, PvP teams / ballista, the charmed pet's master and the shared-claim flag.

## Door target arrow (2026-10-23, corrects "Door target arrow at the door part's origin")

- The arrow's screen point is the target actor's vtable +0x27c (FUN_10159d40 from FUN_10150090); for an XiDoorActor
  (vtable 0x1032fd58) that is FUN_100ac390: its position (+0x1bc, the 0x00E position) projected, with no lift and no
  part matrix. FUN_100acab0 (attach point k = door part k's origin, 4 = the position) serves effects, not the arrow.

## Hit reactions as overlays (2026-10-23)

- Ours now follows FUN_100cdd60 / FUN_100cde70 / FUN_100cdf00 (combatmo spec section 2): the reaction goes into an
  overlay layer (Actor.overlay, actmotion::Overlay) over whatever motion plays: a hit at 0.25 / 0.5 / 1.0 by hit
  distortion (record scale & 3), 2 cycles, in over 1 tick, out over 24; guard / block / parry 1.0, 4 cycles, 16.
- GPU (render/anim.rs): the job header grew to 32 words: 26 overlay animation offset, 27 frame, 28 weight. For each
  bone the overlay keys (FUN_1001a420's regions 5 / 6), the local transform of both the current and the blended-from
  pose slerps / lerps toward the overlay's by the weight before chaining; unkeyed bones are untouched.
- Not done: two overlays at once (a new one replaces the old), the CPU pose for the locked camera ignores it.

## Cast bar (2026-10-23)

- "menu    casttime" (128x24, frames 58 "Casting Time"; layout (16, 94), drawn at (16, 47) unless gaugewin is open,
  FUN_1012c0f0): opened by our 0x028 category 8 with cmd_arg "ca??" (FUN_1022b3d0, spell = the result's value) for the
  spell's cast byte x 20 ticks (Cure 8: 160 ticks, LSB casts it in 2 s); any other category 8 ("sp??", an interrupt)
  closes it (FUN_1022b420). Draw FUN_1012bf60: windowps 38 / 97 gauge at (+8, +9) by the fraction done, "%d" right-
  aligned at +0x6a and '%' at +0x70, y + 7, sprite font. Not done: Chainspell (status 48) skipping it, items (mode 1,
  item +0x26 x 20), the event mode 4, gaugewin.

## The mid-fight "flinch" (2026-10-23)

- Cause (ours): a swing one-shot runs for its scheduler length (op 0x05 u16 @6), which can exceed its animation: an
  Elvaan's dagger at0 is 110 ticks over a 46-frame (92-tick) motion, at1 100 over 46 frames. During the swing the frame
  clamps, but when the stance took over, the blend's "from" pose evaluated the finished swing as a loop, so it jumped
  back to frame 9 / 4 (the wind-up) and blended into btl over 8 ticks: a swing starting and stopping at once, after
  every at0 / at1, never after at2 (56 frames, 112 ticks) nor on Hume swings (all within their animations). Retail's
  queue (FUN_1001a8e0) holds the last frame of a region < 3 entry whose count ran out; the from pose now holds it too.
- Also from this hunt: hit reactions were not the cause (reactions fire only on hits; misses play nothing).

## K.O. window (2026-10-23)

From scratchpad ko/SPEC.md. FUN_10096c90 mode 1 opens "menu    dead" (FUN_1012e0c0) on the first frame our applied
status is 2 / 3, after closing every window FUN_1015f3e0(0x800) closes (main menu, items, config, ...; the logs, bars,
party, map stay). Deadline = end - 360 s, end = dc2 when ahead of now else now + dc1 / 60 (0x037 @0x3C / @0x40, only
with dc1 > 0; 0x00A DeadCounter @0xA4: now + it / 60). The window: framesus 65 ("Time Left:" baked), 144 x 44 by
FUN_1011ad50 (non-Japanese: the frame sprite's bounds), child 1 windowps 254 "Back to Home Point" with the hand; the
time "%d:%02d" (minutes unwrapped) at (+75, +6) in the sprite font, 0x80808080; help 0xe 0xf1, title 0xd 0x39 "K.O."
from child 1's strings. Enter / click: comyn 0xe 0x46 "Return to Home Point?", default No; Yes sets +0x24 and the state
machine sends 0x01A 0x0B, again every 600 ticks while dead; the deadline passing does the same and closes the window.
Escape is not taken (the log shrinks). Alive again: all of it goes. Not done: raise / tractor offers (s2c 0x0F9 ->
resynus / comyn 0x60, 0x01A 0x0D / 0x13), camp mode, mp_dead.

## Multi-hit melee rounds (2026-10-23)

- A 0x028 category 1 with several records (double attack, dual wield, kicks): retail's "atk0" plays the swing for
  record 0, then op 0x32 moves actor2 to the next record's target and jumps back: one full swing, its dada (message,
  reaction) per record, each starting when the previous swing's call returns. Ours (combat::on_action) splits the
  round into one-record actions, plays the first and queues the rest (World::rounds) to start when the attacker's swing
  one-shot ends; a new round flushes a queued remainder at once. Not seen live yet (no double attack in the test fight).

## Emotes: s2c 0x05A (2026-10-23)

From scratchpad emote/SPEC.md. Handler LAB_100a06e0 (not a function in all.c; all.asm ~172896): caster @4, target @8
(0 none), indices @0xc / @0xe, emote u16 @0x10, Param @0x12, Mode @0x16, five trust ids @0x18 / indices @0x2c.
Dropped for emote 0x61+, a caster / target not in the actor table, or a caster on the block list.
- Motion (FUN_100a0b90, op 0x6e's wrapper FUN_100a0b60 passes val >> 8 as Param): sitting (0x2f) / seated
  (0x3f..0x53) only emotes 4..9, 13, 37 move (tables 0x1035b7b8 / 0x1035b6f0: 64.. / 232..); else table 0x1035b880
  with the forms salute Param + 2 (nation), /hurray 0x48 + the main weapon's motion class (item record byte 0x28; 6
  bare-handed), /bell Param + 0x68, /jobemote (Param & 1) + 0x88 + (Param >> 1) * 8, /aim 0xe0 + the ranged class
  (Param 0xffff fails: group 9 0x66 in mode 0x79 instead of the line). Skipped (not queued) while the caster emotes,
  isn't idle / sitting / seated, or Mode 1. The pet copies it; trusts copy unless config 0x99 bit 9.
- Line: file 7025 (base slot 0x62, dialog format), entry emote * 2 with a target, + 1 without; 7f 90 / 91 [his/her]
  of the caster / target (0x00D u32 @0x20 bit 15 = male; no operand byte), 7f 88 the article. Mode 7 for our own
  untargeted emote, else 0x0f (both config 0x20's colour); not with Mode 2 or the Emotes filter (config 0x32 bit 3).
- Ours: the above but the block list, pet / trust copies, /aim's ranged class (0), our gender from the race (inf).

## Event special values: race, job, server id (2026-10-23)

- FUN_100af4c0 (all.c ~130780): 0x7f07 / 0x7f87 = the bound actor's / player's race byte (+0xef; 9,374 player.7 and
  2,118 self.7 reads, e.g. Port Windurst 0x131 picks camera s100 over s008 for a Galka); 0x7f06 / 0x7f86 main job,
  0x7f08 / 0x7f88 its level (DAT_10485640 / 41 = s2c 0x061 @0x0C / @0x0D; 0x7f08 is 1 for a VM not the player's);
  0x7f0a / 0x7f8a the server id (+0x78). Ours: Host::race / server_id cached per tick, player_word from 0x061.
  0x7f0b / 0x7f8b (+0x124 bit 25, 475 sites) not decoded. `event-run --race N` sets the player's race offline.
- Event VM audit 2026-10-23: scratchpad evgaps/STATUS.md re-ranks REPORT.md's items (11 done, 1 approximate, 6
  partial, 4 missing) and the special values above.
- Op 0x90 (FUN_100b3540): the event actor hidden as op 0x22 1 (+0x120 bit 17). Op 0xb5 sub 0 (FUN_100b10f0): the
  event actor's name = str@2 (24 bytes into +0x7c) unless it has an alternate name (+0x210); ours sets the entity name.
- Op 0x92 (FUN_100b7d10, actor@2 +0x12c bit 16 = u8@1) is NOT "hide in the event": scripts set it to 1 on the
  actors that then speak (Southern San d'Oria event 0x17: npc05f, npc0ed, each with op 0x94 beside it). Its reader
  FUN_100977f0 gates something in FUN_10097840 for PCs (not traced); left alone.
- Op 0xb4 (FUN_100b0a00; 5,146 sites): every sub ends the frame. 0 / 0x13 write the 16 literal bytes at pc + 4 into
  str@2 (byte 15 zeroed; 0x13 '@' -> ' '); 1 reads event string slot val@4 (0..3) into str@2, 4 writes it (DAT_104882b8
  / DAT_10488d08; ours: Start::strings, which the message string codes read); 9 / 0xc read the cast bar's / event
  timer's progress (ours 0: those windows aren't modelled); 0x15 waits while the map window is open. 2 / 3 target
  window, 7 / 8 casttime, 0xa / 0xb eventtim, 0xf..0x12 chocobo race windows, 0x14 map view: host effects only so far.
  Sweep of zones 230 / 240 after: 1,450 of 1,489 events finish headless, 0 faults.
- Smaller VM fixes (audit R10 / R14 / R15 / R20 / R21): 0xb0's talking mouth is actor@6 (the prefix stays actor@2);
  0xbe stores the caller's real server id; a 0x5b / 0x66 / 0x5f motion named 0 or 'xxxx' only loads its file; 0xb6
  0xf / 0x17 set the event actor's / player's scale (+0x204 = val@2 * 0.01, Actor::event_scale); frame ends after b5,
  c1, 7e, 71, a7, b3 and the 0x87 / 0x88 / 0x8c / 0xa6 sends. Sweep 230 / 240 unchanged (1,450 / 1,489, 0 faults).
- Op 0xac sub 1 (FUN_100bb180): the event actor's event status +0x174 = val@2; a bound actor poses with it (+0x170)
  while the event runs. City zones 231..246 use 0 (13), 1 (battle stance, 11), 34 (2), 2 (corpse, 1). Ours:
  State::event_status, read by actor_place; riding 5 / 0x55 skipped until the chocobo / mount model is spawned
  (0x7e waits for that too). Subs 0 (server status), 2 / 3 (update freeze, c2s 0x016), 4 not done.
- Op 0xa7 (FUN_100baa60, 108 sites, battlefield registration NPCs): sub 0 sets body +0x180 bit 0 and yields; the next
  call sends the update (c2s 0x05B mode 1, FUN_100a0510), zeroes +0x184, advances 2, yields. Sub 1 yields until bit 0
  clears, then set@2 = +0x184 and advances 4. s2c 0x0BF (FUN_100a2160 -> FUN_100bab20; LSB
  GP_SERV_COMMAND_REGISTRATION: Result u16 @6, ActIndex u32 @0xC) finds the actor by ActIndex and, if it runs an event,
  stores Result in +0x184 and clears bit 0. Ours: Runtime::registration on the running event (the index is logged).
- Packet ops (audit R17), each call ends the frame; a failed packet alloc retries without advancing:
  - 0x8c (FUN_100b98e0, 196 sites, guild image-support / recipe NPCs): c2s 0x058 RECIPE (0x14 bytes) with mode +0xa
    = sub (1 for sub 0). Sub 0 {+4 val@2, +6 val@4, +8 val@6} len 8; 2 {+4, +6, +8 = 0, +0x12 val@6, +0xc val@8, +0xe
    val@10} len 12; 3 / 4 {+4, +6, +0x12 val@6, +0x10 val@8} len 10; 5 {+0xc val@2, +4, +6, +0x12, +0x10, +8 val@12}
    len 14, arms no wait. Sub 1 waits while DAT_10480a90 is set, then len 2. s2c 0x031 (FUN_1009f7c0) clears it; type
    @0x30 1 / 3: Z40..Z44 = u16 @4..@0xc, Z55 = u16 @0xe, Z45.. / Z4D.. = items @0x10 / counts @0x20; type 4 the same
    but one slot per unit (FUN_1009f8e0, count 1 each); type 2 a list: Z2..Z9 = u16 @0x10.., 0x1700..0x1707 = @0x20..,
    0x1708 = @0x32. Southern San d'Oria 0x28a (Orechiniel) sends skill 5, mode 1 headless.
  - 0x87 / 0x88 (FUN_100b9140 / FUN_100b9220, world pass NPCs): c2s 0x01B FRIENDPASS (0x1c bytes) Para +4 = 0 / 2
    for 0x87, 1 / 3 for 0x88 (sub 0 / 2); sub 1 waits on DAT_1048557a (zeroed at every event start). s2c 0x059
    (FUN_1009fa00): Z2 = i8 @0x20, Z3..Z5 = u32 @4 / @8 / @0xc, Z6 = i8 @0x21, string slot 0 (DAT_10488d08) = the 16
    bytes @0x10, then the flag clears.
  - 0xa6 (FUN_100ba9c0): sub 0 c2s 0x0EB (header only), sub 1 waits on DAT_104804c0, sub 2 set@2 = DAT_10485a28. s2c
    0x10E (100a2100), only while waiting: DAT_10485a28 = u32 @4 (LSB MapNum).
  - Ours: Packet::Raw, Runtime::answer(id, packet); headless runs answer with empty packets. Sweep 230 / 240 unchanged.
- Op 0x6a volume fades (audit R16): each channel gets a fade task (FUN_10035a00, one per channel: 1 DAT_1046392c,
  2 DAT_10463928, 4 DAT_10463930, 8 DAT_10463934) starting at the channel's level; its tick FUN_10035940 takes the
  frame's ticks off the remaining count and sets from + (to - from) * (1 - left / total) (constants 0.0 / 1.0 at
  0x103295d8 / 0x1032961c), ending on the target once below 0. A replaced or cancelled task lands on its own target
  first (destructor FUN_10035b00); 0x69's sets cancel the fade the same way (FUN_10036020 / FUN_10036070). The
  channel setters multiply by DAT_103516fc * DAT_103516f8 (config) while the task starts from the multiplied level,
  so retail compounds the config factor during a fade (not copied). Channel 4 (zone ambient loop) re-levels its
  voice DAT_10463910 when it changes; the others are read when a voice starts. Ours: Audio::fades, step_fades.
- Op 0x59 subs 0..3 (FUN_100b6a60, audit R18): 0 / 2 the event actor's +0xdc / +0xe0 = val@2 (len 4), 1 / 3 actor@2's
  = val@6 (len 8; no advance while actor@2 is missing). Both live on the actor (getters FUN_10084b40 / FUN_10084b80,
  vtable slots +0x13c / +0x144 of 0x1032cfe0, 100 when unset) and are read only while FUN_10087060 holds (event
  running, DAT_10487e48 *(inf)*), and only when not 0:
  - +0xdc, the turn speed: FUN_100c63d0 eases the model's drawn rotation (+0x61c..+0x624) toward the actor's (+0xe4)
    once a tick, angles wrapped, by 0.125 or +0xdc x 0.00125 (0x10331364; 100 = the default).
  - +0xe0, the head-look speed: FUN_100d5b10 steps the look weight (K+0xbc) by 0.04 or +0xe0 x 0.01 x 0.04
    (0x10329a18 / 0x1032a85c).
  - Ours: Actor::turn_pct / look_pct, turn_rate() for entities and event poses, Look::step's rate.

## Event input window: op 0x71 and "menu    passinpu" (2026-10-23)

Full spec: scratchpad passinpu/SPEC.md of that session; the facts:
- Op 0x71 (FUN_100b7e10, 5,585 sites), every sub ends the frame, unknown subs stay. Globals: DAT_10489930 input open
  (cleared only by the callback LAB_100b83d0), DAT_1047fab0 waiting for the server (cleared by s2c 0x052 mode 3,
  handler 0x100a2050, and at event start), DAT_104886e0 the 16-byte text (kept across inputs).
- Opens (FUN_10206540(cb, 0, kind, max); an open while one is up does nothing): 0 kind 0 max 16 (len 2); 0x10 kind
  val@2 + 1 max 8 (4); 0x12 kind val@2 + 1 max val@4 (6); 0x30 kind val@2 + 3 max 16 (4); 0x32 kind val@2 + 3 max val@4
  (6). 1: once closed, c2s 0x060 PASSWARDS {+4 target u32, +8 index u16, +0xa 0, +0xc text[16]} (len 2); 2: waits for
  0x052 mode 3 (len 2; LSB answers 0x060 with onEventUpdate, 0x052 mode 1, then mode 3); 3: set@2 = vulgar-name check
  (FUN_10094160, "entryz.dic" / "VulgarName", not in the install: 0); 0x11 / 0x13: once closed, set@2 = atoi(text);
  0x31: once closed, the 16 bytes into str@2. 0x20 / 0x21 maze preview ("menu    prvdungp"), 0x40 / 0x41 linkshell
  picker ("menu    lscswin"), 0x50..0x55 a full-screen picture (zone 291): not done.
- Kinds (title from group 0xb by table 0x103854c4): 0 none, child 1 the "Password:" sprite (windowps 328), text at
  x + 80 (92 fr / de), printable ASCII; 1 0xb9 "Enter quill amount: " digits; 2 0xbe "Enter number:" digits; 3 0xb9
  letters; 4 0xc8 "Enter password: " letters; 5 0xc9 "Enter name: " letters. Max clamped to 16.
- Window: 16, H - 38, 366 x 22 (the msgline rect, DAT width kept), registry flags 0x12 (animated, Escape does not close
  through the generic path). Title at (x + 4, y + 4), the text right after it, 200 px around the caret, steady '|'.
  Opening sets the log's input-line flag (DAT_1062193c +0x4c) and takes the text engine from the chat line.
- Keys (shared text engine FUN_101b5d10): over max SE 0004, other characters dropped silently; Left / Right / Home /
  End / Backspace / Delete edit. Enter (SE 0002, none when empty), Escape (SE 0003) and Up / Down (silent) all close it
  returning the text as typed: there is no cancelled result.
- Ours: Effect::Input, Packet::Passwords, Runtime::input_done, release mode 3; client event::Input, input_events,
  draw_event_input; topbar-shot --window passinpu:KIND:TEXT. Headless: Scripted::inputs; Lower Jeuno 0xc7 sends the
  typed name and finishes. Not seen live (this server has no Lower Jeuno / Port Jeuno NPCs to talk to).

## Map window: pan, zoom, fades, event maps (2026-10-23)

Full spec: scratchpad mapwin/SPEC.md of that session (current-build addresses; the "Map window" section above uses
2019 ones: base draw FUN_101eebd0 [101e6ca0], open FUN_101f7810 [101ee8f0], core open FUN_101f9a50 [101f0af0], base
update FUN_101ef0a0 [101e7170]). Done so far:
- Zoom: fit = W / 512, range [fit, 4 fit]. Base zoom mode 0 = fit x config 0x8a / 1000 (default 2000), mode 1 = 1.75
  fit (0x1033a910), others fit; a no-map record always fit. Closing writes config 0x8a = zoom x 1000 / fit whatever
  the mode (FUN_101fa420). Keys (context 0x26, mask 4): 0xa4 / 0xa5 (PageUp / PageDown, Num9 / Num3, Ctrl / Alt + . / ,)
  and the compact I / K stick (axis 0x7c) 0.04 a tick, doubled with 0x9a (LAlt); 0x7d (Num5, Ctrl / Alt + V) or both
  held eases back to the base zoom (a fifth a frame, done under 0.003). FUN_101ede30 keeps the map pixel under the
  cursor point (W x 200 / 512, H / 2). No mouse-wheel zoom in retail.
- Scroll: axes 0x79 / 0x7a (Num6 / Num4, Num2 / Num8, compact W / A / S / D; FUN_10122e30) x 15 x ticks / 2, doubled
  with 0x9a; arrows never scroll. Under 2 px a retail frame is dead (ours judges that per 2-tick frame, so it works at
  any frame rate). 0x7e (Num*) eases back to the player. Bounds FUN_101ee040: view in [base, base + (512 zoom - W) /
  zoom]. The left-drag joystick (W / 21 from the press point) is not done.
- Open: view = player pixel - cursor point (screen pixels from map pixels, as decompiled), clamped, then the scroll-to
  (FUN_101ef990: each 2-tick frame the view moves (target - point) / zoom map pixels, halved over 2 x zoom; one exact
  step once it moves under sqrt(0.8)). Fade level 0x80 -> 0 at 8 a frame; the image alpha min(0xff - level, 0xa0); the
  player arrow and the grid text only at level 0. The arrow's pixel and heading are taken at open only.
- Close (slot 20, FUN_101ee580): map0 and the other sub-windows go at once, the image fades to 0xff at 0x10 a frame,
  the window goes when that ends or after 3 s; then the close callback reopens the main menu (LAB_101faff0).
- /map (command 0x4f, FUN_1007d2c0): group 7 0x44 in a room (zone flag 0x100), nothing when already open, else mode 0.
  No record: group 7 0x62 "There is no map for this area." (FUN_101f86b0).
- Event maps: 0x89 (zone val@1, key -1), 0x8d (key val@3), 0xc8 (arrow with val@5) open modes 1 / 2 (zone <= -100) /
  3 (zone < 0), reopened only for another map; no map0, Escape does nothing; illustrations take no keys. A negative
  key is the one under the player in that zone, else 0 *(inf)*. Without the arrow the view starts at the image base
  *(inf)*. 0x8a closes (fade-out; op 0xb4 0x15 waits through it). Op 0xb4 sub 0x14 (x, y, zoom factor, speed, flags,
  each x 0.001; FUN_101fb070 / FUN_101edee0): zoom = zf x base, view centred on (x, y) unclamped, t += ticks x speed x
  0.005 (mode 1 linear, 2 x (1 - t + 0.02), 3 x (t + 0.02)); speed 0 jumps; flag 0x80 shows at once; the keys, fade
  and closing stop.
- Live (Lnxcsfix, Southern San d'Oria): /map eases to the player in about 2 s, Escape fades out and returns. Keys
  not driven live (scripted keys are one-frame taps).
- Left: markers (mapv3 / mapv2, the .MRK file, sets 0..3), party dots (c2s 0x0D2 / s2c 0x0A0), Change Map, Wide
  Scan, event marker ops 0x8b / 0xb8 / 0xa8 / 0xb9 drawing, the no-key-item chat line (group 10 id 7).
- Markers (mapv3 / mapv2, spec 5 / 7): the .MRK file (client/mapmark.rs; USER/<id>/M%03d%02d.MRK, "KMIX", CRC-32 of
  the 0x1b8 data bytes, 2 sets x 10 records {x, y s16, name[16], valid u8, pad}; the old name M%03d%02d_%08d.MRK with
  magic @0, CRC @8, data @0x10 is read and removed on save) loads at open and saves on close when changed. map0's
  Markers opens mapv3 (rows: icon table 0x10382178 at (16, 5 + 16 i), name at (0x1f, 7 + 16 i); font 0x44 taken as
  fontshp *(inf)*); Left / Right step the set skipping empty ones (FUN_101fad70); Enter on set 0 opens mapv2 (View /
  Place-Remove / Rename; its place beside the row is inferred), set 1 scrolls to the marker, sets 2 / 3 show only that
  member's label and scroll. Place: cursor mode (axes 0x9d / 0x9e numpad and stick, speed 8, the cursor box, overflow
  scrolls), Enter puts the marker under the cursor and opens the 15-character name entry (Enter keeps, Escape
  discards). Markers of the shown set fade in 6 a tick while mapv3 is open, the rest fade out; labels (colour 2,
  0x606070 x alpha) at (+16, +1). Sets 2 / 3: party then alliance members here with a model on the same sub-map, from
  the actors (FUN_101f7db0); the c2s 0x0D2 / s2c 0x0A0 positions are not done. Mode 1 event maps open mapv3 on set 1.
- Event marker ops (FUN_100b9490 0x8b, FUN_100b9600 0xb8, FUN_100b9780 0xa8, FUN_100b9820 0xb9; DAT_104896e6 = an
  event map is open): with one open, FUN_100b2570(1, zone, key) reopens it if needed, the slot is set (FUN_101f8d50 at
  world x / z x 0.001 through the region) and scrolled to (FUN_101f8de0), named (FUN_101f8e30); without one, a mode-4
  load of the file, the change, then FUN_100b2a60 closes and saves. 0xa8 / 0xb9 sub 0 the same way, sub 1 only on the
  loaded data. 0x8b and 0xa8 pass key -1.
- Live (Lnxcsfix): /map, Markers, slot 1, Place, Enter, "Westgate": the marker drew and USER/5/M23000.MRK held it.

## Item info window: "menu    iteminfo" (2026-10-23)

Full spec: scratchpad iteminfo/SPEC.md of that session. CTkItemInfo DAT_10578204 (vtable 0x10333e20, registry flags
0x01400003): strings FUN_1013d790 and rows FUN_1013c500 rebuilt every frame (charge timers count live), draw
FUN_1013d760. Ours: client/iteminfo.rs (build / pages / draw), World::info_win, items::info_source.
- Openers: event 0x93 (FUN_100b9d30, item val@1, 0 closes) and 0xcc (FUN_100b9d90) subs 0 / 1 / 3 (item val@2; the
  shared buffer DAT_104886bc bytes 0..12 = val@4 / @6 / @8, the rest kept; sub 3 flags 2 = no signature) and sub 2 (a
  linkshell: zeroed, [6] = r | g << 4, [7] = b, [8] = status, [9..24] the string operand's 15 bytes). Mode 0 at (16, 240)
  stacked on the log like comyn. The item list (FUN_101f27a0) shows the slot under the cursor (mode 0); the equipment
  grid the piece in the cursor's slot and its candidate list the candidate (mode 1, (130, 240), not stacked). Enter in
  the list opens "menu    iuse", not this. Event start (menu broadcast 0x800) and end (0x1000) close what the ops opened.
- Layouts by rows 3..12 (table 0x1036fa70): iteminfo, item4inf .. item9inf, item10in .. item12in, 366 x (8 + 16 rows);
  FUN_1011a8f0 swaps the layout in place (ours: GameUi::window_swapped, one open animation under "menu    iteminfo").
- Header: icon 32 x 32 at (8, 12) with tint +0x28; the singular log name with its first letter upper-cased at (51, 4)
  (green 0x80509050 when equipped, yellow 0x80909040 in shop style); marks windowps 293 Aug / 294 Rare (flags 0x8000) /
  295 Ex (0x4000) / 296 Tmp (container 3) from (314, 4) +16 each, 842 Alt at -16 (flags 0x10 ...); line 2 at (51, 20):
  weapon skill (group 12 by FUN_1015aab0: skill 1..12 -> 0x18..0x23, 25 / 26 / 27 -> 0x24..0x26, 41 / 42 / 45 / 48 ->
  0x27..0x2a) or slot (first of 0x10 Head .. 0x100 Feet, #0xe..0x17), races (#0..0xd), "Su%d" (#0x1bc).
- Rows at y + 4 + 16 k: description lines (pre-wrapped 316 px) at x + 51; jobs "Lv.%d " (group 10 #0x53) + "All Jobs"
  (group 9 #0xe2, mask 0x7ffffe) or "WAR/MNK/..." wrapped at 309 px after " \t,/"; the extra line (charges "<11/11
  0:05/[0:05, 15:00]>" blue 0x80407090 full / orange 0x80804020 empty; fish "%d[Im]/%d[Pz]"; linkshell name; furnishing
  "%s STG:%d"), signature "[name]" (6-bit, ext +0xc) and "<Item Level:%d>" (slot 0x1c #0x3e) right-aligned at x + 360;
  with more pages the last line moves to x + 340 and "▼" (81 A5) sits at x + 360. Key function 0x18 (Num- / -) turns the
  page; past the last, back to 0.
- Linkshell tint (FUN_101cbac0 / FUN_101cbb80): ARGB 0x80RRGGBB with R = [6] low nibble, G = [6] high, B = [7] low,
  each x 13 + 0x38 (the spec's "0x80BBGGRR" was wrong; the decompile puts [6] & 0xf at bits 16..23). Name 6-bit
  MSB-first from [9] (1..0x1a 'a'..'z', 0x1b..0x34 'A'..'Z', 0x3e ' ', 0x3f end).
- s2c 0x020's Attr (24 bytes @0x11) is kept per slot (Inventory::ext).
- Not done: augment decoding (FUN_100ff140 / FUN_1013b280: the Aug lines and pages), item types 0xf / 0x11..0x15 /
  0x1a, the itemxinf / itemyinf compare windows, the de / fr layout and ja rows.
- Live (Lnxcsfix): the inventory list shows "Hume tunic", [Body] H♂, DEF:2, Lv.1 All Jobs with Rare / Ex.
- Change Map (maplist, build FUN_101fcae0, message FUN_101fcd90): region groups FUN_1020a400 (50), FUN_1020a3e0
  (zone lists, 6 u16 per 12-byte row, ended by 0; 0x103858a8 in this build) and FUN_1020a3f0 (group 0x13 name ids,
  the 50 bytes right after the pointer table). FFXiMain's code is packed in the install, so ours finds the data: the
  San d'Oria / Bastok lists as the hint, the pointer naming them, the names 0xc8 on. Rows: top = "Current Area" (group 9
  0x23, column 1 group 9 0x24 in small text; zone = ours, key -1) then each group with zones holding an owned key-item
  map (FUN_101f7340(zone, -2, 1): the zone's first record with a map number (+3), its key item held; maps needing none
  are not listed) and a place name, with the count; a group: each such zone with its area name (FUN_101cbc10, group 4),
  id -zone - 2 when two or more of its floors are owned; a zone: each owned floor with its map number. Columns (table
  0x10382188): 20 / 245 / 320 / 380; cell types (FUN_101f4bb0): 1 game font at the column, 9 "%d" right-aligned in
  the sprite font at column end - 2 (ours: game font, inf), 0x21 small text at y + 2. The cursor row's first cell is
  0x80803040. Enter descends or picks; Escape goes up (a floor list back to where it came from), closing from the top.
  The pick (FUN_101f7d80) closes the map; the close callback reopens it in mode 5 (FUN_101f79e0: fit zoom, the arrow
  only in our zone, map0 again).
- Live: Lnxcsfix's list read San d'Oria 4, Ronfaure 2, Zulkheim 3, Norvallen 1; Current Area reopened the map.
- Party positions (FUN_101f9070 / FUN_101f9110): when the Markers list opens on, or steps to, set 2 / 3, c2s 0x0D2
  {u32 zone} once per map (+0xa45); s2c 0x0A0 {id u32 @4, zone s16 @8, x / y / z f32 @0xc / 0x10 / 0x14} places that
  member (skipping ourselves; party or alliance by the group bits) when the sub-key under the position matches the
  map, replacing the actor-based position. Not seen live (needs party members out of model range).
- Op 0xd4 option entries (FUN_100b2290 subs 1 / 3 / 4 / 5, 6,445 sites; FUN_1014e070): 32 bytes for option val@2
  (< 0x20) at select window +0x13c + 0x20 i, written before the choice opens: sub 1 {u16 val@4, u16 val@6, rest
  0xff} (the map preview LAB_101f9510 reads them); sub 3 {u16 item val@4, byte 2 = 0}; 4 / 5 {u16 item val@4, byte 2 =
  1, byte 3 = 0 / 2, u32 val@6 / @8 / @10 = extdata words}. Sub 2's choice installs LAB_1013e6a0 as the window's
  +0x53c callback, called every frame with the cursor option's entry (FUN_1014e0a0: option item +0x104 - 1): item
  0xffff closes the item info window, else FUN_1013e6d0 -> FUN_1013e4b0(item, byte 2 ? entry + 4 : 0, jump = byte 3)
  opens it in mode 4 ((16, 16), stacked on region 3, taken as the choice window *(inf)*). An answer
  (FUN_100b2940 / FUN_100b2aa0) closes the item info window whoever opened it. Guild point and reward menus use it
  (Southern San d'Oria 0x2b2: sub 3 entries #3..#11 with item ids). Ours: State::option_items / item_preview,
  event::option_item. Not seen live.
- Op 0xd8 (FUN_100bc060, 118 sites, audit R20): actor@2 event body rotation = val x 2 pi / 4096 (constants
  0x10329d2c 6.283, 0x10330250 1/4096): sub 1 x (+0x150, len 8), 2 y = the direction (+0x154, len 8), 3 z (+0x158,
  len 8), 4 all three (val@6 / @8 / @10, len 12); sub 0 copies the actor rotation +0x34..+0x40 (len 6). With no body
  it advances 6 whatever the sub. Scripts tilt NPCs up to ~94 degrees (sub 3 #-1074). Ours: Body::tilt, Pose::tilt,
  Place::tilt, tilt_matrix in the actor frame (z then x, signs and order inferred; not seen live).
- Map left-drag (FUN_10122e30 n 1 / 2 with mouse state 4: FUN_10126100 / FUN_10125fe0): the offset from the anchor
  over W x 0.047619 gives -1 / 0 / +1 per axis in place of the scroll keys; ours anchors at the press point *(inf)*
  and keeps the camera still while the map is open (context 0x26 has no camera functions). Not driven live.

## Item use menu: "menu    iuse", Use and Drop (2026-10-23)

Full spec: scratchpad iuse/SPEC.md of that session (sort windows, manual sort, the direct-use list are in it, not done).
- Enter on the item list (FUN_101f2bb0) opens "menu    iuse" ((384, 48) 112 x 42, 1 Use windowps 129 / dim 413, 2 Drop
  123 / dim 414, caption comwin 25 "Item"; title group 0xd 2 "Items", help 0xe 0x1a / 0x110) for the slot under the
  cursor, SE 0002; nothing for lock byte 0x19. It closes when the list cursor leaves that slot (FUN_101f2ec0).
- Enabled (FUN_101f2de0 / FUN_101f2ec0): Use for crystals (type 8), maze tabulae (0x16 / 0x17), non-equipment with
  record flag 0x400 (the client's test; POLUtils names 0x200 "CanUse"), equipped charged gear whose extdata (kind 1)
  has u16 @2 bit 0x4000; never as a monster (main job 0x17). Drop when the slot's lock byte (s2c 0x01F @0xc, 0x020
  @0x10: 5 equipped, 0x0F in use, 0x13 linkshell, 0x19, 0x1B mannequin) is 0. Enter on a disabled child: SE 0004.
- Use (FUN_101f33a0 case 1): the status gate (0x7f; crystals 0x77; the Mog Garden 0x80 / flag 0x08 rule not done),
  then the target prompt with the record's targets (0x7f without one), title "Items" (0xd 2), help 0xe 4 "Select
  target to use item on."; the list and iteminfo hide meanwhile (FUN_101f2740) and come back after. The sender
  FUN_101f0260 (also /item's end): no charges and a non-monster NPC -> c2s 0x036 trade (inventory only, within 6
  yalms else message std 9); charged gear must be equipped and ready; cast bar = use time (usable items record +0xe =
  extra 0xd08, gear the cast byte) x 20 ticks; c2s 0x037 {target u32, 0, ActIndex u16, slot u8, pad, container u32}.
- Drop: "menu    sortyn" ((384, 124), Yes / No default No, caption comwin 16; help slot 1 group 0xc 0x3e "Dispose of
  item?", title slot 2 "Items"), iuse stays under it. Yes: Ex / Rare (flags 0xc000), augmented (extdata kind 2 / 3) or
  the Linkshell (0x201) ask in comyn first (group 0xa 0x30 "Really dispose of the %s?"; the Linkshell then group 9
  0x157 and 0x158); then c2s 0x028 {count = the whole stack u32, container u8, index u8} (no quantity prompt).
- s2c 0x009 (LAB_100fbd40): MesNo @0xa, parameters as "Para0 %u Para1 %u" text in Data @0xd -> the message-std
  expander (ours: SystemMessage).
- Grammar codes in FUN_100bc8a0 (English): items 0x20..0x22 take the index as the id, 0x23..0x25 the table +0xc64,
  0x26..0x28 +0xc50 (ours: both the work values *(inf)*); 0x29 / 0x2a take a count index then an item index: "a
  <item>" (vtable +4) for one, the plural (+0xc) otherwise. A count of 1 (codes 3 / 4) prints nothing and goes to
  FUN_1024fcd0(0); ours also drops the space after it *(inf)*. Message 180 now reads "You throw away a potion.".
- Live (Lnxcsfix, !additem 4112 2): iuse showed Use / Drop; Use -> prompt -> the server's "Unable to use item." (the
  potion script refuses at full HP); Drop -> sortyn -> Yes threw the stack away.
- Wide Scan and tracking (full spec: scratchpad widescan/SPEC.md of that session; corrects the map spec's 7.6):
  - map0 item 2 is enabled (FUN_101f7c00, at open) when s2c 0x119 was seen (DAT_10485824 bit 0; ours recasts_seen)
    and the map is our own zone; disabled it draws keytops3 20 and Enter plays SE 0004. LSB lets every job scan
    (map.ALL_JOBS_WIDESCAN).
  - Store (session object, zeroed on every map server connect, so zoning loses results and tracking): s2c 0x0F4 {index
    u16 @4, level u8 @6, type @7 & 7 (0 PC, 1 NPC, 2 mob), dx / dz s16 @8 / @0xa from the player's world x / z at the
    scan, name[16] @0xc (PCs only)} into record[index] (< 0x900); s2c 0x0F5 {x, y, z f32 @4.., level @0x10, index u16
    @0x12, state u32 @0x14}: state 1 for the tracked index updates it, another state for it or index 0 clears tracking;
    s2c 0x0F6 {state u32 @4}: 2 = list end (scan done, results kept). Tracking counts once state 1 with a level came.
    c2s 0x0F4 {1} (but 0x0F6 while the last list is still open), 0x0F5 {ActIndex}, 0x0F6 {0}, 8-byte packets.
  - scanlist (16, 48) 112 x 248, 10 rows of 24, cursor anc_trac, title 0xd 78 "Wide Scan": one scan per map opening
    (results kept within it), origin = the player's world x / z; rows and markers rebuilt every frame in index order:
    PCs (not ourselves) with their packet name (markblue, keytops3 363, untrackable), NPCs (markgree 326) and mobs
    (markred 327) named from the zone NPC list by index & 0x3ff, empty names dropped; markers not centred, the cursor
    row's as the ring keytops3 105; names fitted like the target window's (88 px at +7, +6). Enter scrolls the map to
    the row and opens mapscan; Escape closes the list.
  - mapscan: "Track target." (c2s 0x0F5; map0, scanlist and the map close; untrackable -> SE 0004) and "Narrow down
    range ..." (scanlist hidden, cursor mode with the anc_scan cursor; mapframe Enter: centre = the map pixel under the
    cursor, radius = ftol(128 x 0.65 / zoom + 0.5), rows outside dropped; Escape: unfiltered). No packet for narrowing.
  - map0 Wide Scan while tracking: comyn group 0xe 0x45 (default No); Yes sends 0x0F6, drops the ring and opens the
    list. The tracked ring (keytops3 105) is taken at map open only.
  - Compass (FUN_102203b0): while tracking, the kaipage arrow on the rim toward the target (radius 28.8 x 0.8, 36 x 0.8
    with NIN main / sub, scale 2k, the disk's tilt), colour 0x802080f0, bottom centre on the point; the name at (cx +
    36, cy + 26). Tags: `<scan>` = the tracked name in text, the tracked actor as a command target.
  - The busy strip, the untracked pointer, `<ht>` and paging: see "Wide Scan leftovers" below.
  - Live (Lnxcsfix, Southern San d'Oria): the list filled with NPCs, Track on Ephauge closed the map, the compass
    arrow and name showed, `/echo tracking <scan>` printed "tracking Ephauge".
- Augments (full spec with every table, handler and the 23 test vectors: scratchpad augment/SPEC.md of that session;
  ours client/augment.rs, a port of its reference):
  - Gate: weapons / armour, ids not 0xffff / 0 / 0x200..0x203 / 0x108c / 0x108d / 0x1525 / 0x1526 (FUN_100fedc0),
    extdata kind 2 (FUN_100fee70) or 3 (FUN_100feef0); only ext bytes 0..11 are read. Kind 0 or anything else shows
    nothing, not even charges: the charge line needs no extdata (an event's item) or kind 1.
  - Aug mark (FUN_101cb8a0 / FUN_101cb990): not for the 50 weapons of the no-mark list (0x1037d378) nor a serialized
    item (kind 2, ext[1] & 0x10); ext[1] & 2 adds the Ex mark (LSB sets it on every standard augment).
  - Forms (FUN_100ff140), in order: item id < 0x2800 evolith stone; kind 2 with ext[1] & 0x80 evolith slots, & 0x10
    serialized ("%s No. %05d", server names group 0x16), & 0x20 mezzotint (Type / Rank / RP), & 0x08 escutcheon
    (craftsmanship), else classic (5 x {11-bit id, 5-bit value}, 4 plus a trial word with & 0x40: "[Trial %d]" until bit
    15); kind 3 with ext[1] & 0x80 bundle (file 81's "mnc2" chunk: RP curves, 1024 bundles, 1996 records).
  - Classic: FUN_100ff9c0 folds the augments into a stats record (handler table 0x10362538 for ids 0..0x7f, ranges
    above), FUN_100ff2f0 prints it in a fixed order with a packer of 5 units a line (Rng. Acc. / Rng. Atk. / Mag. Acc.
    3, specials a line each). Texts: file slot 0x1b (55692) through sprintf with the value; "{FIR}".."{DKN}" become
    EF 1F..EF 26. The block (+0x1b8, 0xbf bytes) groups bundles / mezzotints / evolith slots as "[n]"; the short line
    goes to "<...>" (0x80407090 at the top rank).
  - Rows: augment lines never on page 0 (English); page 1 shows the rest of the description, else the augment lines to
    row 10 (the rest dropped); page 2 only after a description page, with all augment lines. Pages 1 / 2 end with the
    item level line's 0x48 appended, else 0x50 on the last blank row. FUN_101ca520 / FUN_101ca570 wrap after the last
    of " \t,/" keeping it, no trimming (job lines too).
  - Data tables found by hint + MD5 in the packed install DLL (attribute / pet / element masks, prefix, evolith,
    mezzotint index and RP, no-mark ids); the nearly empty 0x280 / 0x480 tables and the shape glyphs as constants.
  - Not seen live (no augmented gear on the test characters); offscreen Kupofried's Ring with vector 5 matches.
- The direct-use list (action menu Items, FUN_1012b880 case 8 / FUN_101f0a10): the inventory list with only the rows
  FUN_101f0810 colours (crystals, maze tabulae, usable non-equipment with flag 0x400, equipped charged gear), no iuse
  and no sort window; Enter uses at once (the same per-type branches and target prompt as iuse Use), SE 0002. Ours:
  Win::Items direct; the row tints are still the main list ones (the blue / grey for gear is not done). Not seen live.

## Item sort window: "menu    itmsort2", Auto and Manual (2026-10-23)

- Item sort window ("menu    itmsort2", FUN_101f0600 / FUN_101f36d0): drawn beside the main inventory list (not the
  direct-use list); key function 0x11 (Num+) moves the focus there and back (FUN_101f3740, SE 0013). Rows: 1 Auto,
  2 Manual, 3 Recycle Bin (not done here).
  - The list has a view order per container, separate from the slot indexes (FUN_1021c210 / FUN_1021c5b0 append a
    newly filled slot, FUN_1021c680 drops an emptied one). Ours: Inventory.order, Inventory::viewed.
  - Auto (FUN_1021c7e0): sortyn "Sort items?" (group 0xc 0x3d); Yes reorders the view by the item record key at +0xc
    and then the id, or with config 0xca == 1 pulls identical ids up beside their first row; then c2s 0x03A
    {u32 container} and the server merges stacks. Seen live: Distilled Water 3 + 2 merged to 5.
  - Manual (FUN_101f0b30): help group 0xe 5 "Select item to move." / 6 with one picked. The second pick moves the
    first row to the second row's place, shifting the rows between (FUN_1021cac0, client side only; not a swap,
    although the help text says "switch"). The same row splits one off with c2s 0x029 {1, inv, inv, index, 0x52} when
    the stack holds more than one and stacks (0x52 as the target index is inferred to mean the first free slot).
    Another row of the same item takes the whole stack (c2s 0x029). Seen live (the move; the split was not tried).
- Test drives: act:numpad+ taps Num+.

## Equipment compare: "menu    itemxinf" / "menu    itemyinf" (2026-10-23)

- An equipment-menu feature (the candidate list's per-frame hook FUN_10130a80, class next to the equip window
  DAT_105781a8). With the equip window in candidate mode (+0x1c == 1), a rising edge of key function 0x76 (Ctrl held)
  or 0x89 (pad) while no compare is open: FUN_101317b0(item, turn & 1) gives the first or second set bit of the
  focused item's slot bits (+0x14, table 0x1036f734 maps bit to grid child); turn += 1 (2-bit counter, two misses clear
  it: FUN_10130be0). The grid cursor moves to that slot (FUN_10131810), and the worn piece there (FUN_10131770) goes
  to list+0x8c with +0x89 = 1. So rings and earrings alternate between their two slots on each new open.
- The list draw (FUN_101f2740 area) then opens itemxinf (DAT_10578208, a subclass of the iteminfo object) by
  FUN_1013e410 mode 5 with that item, or with item 0 / container 0x12 when the slot is empty (an empty frame,
  inferred from the code path; not seen). While it is open, both windows take the larger row count (+0x381 = max of
  FUN_1013cb60, +0x382 = 1); layouts "itemxinf", "itemx4in" .. "itemx9in", "itemx10i" .. "itemx12i" (366 wide, the
  same heights as the iteminfo set).
- Placement (FUN_1013ad50): itemyinf (366x20, keytops3 sprite 549: a yellow chevron) right above iteminfo, itemxinf
  above itemyinf, both at iteminfo's x.
- itemxinf's own update (FUN_1013ac50): the next rising Ctrl edge closes it (states 1 then 2, FUN_1015e790). Its
  open (FUN_1013aba0) primes the edge with the current Ctrl state, so the opening press does not close it.
- The candidate list's messages 1..6 and 0x1b (FUN_101306f0: cursor moves, select, cancel) clear the compare.
- Ours: Pick.compare / alt / ctrl, items::compare_key and items::compare, drawn in draw_item_info. The compare shows
  its first page only *(inferred: each window keeps its own page state in retail; ours does not page it)*.
- Live (Lnxcsfix, Main slot): the Onion Sword candidate with the worn Onion Sword above it and the chevron between;
  a second Ctrl closes it.
- Test drives: `act:down:1d` / `act:up:1d` hold Ctrl.

## Event ops 0x7e (stance / mount) and 0xab 9 / 0xa (2026-10-23)

- 0x7e (FUN_100b8b80) subs 0 / 5 / 8 clear actor@2's event status (+0x174 = 0, and 8 also clears the mount id
  +0x1db). Ours removes the event status the actor poses with. Subs 1 / 3 / 6 / 7 mount the actor (status 5 or
  0x55, FUN_10097a20 spawns the chocobo / mount): not done, as for op 0xac's 5 / 0x55.
- 0xab 9 / 0xa set / clear DAT_10487ef5. Its only reader, FUN_10002d80, returns the effect-time multiplier: 1.0
  (_DAT_1032961c) while it is set, else _DAT_10456a68, a config value (setter FUN_10002d70, called from the config
  switch at 0x101aa703). Our effects always step at 1.0, so these subs change nothing here.

- 0xb6 0xd / 0xe (FUN_100b1150): the event actor becomes a chocobo: race +0xef = val@4 + 0x20 (our NPC race
  tables 32..36 are the chocobos), body +0x100 = val@6, head +0xfe = val@8, feet +0x106 = val@10, hands +0x102 =
  val@12, legs +0x104 = val@14 (0xe) or 0; face, main, sub and range cleared; +0x120 bit 30 = val@2 in {3, 4, 8}
  (meaning not traced). Sites take their values from Z registers (e.g. Bastok Mines actor 0x010ea097), i.e. the
  server: not seen live.

- Zoning: LSB re-sends every container after c2s 0x00C (charutils::SendInventory from 0x00C gameok, equipment
  as 0x050 too) and empties the temporary items and the recycle bin at zone-in, so ours resets the inventory
  (but the container sizes) at ZoneIn; that retail resets its copy on connect is inferred. The view order resets
  with it.
- The direct-use list colours (FUN_101f0810): usable items 0x80909040, equipped charged gear 0x80407090 when ready
  (FUN_100febe0: extdata kind 1, u16 @2 bit 0x4000) else 0x80404040.

## Wide Scan leftovers: the busy strip, the call-for-help pointer, `<ht>`, scanlist paging (2026-10-23)

- helpwind busy strip (text object at window +0x20, bits +0x64; set FUN_10221270, clear FUN_10221300, draw
  FUN_102214d0, tick FUN_102213e0): while any bit is set the help text and its marquee are replaced by keytops3
  165..169 (table 0x10385fcc, 8 ticks a frame, looping; each set restarts at 165), centred in the text box (x = box
  centre - half the widest frame - the frame's x0, y = y0 + 8), 0x80808080, unfaded. It is the "Downloading data ..."
  strip. FUN_10221740 counts a busy bar as active (the bars stay up; inferred from the caller testing AL).
  - Bits: 0x80 Wide Scan (set by FUN_101fb7d0 after c2s 0x0F4 or the 0x0F6 sent in its place; cleared by the 0x0F6
    list end, a 30 s timer, scanlist closing), 4 party positions (c2s 0x0D2; cleared by every s2c 0x0A0 and the map
    closing), 0x40 "menu    level" after c2s 0x061, 2 after c2s 0x05A (inf: region info), 0x10 message windows, 8 /
    1 list windows (not identified). Ours: 0x80 and 4 (Scan::busy / party_busy). +0x68 / +0x6c are separate
    right-aligned indicators from session flags (not done).
- FUN_1021f030 (the compass's untracked pointer and `<ht>`): over all actors, those with a model (+0xa0), the monster
  flag (+0x120 bit 13), the call-for-help flag (+0x127 bit 0: 0x00E u32 @0x20 bit 13, our marks.yell) and not dead
  (+0x170 not 2 / 3); the nearest by horizontal distance (first match, then strictly nearer); 0.0 = none. No range,
  claim or party test.
  - Compass (FUN_102203b0 tail), only when not tracking: the same kaipage arrow as tracking in 0x80802020, the name
    (zone NPC list by index) at (cx + 36, cy + 26) in ncol entry 8; nothing when none qualifies. Both arrows and names
    use fixed colours, not the disk's open alpha (ours faded them before).
  - `<ht>`: in text (FUN_10076ff0 type 0x16, where `<t>` expands) the NPC-list name; as a command target
    (FUN_1007a1c0) that actor, else the generic unresolved-tag path.
- scanlist paging (list base FUN_101f6c70 messages 3 / 4; the base sets +0x47 = 1, so plain Right / Left page): a
  page of 10 with the cursor row kept; at the last page Right puts the cursor on the last row, at the first page Left on
  the first; SE 0001 when something moved.
- Done later: mapscan's disabled look for untrackable rows (keytops3 300 at 0x30808080, with the cursor 0x40402810 at
  (+1, +1); FUN_101177d0's rule) and map0 child 2's cursor highlight (keytops3 20 at (+1, +1) in 0x80805020).
- Checked offscreen: `topbar-shot --window busy` draws the strip. Not seen live: the call-for-help pointer, `<ht>`,
  the paging (paging and the strip seen later, see Recycle Bin).

## Recycle Bin (2026-10-23; spec: scratchpad recycle/SPEC.md of that session)

- Not its own window: the two-list storage view with the storage side on container 17. itmsort2 row 3 and
  `/recycle` (id 0x7a, FUN_1007e420) call FUN_101daf70(0x11): status gate 0x44 (synthesis; costume not tracked
  here), no zone or Mog House rule; "menu    bank" (FUN_101dc880 / open hook FUN_101dc700) at its layout (16, 48),
  titled group 0xd 0xc5 "Recycling Bin", rows help 0xe 0x42d; the inventory list moves to (200, 48), every row grey.
  List +0x90 means "opened from main menu Items" (itmsort2), not the bin.
- Bin row colours (FUN_101db220): grey with no free inventory slot or for a Rare item already held outside
  containers 3 / 17; orange for the row whose quantity is being set; else white.
- Enter (FUN_101db690 case 5): one item goes at once; a stack opens "menu    itemctrl" (16, 240), "%2d" / "/%2d"
  at (+0x34 / +0x3a, +0x16), Up / Down +-1, Right 1, Left all, Enter sends, Escape drops it; iteminfo moves to mode
  1 meanwhile. c2s 0x029 {n, 17, 0, slot, 0x52}. Grey rows and the inventory side: SE 0004. Right / Left switch
  the focused list (SE 0013). Escape: back to itmsort2 (FUN_101f2640), or closes the view after `/recycle`.
- LSB: drops (0x028) put the whole item in the bin, the bin holds 10 and evicts slot 1 when full, it is emptied at
  every zone-in and login; from 17 only to the inventory or the Mog Locker.
- itmsortw / itmstora belong to the storage views (bank, Mog Safe, wardrobes ...), not done.
- Live (Lnxcsfix): dropped Distilled Water x5 and a Potion; the bin listed both; 3 of 5 moved back through
  itemctrl; `/recycle` opened it; Escape returned to itmsort2.
- mapscan / map0 disabled looks (FUN_101177d0): Track for an untrackable row keytops3 300 at 0x30808080, under the
  cursor 0x40402810 at (+1, +1); map0 Wide Scan disabled under the cursor keytops3 20 at (+1, +1) in 0x80805020.
  Live: the busy strip while scanning, and scanlist paging (Right, Right, Left) in Southern San d\x27Oria.

- Item list paging (list base FUN_101f6c70 messages 3 / 4): a list pages with plain Right / Left when +0x47 is set,
  else only with key function 0xa8 (Ctrl / Alt held). +0x47 comes from the source vtable +0x1c: the main inventory
  sources FUN_1012e470 = 1, the storage view sources (bank and its inventory partner) FUN_101dc590 = 0 (there plain
  Right / Left switch lists). A page keeps the cursor row; on the last / first page the cursor goes to the last /
  first row; SE 0001 when something moved. Ours: page_keys for the main list and Manual, Ctrl / Alt in the bin
  view (egui modifiers taken as 0xa8). Live: a 10-row list (one page) moved to the last and first rows.

- Config 0xab default (FUN_10195460 call after 0xa8): FUN_100156a0(width, height) with the size from DAT_104568fc
  +0x10 / +0x12 (FUN_10015600 / FUN_10015610; taken as the desktop, inferred): the index of table 0x1034f4a0 (4:3,
  15:9, 15:10, 16:9, 16:10, 5:4 as float pairs) with the smallest |a - r| / r, -1 for a zero size. Ours:
  gamecfg::set_desktop from the window\x27s monitor at start, used as 0xab\x27s default in get / set.

- Top-bar rule in 30261006_0 (asm 0x1015feb0, in the manager update with the window loop; was FUN_10159080):
  the same timer (180 / count 20, states 3 / 2 for the 0x8000 windows) in game state 0x60. Inactive needs all of:
  titlewin text empty (FUN_10221740 on 0x1057847c + 0xa4), titlewin +0x448 == 0 (HN mode), window DAT_10578480
  not open (FUN_10150d90; class vtable 0x103365d0, draw FUN_10150da0: two strings from FUN_100f1a60 /
  FUN_100f1ac0 at its corner; not identified), helpwind text empty (0x105781f8 + 0x20, busy bits count as text).
  Ours matches for the windows we have (titlewin / helpwind text, the busy strip); HN mode and that window are not
  modelled.

## Random names in the creator (2026-10-23; 30261006_0: FUN_10235310, FUN_10238330, trigger FUN_102384d0)

- Num+ (DIK 0x4e, FUN_10124070 press bit) on the name step sends the window message that asks for a name. Two raw
  DAT files of 16-byte NUL-padded lowercase words, grouped by length: 0x7c06 (31750, ROM/68/2.DAT) 1410 adjectives,
  0x7c07 (31751, ROM/68/3.DAT) 981 nouns, read asynchronously (FUN_101c4820, callbacks 0x102374a0 / 0x102374f0 set
  the done flags DAT_10668fdc / fe0; buffers DAT_10668f00 noun, DAT_10668ee8 adjective).
- Noun = record rand() % 981; its length class = the first index where the running sum of weights 0x103873f8 {2, 56,
  149, 171, 173, 169, 127, 81, 34, 17, 2} exceeds it; budget = 10 - class. r = rand() % (2^(budget+1) - 1);
  adjective class = 1 when r == 0, else ftol(ln r x 1/ln 2); adjective = rand() % counts[class] + offsets[class]
  (0x10387424 {21, 74, 153, 220, 264, 317, 190, 99, 51, 12, 9}, 0x10387450 {0, 21, 95, ..., 1401}).
- Name = adjective + noun when shorter than 16 letters (else truncated to 16, then dropped by the < 16 test), then
  FUN_101ba240 (a validity check, not traced; ours skips it) and into the text input, which capitalises it.
- Ours: creator::random_name with getrandom standing in for rand(). Live: "Attentivepond", "Lovelessharrier".

## Magic / ability sort windows and the order files (2026-10-23; spec: scratchpad magsort/SPEC.md of that session)

- Order files are USER files registered by FUN_101b9f20(version, ...): mix.dat version **2** (0x884: bits[0x80], u16
  order[0x400], u32 count; count >= 0x400 reads as all zero), aix.dat version **4** (0x1764: bits[0x160], u16
  order[0xb00], u32 count; count >= 0x700 zero), eix.dat / moix.dat version 1 (emote / mount order, not ours yet).
  Container as the macro files (u32 version, u32, MD5, payload). Our reader took only version 1, so it never read a
  retail mix.dat / aix.dat; read_file / write_file now take the version.
- Read once at field entry (FUN_101ba030, with the macro files), merged in place with the known bits on every list
  build (FUN_1021e550), written only when leaving the field (FUN_101ab7d0 -> FUN_101b9ff0; mix.dat not in
  Monstrosity). Ours: Macros.mix / Macros.aix (macros::Order), merged before saving on Zoning / Logout.
- Sort windows at (384, 48), shown unfocused beside the list: mgcsortw (Auto-sort, Manual Sort) for main menu
  Magic lists (mgcmenu, list mode: FUN_1022c3a0 tail), blusortw (with row 3 Blue Magic) for type 6; abisortw
  (Manual Sort only) for abimenu's Job Abilities / Weapon Skills, passed on to sub-lists and the Escape back to the
  parent. Cast-mode lists (action menu, magselec, abiselec) have none. Num+ (message 8): list (rows, not in Manual)
  to the sort window and back, SE 0013; Escape on the sort window back to the list.
- Auto-sort: sortyn 0xe 0x50 "Sort list?" (16 px lower under blusortw, inferred), Yes = FUN_1022c070: every known id
  by record +0x3e (SE's family order) with the other types zeroed, then the other types' ids refill the holes in
  their old relative order; count kept; no packet, no write. Manual Sort: Enter picks (state 2, the row in
  0x80804020), Enter on another row takes the picked id out and inserts it there (FUN_1022c8e0, not a swap); Escape
  drops the pick, then returns to the sort window. Help 0xe 8 / 9 (magic), 0xa / 0xb (abilities); sort rows 0x15a /
  0x3f, 0x10e / 0x40, blusortw row 3 0x283 / 0x8d (disabled look without 0x119 and BLU: keytops3 89 at 0x30808080,
  refused with group 7 0x57; the set screen is not done).
- Num+ also fires key function 0x78 (log focus cycle). FUN_10151550 skips it, with no focus yet, while a window
  flagged 0x20000 / 0x20000000 in the window registry (0x103722f8: 0x2c-byte entries, flags u32 @4, name @0xc) is
  open: mgcmenu 0x20001a00, abimenu 0x20201200, magselec, abiselec, sortyn 0x20001002, money, persona, iteminfo,
  the party windows, many others (inventor 0x10000, magic / ability 0x80002 are not). Ours skips it while a list,
  category window, item window or prompt is up (the item windows: inferred). Before, Num+ in the magic list put the
  focus on the log and the next Enter opened the full log.
- Not done: cursor memory per type (+0x74 / +0x98 / +0x70 tables), the mouse on the sort windows, emotes / mounts.
- Live (Lnxcsfix, White Magic after !addallspells): Auto put Cura after Curaga V and Raise later; Manual moved Cure
  two rows down (Cure II, Cure III, Cure).

## Storage views (2026-10-23; spec: scratchpad storage/SPEC.md of that session)

- Every container uses the bin's two-list view: "menu    bank" (16, 48) on the storage container, the inventory list
  at (200, 48), "menu    itmsortw" (384, 48) but for the bin. Text commands (table 0x10355408, handlers 0x10356e30,
  flags 0x51): /bank 0x6c, /locker 0x6d, /storage 0x6e -> FUN_101dae10 (containers 1, 4, 2); /satchel 0x6f, /sack,
  /case, /wardrobe 0x72, /wardrobe2..8 0x73..0x79, /recycle 0x7a -> FUN_101daf70 (5, 6, 7, 8, 10..16, 17); /bank2 0x7b
  checks Safe 2 (s2c 0x067 @0x27 bit 0) first, else group 7 0xa8 counted as success.
- FUN_101dae10: outside the Mog House a read-only list of the container (the item list at (16, 48), every row
  0x80404040, title in top-bar slot 2); the Locker without a lease (0x01C ItemNum2[4] == 0) read-only plus group 7
  0xb6; the moogle away (zone not in table 0x1035b680: 230-232, 234-236, 238-241, 243-246, 48, 50, 80, 87, 94, 256,
  257, 280) read-only plus 0x48; visiting another's Mog House 0xbc (not parsed); else the view at the moogle (bank
  +0x70). FUN_101daf70: gate 0x44; Satchel needs ItemNum[5] >= 30 (else a silent failure, the command error path);
  opens anywhere, at the moogle only in the Mog House of a residential zone.
- Row colours (FUN_101db220, first match): the bin's inventory side; destination full; bin rules; installed
  furniture in the Safe (grey on this floor, 0x80602020 on another); equipped (0, wardrobes); a wardrobe takes only
  records with level and races; no furniture into Storage; containers 1 / 2 / 4 / 9 away from the moogle; the picked
  row orange; else white. Not done: wardrobe 3..8 feature bits, mannequin-worn Storage items, the equipped linkshell
  (s2c 0x0E0).
- Enter works on both sides: from / to by side, a stack through itemctrl, c2s 0x029 {n, from, to, index, 0x52};
  the inventory side refuses lock 0x19. Right on the bank / Left on the inventory side move the focus with the
  visible row (SE 0013); Ctrl / Alt + Right / Left page. Titles by container (0xd: 3 Mog Safe, 0x25, 0x8b Locker,
  0x97, 0x9e, 0xa6, 0xa8, 0xb5, 0xb9.., 0xbf.., 0xc5), row help names the destination (0xe 0x42d..0x43c).
- itmsortw (Num+ from the focused side, SE 0013): Auto (sortyn 0xc 0x3d; c2s 0x03A {c} sent twice for a storage
  container, as retail) and Manual (FUN_101dbed0 = itmsort2's Manual with the container; equipped rows green
  0x80509050). Row 3 Switch and the Alt cycle: below.
- 0x01C ItemNum2 (u16 x 18 @0x24) is parsed now. Escape closes the view (the bin returns to itmsort2 when it came
  from there).
- Live (Lnxcsfix, Southern San d'Oria): /case moved 2 of 3 Distilled Water in through itemctrl and all back;
  /wardrobe greyed the non-gear rows; /bank read-only "Mog Safe"; /bank2 "That cannot be used right now."; /satchel
  "...A command error occurred.".
- itmstora (Switch, FUN_101f3e60): rows 1 Inventory, 2 Mog Safe, 3 Safe 2, 4 Storage, 5 Locker, 6 Satchel, 7 Sack,
  8 Case, 9 Wardrobe, 10..16 Wardrobe 2..8 (FUN_101f3f80); disabled (FUN_101f3c60, dim sprites of kind 4): both
  sides\x27 containers, 1 / 2 / 4 / 9 away from the moogle, the Locker without a lease, the Satchel under 30, Safe 2
  (not parsed), Sack and wardrobes 2..8 with size 0. Enter switches the side itmsortw serves and focuses that list;
  Escape / Num+ back to itmsortw. Alt+Right / Alt+Left (key functions 0xb5 / 0xb6) cycle the focused side through
  table 0x1037ef98 {0, 1, 9, 2, 4, 5, 6, 7, 8, 10..16} with the same skips (FUN_101dbc20); not for the bin. Ctrl and
  Alt are told apart now (Alt alone for the cycle; either for the 0xa8 page). Up / Down in itmstora skip the
  disabled rows, and it opens on the first enabled one (both inferred).
- Live: Switch from Mog Case to Mog Wardrobe, Alt+Right to M.Ward. 2 and Alt+Left back.

## Party-list cursor: `<stpt>` / `<stal>` (2026-10-23; spec: scratchpad stpt/SPEC.md of that session)

- FUN_10076ab0 modes 6 / 7 call FUN_10202d30(LAB_10077a20, .., 2 / 3), not the 3D prompt: a cursor on the party
  window's slots (DAT_106643e0) below a count of 6 (`<stpt>`) or 18 (`<stal>`, through raid1 / raid2), no candidate
  filter, the world target and lock untouched, nothing refused (slot 0 = us always exists). Opens on the current
  target's slot when below the count, else on us; then one preview run of the line gives the hand its range.
- Keys (FUN_102036f0 via the window events; its window is raised and takes them): Down / Up the next / previous used
  slot, wrapping (FUN_10203570 / FUN_10203630; the fellow row -1 between is not modelled), SE 0001; Left / Right
  nothing; Enter SE 0002, the line with the slot's id (or quoted roster name for flag 0x08), then closed; Escape SE
  0003, a macro line stops the macro. F-keys / Tab still change the world target (the lock is bypassed). A slot that
  empties steps the cursor back (FUN_10203c80).
- The hand (FUN_10202110 -> FUN_102022b0): the window's kind-6 cursor ref `anc     anc2s` (6 frames, half size) at the
  row child's top-left; tint from base 0xa04040d0 by the range (red 0xe0d04040 past range + 0.5 or at 25, yellow
  0xe0d0d040 past range - 0.5), the pick flashing unless red. With no cursor, the current target's slot gets the hand
  in 0x80808080 (so targeting a party member, or a 3D prompt's pick, marks their row). No second world arrow.
- "Sub-target up" (T+0x59 || T+0xc4) gates: typed lines need flag 0x02, macros wait, Ctrl / Alt list shortcuts, the
  lock bypass. Ours: target::sub_up.
- Not done: the hand on raid1 / raid2 rows (we do not draw those windows yet), the fellow row, the mouse.
- Live (Lnxcsfix, solo): F1 put the white hand on our row; `/ta <stpt>` + Enter targeted us (0x5); `/ma Cure <stpt>`
  showed the blue hand and sent `/ma Cure 5`.

- List cursor memory (FUN_10221b90; magic +0x74 / +0x98 + 4 x type for list / cast mode, abilities +0x70 + 4 x type,
  written by the close hooks FUN_1022c990 / FUN_102243d0): the list reopens on the remembered row (scrolled into
  view: inferred). Ours records it as the cursor moves. Live: Ctrl+M, three rows down, Escape, Ctrl+M: back on the
  fourth row; the Manual order from the earlier session came back from mix.dat (written at logout).

- The item count (FUN_101cb590, from the main list\x27s update FUN_101f2740 tail and the bank\x27s FUN_101dcb40): while an
  item list is focused, titlewin\x27s right edge shows "items/size": "/" at (x1 - 0x16, y0 + 8), the count
  right-aligned at (x1 - 0x13, y0 + 4), the size (ItemNum - 1) right-aligned at (x1 - 1, y1 - 0x10), 0x80808080
  (Japanese: x1 - 0x1c / 0x1e / 6). Retail takes temporary items and a few table counts off the main list\x27s count
  (not traced); ours counts the rows. Live: "11/30" in Items, "0/80" in the Mog Case.

- s2c 0x067 kind 2 (@4 = 2, char sync; LSB packets/char_sync.cpp): @8 the character id, @0x20 the owner of the Mog
  House being visited (own client only; 0 = none / our own: FUN_100f9600 / FUN_101cc0a0), @0x24 open to visitors,
  @0x25 main job level, @0x27 bit 0 the 2F / Safe 2 flag (the owner\x27s when visiting: FUN_100f97a0). Parsed now
  (World.mh_owner / mh_floors): /bank2 opens Safe 2 when set (else group 7 0xa8), itmstora / the Alt cycle offer
  Safe 2, FUN_101dae10 refuses with 0xbc while visiting, and the Mog House door menu takes its 2F and owner tests
  from it (myroom::OWNER and the "2F off" placeholder are gone).

- Storage colour rule 4 now: the lobby reply\x27s feature bits (u32 @0x24, World.features) 0x04 << (c - 0xb) gate
  Wardrobe 3..8; without the bit every row of that view is grey. Live: /wardrobe3 with LSB defaults left a dagger
  white (gear) and the rest grey.

- The window registry (records 0x2c apart from "menu    menuwind", flags u32 @0x24) was keyed with retail\x27s space
  padding, so a lookup by a short layout name ("menu    raid1") missed and read flags 0. Keys and lookups are
  normalised now (trimmed, lower case). No visible change yet: the short names with flag 8 (no open zoom: inline,
  fep, cnttime, quest01, guide01) are not drawn through the gate.

## Alliance windows: "menu    raid1" / "menu    raid2" (2026-10-23; spec: scratchpad raid/SPEC.md of that session)

- Handlers DAT_106643ac / DAT_106643b0 (vtables 0x1033b028 / 0x1033afd8), draws FUN_10204660 / FUN_10204770, rows
  FUN_10202720 (2019: FUN_101f9750). raid1 = window slots 6..11 (the first other party met in roster order), raid2
  = 12..17 (FUN_10202b80, as our party::Slots). Layouts 112x100, places (384, 48) / (384, 150) anchored right and
  bottom: (W - 128, H - 400) / (W - 128, H - 298); six row children at (3, 4 + 16 (k - 1)), 106x14; frame framesus
  16 (only the style frame shows). Always six rows (no layout swap); empty rows blank. Registry flags 3 (they zoom
  open; no stacking flags).
- FUN_10203c80: shown while their party exists and none of the windows with registry word +0x2a bit 0x01 / 0x80 is
  up (main / social menu, magic / ability lists, map, config, party menus, auction, search, ...), nor the equipment
  window, the macro palettes, an event holding the player (FUN_10087070(1)) or text-input +0xf100 (inf: the
  auto-translate list); a party-menu member pick keeps them. Closing them turns the party-list cursor off without its
  callback. Ours maps that to world.menu / win / config / map / magic / ability / palette / event (inferred).
- Rows (FUN_10202720): in zone the HP gauge (windowps 38 / 97, eased) at (+26, +7), the name (fontshp, 8-character
  ellipsis) at the row, HP right-aligned (+88, +1) in the HP colour, MP right-aligned (+104, +8); TP (+48, +10) with
  config 0xae, not for trusts; no MP gauge. Out of zone the name alone in 0x80606060 (no "(zone)" text, unlike the
  party rows). Marks as on the party rows (FUN_10201ff0; the quartermaster 433 / level-sync 493 marks are not done
  on either). The hand (anc2s) on these rows as on the party window: passive grey for an alliance-member target,
  range-tinted under `<stal>`.
- Not done: the lot column and the lnowin shift (FUN_10202440, FUN_1013f1c0: no loot window yet). Not testable live
  with lnxtest01 (three characters, one session); checked offscreen: `FFXI_UISHOT_MEMBERS=6 FFXI_UISHOT_ALLIANCE=6,4
  ui-shot`.

- Refusal lines (table 0x103763b0, 4-byte entries {group 7 id, 3, 0, mode}, indices 0..0x23, printed by
  FUN_10129d90(index)): the target failures are indices, so the magic list\x27s "no target" is 0x16 -> group 7 0x5a
  "You must specify a valid target for that spell." and the ability / ranged one 0x1b -> 0x5d (ours printed group 7
  0x16 / 0x1b, lobby errors). Chat mode per line: 0x7a for 0x56..0x5f, 0xb5, 0xb7, 0xbd, 0xbf; 0x7b for the status
  lines and the rest (textcmd::refusal_mode, both refuse helpers).
- subwindo modes 2 / 10 (FUN_1014fc20): "Next" sits on the second line (+0x1a) with the first empty.

## The mount list, /mount, /dismount (2026-10-23; spec: scratchpad mount/SPEC.md of that session)

- "menu    mount" (list object DAT_10667170, FUN_10226490(1, sortable)): abimenu row 6 attaches abisortw, abiselec
  row 5 does not; subwindo in mode 10 (only "Next" on its second line, the shared mount recast). Rows: the owned
  mounts (s2c 0x0AE: 64 bits @4, LSB key items 3072 + n; none before the first 0x0AE) in moix.dat order (USER file,
  version 1, u16[0x40], identity by default; written with the other USER files on leaving the field). Names / icons
  from d_msg 55681 (en; ja 55561, fr 55801, de 56221: field 0 the name, field 1 the magico2w icon 86 / 87),
  descriptions 55682. Empty: group 9 0x4f "No mounts" / 0x50 "available." on two lines.
- Refusals (FUN_10226d80): fishing 0x4d, mounted 0x4b, healing 0x51, sitting 0xa0, no-action area 0x44, the recast
  (s2c 0x119 u32 @0xfc seconds) 0x4c; rows grey while any applies (but the cursor row). Enter: refused -> the line and
  SE 0004, the list stays; else `/mount "<name>"`, SE 0002, closed. Manual Sort (abisortw row 1): help 0xe 0x3e6 /
  0x3e7, abisortw's help 0x3e5; the picked mount is inserted at the cursor mount's place in moix (FUN_10226d00).
- /mount (id 0x28, FUN_1007bb90): gate 0x7f, the mount by its 55681 name, c2s 0x01A category 0x1A param = index.
  /dismount (0x38, FUN_1007aa80): category 0x12. LSB checks level 20, the zone (ZoneMisc::Mount: 316 "That action
  cannot be used in this area."), the key item, a 60 s recast and enmity; success sets Mounted (252) and status 5
  (chocobo) / 0x55, with the chocobo music.
- Not done: the mount model under the rider (status 5 / 0x55), the riding layouts of the action menu (Dig, Dismount
  with its "Dismount?" prompt, table 0x1036f390), the emote list.
- Live (Lnxcsfix, !addkeyitem 3072 / 3073, !changejob war 20): the list showed Chocobo and Raptor with their
  descriptions and "Next 0:00"; in Southern San d'Oria the server refused (316); in West Ronfaure /mount Chocobo
  started the chocobo music (212) and /dismount went back (109).

## Moogle menus: s2c 0x02E, "menu    myroom", "menu    mogext", the storage menus (2026-10-23; spec: scratchpad
## moogle/SPEC.md of that session)

- s2c 0x02E (handler 0x100a15f0, header only) sets a pending flag; FUN_10097af0 opens once no event runs
  (FUN_101d6110): myroom in the Mog House (its open hook FUN_10145180 shuts it unless the house is ours: 0x067 @0x20
  owner 0), mogext elsewhere; nothing when a window with registry flag 0x20000 is up (the flag is dropped). Also event
  op 0x85 (not wired yet) and main menu row 12 in the Mog House (FUN_10140400 case 0xc: myroom, SE 0002; disabled when
  visiting, help 0xe 0x29 / 0x2a).
- myroom (16, 208), 7 rows (FUN_10145460): 1 Storage -> "menu    storage"; 2 Delivery Box, 3 Change Jobs, 7 Remodel
  (not done); 4 Gardening / 5 Layout run /garden / /layout and close it; 6 Open / Close Mog House: c2s 0x0CB {1 | 2,
  0, 0}, the label windowps 606 / 607 (pending) / 608, title 0x93 / 0x94, help 0x29a / 0x29b. Gates on Enter: synthesis
  (0x4e) and the 3 s job-change cooldown (not tracked). Row states (FUN_10145240): 4 off with the moogle away; 5..7
  only in the nation house (has_floors) or the Mog Garden (map 0x2d4): open -> only 6; else 5 / 7 with the moogle
  there and not the garden, 7 also with Safe 2; pending -> none. 0x067 kind 2 @0x24 (open to visitors) parsed now.
- mogext (16, 272), 3 rows (FUN_10145760): Storage -> "menu    storage2", Delivery Box, Change Jobs (not done); it
  remembers our spot and closes, with what it opened (the storage view included), past 6 yalms in x / z (FUN_10145680).
- storage (16, 80) 15 rows / storage2 (16, 96) 14 rows (no Storage) (FUN_10145e70): Mog Safe, Safe 2, Storage,
  Locker -> FUN_101dae10; Satchel, Sack, Case, Wardrobe, Wardrobe 2..8 -> FUN_101daf70; nomad 0 in storage, 1 in
  storage2 (Storage itself 0). Disabled at open (FUN_10145ca0): Satchel < 30, Sack size 0, Mog Safe (storage only)
  with the moogle away, Safe 2 without the 2F flag, Wardrobes 2..8 size 0. A 0 from FUN_101dae10 plays SE 0004.
- The openers' nomad path: FUN_101dae10 with nomad in a MogZoneFlag zone opens at once at the moogle and nomad (no
  moogle-away / visiting checks, no line); FUN_101daf70 in our own Mog House says the moogle's line, else nomad +
  MogZoneFlag opens at the moogle and nomad. Bank +0x71 (nomad) takes Storage out of itmstora and the Alt cycle.
- The moogle's line FUN_100a1860(n): system message n (file 7031) as "<moogle> : text": 0x2e "I'll look after your
  items, kupo!" when a storage view opens in the Mog House, 0x32 for Change Jobs. Ours names the nearest entity called
  Moogle (retail finds it by model 0x52 / 0x3d5 / 0x58a / 0x939).
- Live (Lnxcsfix, into the Mog House by Southern San d'Oria's zmr0zmr1 rect, main menu row 12): myroom with Remodel
  dim, the storage menu with Safe 2 and the Satchel dim, Mog Safe opened (0/50) with the moogle's line, Escape back to
  the storage menu. Talking to the moogle (0x02E) was not tried: Tab found no moogle in view.

- Event op 0x85 (FUN_100b9120, 1 byte): FUN_101d6110(0), the moogle menu as s2c 0x02E opens it (myroom). Wired now;
  the sweep is unchanged (1450 finished, 0 faults).

- Riding action menu (FUN_1012b4b0): status 5 (chocobo) or 0x55 (mount) takes the odd layout of table 0x1036f390
  (1 = Chat, Dig, Dismount; 3 / 5 as listed), not for the NPC layout. Dig (keytops3 0x26) only on a chocobo; Dismount
  (0x27) asks group 0xe 0x4d "Dismount?" (FUN_10210e30, Yes / No side by side, default No), Yes runs /dismount. /dig
  (id 0x37, flags 0x41, FUN_1007aaf0) on a chocobo: c2s 0x01A category 0x11 (LSB ChocoboDig); its place checks
  (refusal index 8 = group 7 0x4a) are left to the server (inferred). Live (West Ronfaure, on a chocobo): the menu
  showed Chat / Dig / Dismount, Dismount -> Yes dismounted (category 0x12, the music back).

## The emote list (2026-10-23; spec: scratchpad mount/SPEC.md section B of that session)

- Social menu child 9 "Communication" (drawn between 10 Current Time and 11 Shut Down) opens "menu    commenu" (384,
  48): 1 Friend List (flistmai, not done), 2 Emote List. Its open hook sends c2s 0x119 (empty); the s2c 0x11A answer
  gives the unlocked job emotes (u32 @4, bit job - 1) and chairs (u16 @8); nothing else asks for them.
- "menu    emote" (FUN_10225460) always with abisortw. The tree is file 96 (u32 count, 0x20-byte entries {id, name
  string, help string, parent (0xffff none), cmd[0x10], arg[8]}; a folder has no command): root 51 rows with the
  folders /bell (25 notes), /jobemote (22 jobs), /sitchair (13); strings from d_msg 55676 (en; ja 55556, fr 55796,
  de 56216). Rows in eix.dat order (USER file, version 1, u16[0x80], identity default), every row magico2w 63; each
  folder keeps its cursor row. Locked job emotes (0x4c..0x61) and chairs (0x63..0x6e) are grey.
- Enter: a folder opens; a locked job emote still runs its command with SE 0004; a locked chair: group 7 0xc2 and SE
  0004; else the target prompt (flags 0x7f) and "/<cmd> <arg>" runs while the pick is still the target, then the old
  target comes back (LAB_10225c50; ours SubThen::Command). Escape goes up a folder, at the root closes. Help: the
  entry's help string; Manual Sort 0xe 0x3a3 / 0x3a4 (the insert on eix), abisortw's help 0x39f; title 0xd 0xb4.
- /sitchair [n] (id 0x34): c2s 0x113 {u32 mode 0 toggle, u32 chair} (LSB's layout; the handler's checks are not
  traced). /jump (0x35, a client-only motion) is not done.
- Live (Lnxcsfix): commenu, the emote list (/aim, /amazed, ...), the /bell folder and back with the cursor kept,
  /aim through the target prompt sent; the server's echo could not play the aim motion without a ranged weapon
  (group 9 0x66 "Failed to execute command.", as retail).

## Change Jobs: "menu    jobchang" / "menu    jobcselu" (2026-10-23; spec: scratchpad moogle/SPEC.md section 4)

- s2c 0x01B (GP_MYROOM_DANCER from @4): get_job_flag u32 @0xc (bit n = job n), job_lev[16] @0x10, sjobflg @0x44,
  job_lev2 @0x48 (indexed by job, used for 16..23). Ours World.job_info.
- myroom / mogext row 3 opens jobchang (16, 256) over them; myroom first has the moogle say 0x32 when the zone has a
  moogle (FUN_10097b40). jobchang row 2 (Support Job) is disabled while sjobflg is 0. Enter opens jobcselu (16, 48)
  with +0x14 = 1 / 2; its cursor starts on child 1 *(inf)*. Escape closes only the top window.
- jobcselu (FUN_10145850 / FUN_101458f0 / FUN_10145a90): child n = job n, rows with a clear job bit disabled
  (sprite 333 "? ? ?"); links follow the layout (two columns). "L.%d" (group 10 0x5a) right-aligned with the
  sprite font at (child x1 + 0x1e, y0 + 2): gold 0x80e09620 main (its level from s2c 0x061), blue 0x804070a0 sub,
  else white. Enter on the current main or sub: SE 0004. Else (n, 0) / (0, n) goes to FUN_101d32c0 (Mog House) /
  FUN_101d52d0 (MogZoneFlag zone; nothing elsewhere), jobcselu closes, myroom +0x18 = now + 3.
- FUN_101d32c0: refused (SE 0004) while a request is pending or equal to the last one; the moogle actor
  (FUN_100a0080) gets scheduler 'main' of file 0x13ae (5038) as FUN_10062e20(0x13ae, 'main', moogle effect object,
  FUN_10081550, 0); then FUN_101a8030(0, 1, 0) = c2s 0x053 {0, Disable, 0} (0x88 bytes). Per frame, once the moogle
  is gone or its 'main' no longer runs, c2s 0x100 {u8 main @4, u8 sub @5, u16 0}. The monstrosity path (no 0x053,
  DAT_105782d4 retry) is not done.
- The +0x18 cooldown (FUN_101039e0 <= it) also refuses the myroom / mogext rows (silently) and FUN_101dae10 /
  FUN_101daf70 (the storage openers and /bank etc.).
- FUN_100a1860 (the moogle's line) prints nothing without a moogle actor (all.c 120034); ours still names a
  stand-in "Moogle" (see the next note on why there is none).
- Disabled children in general (FUN_101177d0 / FUN_10119c00 / FUN_10119870): state maps to a ref kind (0 -> 0,
  1 -> 1, 3 -> 3, 5 -> 4, 7 -> 2, 9 -> 5, else 6); refs of kind 0 or that kind match and the last match wins. A
  non-zero kind draws at 0x80808080 (cursor 0x80805020 at +1 px); a state-5 child left with its kind-0 sprite draws
  at 0x30808080 (cursor 0x40402810 at +1 px); state 0x10 never highlights. Ours GameUi::disabled_children (iuse,
  itmstora, the moogle menus).
- Live (Lnxcsfix, own Mog House 1F): myroom row 3 with the line, jobchang with Support Job dim, jobcselu with WAR
  L.20 gold and five L.1 jobs, WAR -> WHM (gold moved, HP 30 / MP 26, LSB took the gear off) and back. The moogle
  dance did not run: no moogle actor in our Mog House (LSB sends it only after c2s 0x01A ActionID 0x14
  SendResRdy, which we never send).

## c2s 0x01A ActionID 0x14 (LSB SendResRdy) (2026-10-23)

- Builder FUN_100780d0 (all.c 89972; 5-argument wrapper FUN_100780a0): 0x1c bytes, UniqueNo @4, ActIndex @8,
  ActionID @0xa, four u32 params @0xc..0x18.
- FUN_100882d0 (all.c 106166): once DAT_1047d6a0 is clear and our actor has a model (+0x120 bit 9), it sends
  {own id, own index, 0x14, 0, 0, 0, 0} and sets the flag (retried each frame until the send works). The flag
  is cleared only by the zone-load reset FUN_10093720 (all.c 113539), so it goes once per 0x00B / 0x00A load:
  login, zone change, Mog House entry and exit. No other resend.
- Caller FUN_10096640 (all.c 115624, per frame) gates: 0x00A handled, FUN_10094190 set-up, c2s 0x00C sent, the
  entity wait DAT_10482f43 (0x00A SendCount u16 @0xac reached by distinct 0x00E actors with u32 @0x18 bit 16
  KingFlag, all.asm 167839, or 600 ticks; skipped in the Mog House, all.c 114158), game mode 0x60, in the Mog
  House the room object's +0x18 counted down from 0x14 per frame (FUN_101d2cc0 / FUN_101d4a20), and any zone-in
  event starts first.
- LSB: in the Mog House it runs SpawnConditionalNPCs, the only way the Mog House moogle (z 1.5, model 0x52) and
  the Symphonic Curator reach the client; elsewhere it sets requestedInfoSync and spawns NPCs / mobs.
- Ours (client/mod.rs res_rdy): once the world is shown, in the Mog House after 20 ticks of 1/60 s *(inf: the
  frame length)*, elsewhere when the KingFlag count reaches SendCount or 600 ticks after the 0x00A.
- FUN_100a1860 now prints nothing without a moogle actor, as retail.
- Live (Lnxcsfix, Mog House 1F): the moogle appears; Change Jobs WAR -> MNK -> WAR, each with the moogle's dance
  (file 5038 'main' on the moogle and us) before c2s 0x100.

## Name plates under the screen fade (2026-10-23)

- Frame order in FUN_10010d20 (30261006_0): the scene target is bound (FUN_1000a800, 0x10011b50); every scheduler
  task runs into it (FUN_10074c30, 0x10011ca2): the actor draw at priority 6, then CXiActorNameDraw (FUN_100817f0,
  vtable 0x10329c18, priority 0xc), which draws the plates at once (DrawPrimitiveUP, no deferred queue); the op 0x10
  crossfade and the op 0x72 flash also go into the target. Then the back buffer (FUN_1000a1d0, 0x100126a1), the
  target copied with the op 0x0f colour as MODULATE2X (FUN_1000c0b0, 0x10012745), then the 2D windows (FUN_101604b0
  .. FUN_10160650; the target arrow is a window too *(inf)*), then the op 0x51 level over everything (FUN_1000a820,
  0x100129ea). The fade colours are stored by the scheduler tasks (renderer +0x338 / +0x334 / +0x328) and copied to
  the draw copies at the frame end, so they apply one frame late.
- So op 0x0f darkens the plates but not the windows. Ours draws plates in the HUD batch after ScreenFx; their colour
  is now scaled by the same multiply (and the zoning fade), which equals retail for an alpha-blended plate *(inf:
  up to the clamp of MODULATE4X)*. Seen before: a "Moogle" plate bright over the fdo1 black in the AMK cutscene.
- Not done: op 0x51 multiplies the scene too in retail (ours only the 2D layer); the one-frame delay.
- MODLOG's FUN_10080100 / FUN_10084b20 / FUN_100ce4d0 / FUN_1005f630 names are the 2019 build's.
- Also live 2026-10-23: talking to the Mog House moogle with AMK on starts LSB's AMK mission 1 cutscene (event
  30023); it played to its end, and talking again opened myroom through s2c 0x02E.

## Delivery Box: s2c 0x04B / c2s 0x04D, the send and receive UIs (2026-10-23; spec: scratchpad post/SPEC.md)

- c2s 0x04D (FUN_100f7bc0, 0x20 bytes): Command @4, BoxNo @5, PostWorkNo @6, ItemWorkNo @7, i32 ItemStacks @8 (-1),
  Result / ResParam1..3 @0xc (0), TargetName[16] @0x10 (first letter upper-cased). Builders FUN_100f7b70 ..
  FUN_100f7fa0: 1 Work (box, post), 2 Set (2, post, inventory index, stacks, name), 3 Send, 4 Cancel (2, post), 5
  Check (box), 6 Recv (1, post, item 1), 7 Confirm, 8 Accept / 9 Reject (1, post), 0xa Get / 0xb Clear (box, post),
  0xc Query (name), 0xd DeliOpen / 0xe PostOpen / 0xf PostClose.
- s2c 0x04B (handler FUN_100a38b0): Command @4, BoxNo @5, PostWorkNo @6, Result @0xc (1 done, 2 the first of LSB's
  pairs, negative errors), ResParam1..3 @0xd, the slot: Stat @0x10, Name[16] @0x14, OpponentPBoxNo @0x2c (AH gil:
  item | sold << 16), ItemNo @0x30 (0xffff gil), Stack @0x38, extdata @0x3c. Result -6: group 9 0x2d; -7: group 7
  0xb9. Each Command stores its slot only for the Results it takes (Work / Set / Check / Confirm / Get / Clear 1;
  Send / Cancel / Recv / Accept / Reject 1 and 2); every store sets the single latest-answer slot (S+0x2c7c8 /
  +0x2c7cc, flag 0x4000000) that the UIs take once a frame; Check fills the receive (ResParam2) or send (ResParam3)
  count. Query's ResParam1 1 = the recipient is on our account. 0xd / 0xe Result 1 open the UIs (so an NPC's
  unsolicited DeliOpen opens the send UI), 0xf Result 1 closes them.
- Text commands /mailbox, /deliverybox (id 0x7c) and /sendpost (0x7e), FUN_1007e490 / FUN_1007e560: one's own Mog
  House, not visiting (group 7 0xbc), status gate 0x44, the job-change cooldown, the moogle home (0x48), then
  PostOpen / DeliOpen. /logout and /shutdown are refused while a UI is up (group 7 0x54 send / 0x55 receive).
  myroom / mogext row 2 opens "menu    mogpost" (1 Receive, 2 Send): myroom runs the commands as menu lines, a
  nomad moogle sends 0x0e / 0x0d itself; it stays open, Escape closes it.
- Receive UI (DAT_10578450, FUN_1014cb70; states as retail): open (FUN_1014ce80; refusals group 7 0x45 / 0x51) Work
  (1, -1), 1 Check, 3 / 4 Recv into each free slot while the count lasts, 5 / 6 browse; post1 (16, 48) slots drawn for
  Stat 6, 7, 0xa..0xe at 0x60808080; Enter on Stat 7 / 0xb / 0xd focuses post2 (16, 144): Take, Dispose, Return
  (disabled for "AH-" senders); Dispose / Return ask comyn (group 9 0x34 / 0x35, default No). Take = Accept + Get
  for Stat 7 (Get alone for 0xb), Dispose = Accept + Clear, Return = Reject; a failed Get prints 0x88 / 0x89.
  Escape sends PostClose. Every request times out after 300 ticks: group 7 0x85, closed.
- Send UI (DAT_10578198, FUN_1012e880): Check, Confirm for each item the recipients took, Work, then the name in
  stringdl (16, 144; 15 characters, FUN_10077c30 rules), Query, browse: an empty slot opens the inventory list at
  (200, 48) as the picker (grey: NoDelivery without CanSendAccount; CanSendAccount to another account; equipped),
  itemctrl for a stack, the gil field (child 11) moneyctr (130, 240) for gil; each placed item goes out as Set at
  once; Send sends every Stat 1 slot; Enter on a used slot takes it back (Get). Escape from the slots takes the
  unsent items back (Get for Stat 1 / 0x10) and returns to the name; Escape / Cancel in the name closes (PostClose).
  Down out of stringdl's buttons enters cancel mode (group 9 0x32): Enter on a slot, then OK in stringdl, cancels a
  sent item (Cancel) or takes a placed one back. Errors (FUN_10130290): -0x47 0x88, -0x46 0x89, -7 0xb9, -2 0x87,
  -1 0x86, then SE 0004.
- Lines: Get / Clear / Reject on a slot (FUN_1014d0b0 / FUN_1012f2a0, table 0x10371a64): 0xbe..0xc6 with {slot + 1,
  item, count} and string "delivery" (group 9 0xc1 / 0xc0); the cursor's gil line (FUN_1014ca70) 0xaa {cursor child,
  amount, string = sender} or 0xab / 0xc7 for AH sales.
- Draw: slots by FUN_1015bd70 (32x32 icon, "%2d" up to 99 right-aligned at (+30, +22) for stackable items but gil,
  the cursor cell at (-1, -1)); the gil field FUN_10140910 ("N  G" right-aligned at the child's right edge + 10,
  always); labels "Sender:" / "Recipient (...)" game font white at (+10, +6); names game font 0x80606010 at (+20,
  +30) cut at 128 / 130 px with "..."; the editing field white with '|' (19 of 30 ticks); stringdl buttons by code
  (keytops3 53 / 56 and 59); moneyctr FUN_101409f0 (value right-aligned at (+0x7f, +0x16), the step's digit
  0x80a06040, leading zeros below the step, "/" and the max at (+0x80 / +0x88, +0x24)).
- Expander (FUN_100c1650): 1c n = string slot n (0 actor, 1 target, 2 / 3 the caller's strings); 7f b4 n = work[n]
  clamped to 0..999999999 with separators (FUN_101c9440) and group 10 0x5f "%s gil". The queue entry gives work ==
  num for these lines (FUN_100a1ab0).
- Not done: the augment Ex rule and the equipped linkshell in the picker colours; the monstrosity / menu-lock
  gates; caret movement inside the name; the one-frame Recv-pair miss (we compare frames as retail).
- Live (Lnxcsfix to itself, Mog House): name, Query (same account), dagger + 1 gil Set and Sent (Stat 3), unsent
  items taken back on Escape; /mailbox: two Recv, Take x2 with "1 gil, sent to you by Lnxcsfix." and "You take the
  1 gil out of delivery slot 2.", PostClose.

## /jump (2026-10-23)

- /jump (id 0x35, FUN_1007c4f0): needs SendResRdy sent (FUN_100882c0) and status gate 0x77; status 0 or 1 sets our
  actor +0x13c bit 22, else the handler fails. Per frame FUN_1008b410 (called from the actor update FUN_1008e9a0)
  turns bit 22 into bit 21 and, for our own actor, sends c2s 0x11D {u32 UniqueNo, u16 ActIndex} (0xc bytes,
  FUN_100a2720). s2c 0x11E (LAB_100a2760, ActIndex @4) sets bit 22 on that actor. FUN_100c63d0 sees bit 21, clears
  it, and when no jump runs sets draw actor +0x8bc = 32.0 ticks (+0x8b6 = 1, the landing step flag); it counts down
  by the frame ticks. While it is >= 0, FUN_100c8930 (LAB_100c8dde) replaces the stand / walk motion with "jmp*"
  (status 1) or "jmp " at rate 1.
- LSB pushes 0x11E with CHAR_INRANGE_SELF, so the jumper gets it too; by the logic above that would send 0x11D
  again every time, so ours ignores our own index *(inf: retail's server likely does not echo)*.
- Ours: Actor.jump, the motion "jmp" (prefix); the landing flag is not used. Live: Lnxcsfix in the Mog House,
  /jump twice, "idl -> jmp -> idl" after 32 ticks.

## Mouse picking in the world (2026-10-23; spec: scratchpad mousepick/SPEC.md)

- FUN_10083750 (the hover via FUN_10126270 every frame, the click via FUN_10158d90): pointer in NDC from UI pixels;
  the actor list near-first (FUN_10082d70: the box's farthest corner depth); filter FUN_10083430 with the target
  object's kinds, **self allowed** outside a prompt (T+0x60 = all, bit 0x200), inside one only with 0x200; no name
  test, no distance limit. First candidate whose rect holds the pointer (inclusive) with every corner in front, then
  whose fine test hits, wins (the fine test's depth is never written: in effect the first hit near-first).
- Rect (FUN_100cfc10): the status box (FUN_100cfad0) as a plain box, x {b2, b3}, y {b0, b1}, z {b4, b5}; b0 == b1
  widened by 1, then b2 == b0 (sic) widens b2 / b3; RotY(yaw) x Translate(pos), no scale, no tilt. No box: not
  pickable. Doors without a region box (FUN_100ad320): a 0.5-tall box at the door, always a hit inside it (the RID
  box path is not done; "StreetLamp" without one is not pickable).
- Fine test (FUN_100cff40): fading-in actors (+0x59c < 0.01) miss; <= 2 bones hit; circles at each geometry bone
  (skeleton byte bit 0x80: bones a loaded mesh weights and their ancestors) whose parent has geometry too, the parent
  and the midpoint, radius 0.4 x the segment's NDC length (no aspect correction); else circles around attach points 1
  and 3 of world radius 0.75 x min(L13, L15) + 0.25 x max(L13, L15) (horizontal offsets of points 13 / 15).
  Ours reads the bones back from the GPU pose (render/points.rs) for the actors whose rect held the pointer, a
  frame or two late; before any came back the rect decides *(inf)*.
- Clicks (FUN_101585f0, results 0x16 left / 0x17 right from key functions 0x8b / 0x8c): left on another actor
  targets it; on the target, on nothing with a target, or on a refused actor: the confirm FUN_10158db0 (talk to an
  NPC, else "menu    playermo"); on nothing without a target: playermo (no nearest search, unlike Enter). Locked
  (no prompt): no pick, every click is the confirm, so a locked monster opens the action menu (the earlier note
  "monsters take no action" was wrong). Right: cancels the prompt, clears an unlocked target, or with none stops
  autorun. A drag (65 UI px^2 from the press or 25 ticks held, FUN_10125040) release only picks. After a confirm
  the rest of the frame's keys fail (FUN_10123ea0); ours swallows the click for the windows. In a prompt the click
  moves the pick or, unchanged, confirms it (confirm_sub). Blocked during an event, typing, a focused window
  (unless a prompt is up), the UI hidden.
- Ours: App::under_pointer / fine_hit, target::mouse_candidate / click / confirm_target (Enter uses it too),
  App::mouse_button (window and test-script pointer events alike). Not done: the hover's frame lag, the talk lock,
  the Num+ focus mode gate, the ground point (T+0x22).
- Live (Lnxcsfix, Mog House): a click on ourselves targets us, a second opens the action menu; right click clears;
  a click on the moogle targets it, a second talks (myroom through s2c 0x02E).

## Synthesis (2026-10-23; spec: scratchpad synth/SPEC.md)

- Entry: iuse Use or the direct-use list on a crystal (type 8): status gate 0x77, then FUN_101e0e30(index, id, mode):
  refused silently while the cast bar is up, group 7 0x8c with no free inventory slot; the inventory list is
  re-sourced as the material picker at (200, 48) and "menu    tskill1" (16, 48) opens, cursor child 1. (Main menu
  row 10 "menu    cmbmenu", the history and favourites, /lastsynth (id 0x9d) and the cmb%d.dat files are not done.)
- tskill1 (FUN_101e12e0): the crystal icon at half size at (+0x12, +4), its element sprite keytops3 0xb9 + element
  at (+0x24, +4) (0x1000.. / 0x108e.. / 0x196a.. ranges), eight slots by FUN_1015bd70. Keys (FUN_101e1530): slots
  focus the picker; 9 Synthesize; 10 Cancel (back to the list it came from); Escape moves the cursor to Cancel.
- Picker (DAT_106634d0): help 0xe 7; grey for linkshells (0x201..0x203), equipped items and the crystal; a stack
  already in a slot orange 0x80804020. Enter: itemctrl for a stack; FUN_101e0d30 puts it in the cursor slot (the
  same stack in another slot moves; on this slot it is taken out).
- Synthesize (FUN_101e08e0): gate 0x77; FUN_101e1f40 (crystal and slots up to the first empty one still held,
  unlocked, enough); FUN_101e0770: the units (more than 8: group 9 0xf; none: group 7 0x63), a free slot (0x8c), not
  healing (0x51), no request in flight (0x64). c2s 0x096 {HashNo, 0, u16 crystal, u8 index, u8 n, u16 ItemNo[8]
  sorted ascending, u8 TableNo[8], u16 0}; HashNo = ((n + 5) x (ItemNo[0] + 7) x (crystal' + 3)) % 0x7f with the
  cluster ids folded. Everything closes at once. The request waits 30 s for 0x06F, then answered 2 s.
- s2c 0x06F (a pre-hook calling LAB_101e0940): Result 0 / 0xc the result (trddummy: 300 ticks, one line a frame:
  0xa0 + Grade {item, count}, 0xa9; Result 1 group 9 0xa then one 0xa4 per lost item), 2 / 0xe group 9 0xc, 3 0x9,
  4 0xe, 6 0xd, 7 0x10, 0xd group 7 0x64, others group 9 0xb. s2c 0x070: 0xc8 + Grade / 0xcc per lost item / 0xcd
  with the name in string slot 2.
- s2c 0x030 (FUN_1009f990, @4 UniqueNo, @8 ActIndex, @0xa EffectNum, @0xc Type, @0xd Status, @0xe Timer): the
  actor's status takes Status; FUN_1009f4c0 per frame: motion scheduler res0 (rx0 / rx1), the effect file 0x1330 +
  EffectNum's frst (motion ri0), then ednn / edmm / edxx by Type, motion res2 (rx2), and for us c2s 0x059 {u32 0}.
  Status leaving 0x2c during frst: stcr, res2, 0x059 {1}. FUN_100964f0: no movement in status 0x2c.
- Expander: 7f 80 n sets the case of the next grammar block's text (FUN_100c2bf0: 1 first letter upper, 2 all upper,
  3 all lower, 4 each word's first; a block with no text keeps it); grammar kind 1 prints nothing and takes the
  space after it (FUN_1025a400(0), as a count of one). "An ash log was lost.", "<name> lost an ash log."
- Live (Lnxcsfix, Mog House): !additem Wind Crystal + Ash Log, Use on the crystal, the log placed, Synthesize:
  res0 / frst (DAT 4929) / edmm / res2, "Synthesis failed. You lost the crystal you were using." and the lost log.

## Synthesis menu, history, /lastsynth; message expander fixes (2026-10-23)

- Main menu child 10 (drawn fifth, y 70) opens "menu    cmbmenu" (384, 48): 1 Synthesize (the inventory list on
  the crystal source: crystals only, yellow 0x80909040; Enter: gate 0x77, tskill1 mode 1), 2 History (FUN_101e1000:
  cmbhwin, cmbhlst moved to (200, 48), tskill1 mode 2). Both rows disabled while a request is younger than 23 s.
- History (DAT_106634b8, newest first, ten): added on Result 0 / 1 / 0xc (FUN_101e1d70 at the head; the same crystal
  and materials replace their record, a failure does not replace a success, the favourite bit stays; a full list
  drops its oldest non-favourite). Files USER/<id>/cmb0..9.dat (cmb0 newest), container version 1, payload 0x20: u32
  state (3 dirty / 2 saved), u8 Result, s8 Grade, u16 ItemNo, u16 CrystalNo (bit 15 favourite), u16 MaterialNo[8],
  u16 0, u32 valid 1. Loaded once per login at the first zone-in (FUN_1015d860 broadcast with manager +0xaa), saved on
  every s2c 0x00B (FUN_1015d880) and at teardown; never after an add.
- cmbhlst rows (list FUN_101f6a60, columns {0, 18, 170, 200}, row step 18, origin (0, 3)): the icon at half size
  at (+3, +6), the name in the game font at (+21, +5), white; the favourite keytops3 101 at (+166, +5); the bar
  keytops3 428 at (0, +6 + 18 k) while focused. Moves refresh tskill1 (FUN_101e27d0: crystal and materials from
  the inventory; a material not held keeps its id, count 0); Enter focuses tskill1 on Synthesize (disabled unless
  everything is held, FUN_101e1200); Escape back to cmbmenu; Num+ (key 0x11, list message 8) swaps with cmbhwin
  (1 register / 2 unregister the favourite, help "%s" = the item name). tskill1 mode 2: missing marks keytops3 515
  (red cross) at 0.3 over the crystal (element dimmed 0x80585858) and 0.32 at a slot's (+0x14, +0x14); Cancel
  returns to cmbhlst; no picker.
- /lastsynth (id 0x9d): the newest record's crystal and materials from unlocked stacks, sent as tskill1 would, or
  group 9 0xe; "check": 0x10f item, 0x110 crystal, 0x111 per material with its count.
- Expander (FUN_100c1650 / FUN_100be760): grammar kinds 1 / 2 and a count of one are article tokens (FUN_1025a400(0 /
  1)): the next noun token (FUN_1025a2f0, mode 3 singular / 4 plural) takes "a" / "an", "the", or "some" in front;
  the singular form has no article of its own (it used to: "1 an ash log."). 0a n = num[n] "%d". 7f codes: 94 / 98
  / 99 / 9a = %02d / %03d / %04d / %05d of num[n], 95 "$%x" num[n], 96 "%" + binary, 97 "$%x" work[n] (was decimal),
  9b work[n] x 0.1 "%.1f" (skill ups: "skill rises 0.1 points."); every handler 0x80..0xb7 is 3 bytes but 0x8f..0x93,
  codes past the table (fb, fc markers) 2. Zone dialogue: grammar 0x29 / 0x2a (count index then item index from num
  / work) added ("You obtain 3 wind crystals!").
- Live (Lnxcsfix, Mog House): menu Synthesize and History, /lastsynth check, /lastsynth twice, a favourite set,
  cmb0.dat written; chat lines "You obtain 3 ash logs!", "1 ash log.", "Lnxcsfix's woodworking skill rises 0.1
  points.", "You synthesized a piece of ash lumber.".

## Ours, not retail: a synthesis amount (2026-10-23, the user's design)

- Synthesize (after retail's gate 0x77 and inventory check) asks how many in itemctrl hung from tskill1's bottom edge,
  1 filled in so Enter goes on as before. Up / Down 1, Right 1, Left the most (complete sets of the crystal and every
  material at its count in unlocked stacks, 1..99), digits type the number, Backspace drops a digit, Escape back to
  tskill1. Bars: itemctrl's (0xd 0x3e, 0xe 0x10c).
- The repeats (synth::repeat_frame): each next one goes when the last is over (its 0x06F in and the retail 2 s
  request lock gone, our animation done, out of status 0x2c, no synthesis window), from fresh stacks as /lastsynth
  finds them; LSB's own rule is 15 s from the last start (0x096_combine_ask.cpp), and a synthesis takes about 16 s.
  Failures go on; any other answer (interrupted, a cancel, wait longer), missing materials (group 9 0xe) or a zone
  change stop them. Sends actions by itself: the user asked for it as a standing feature (not an Extras toggle).
- Live: History -> Synthesize -> 3 typed -> three in a row, about 5 s apart (two failures, one success).

## Effect element life: GPU and CPU in step (Bastok smoke flashes, 2026-10-23)

- FUN_10043ca0 (2019 build, all.c ~49361): life -= dt, then expire / delete at life < 1.0 before the update stream
  (FUN_100499d0), so retail never samples t above 1 - 1/max_life on a dying element (flag 0x20000000 elements are held
  at life 0 instead). Many alpha keys rely on it: they end (0.99999988, 0) (1.0, 1.0), a spike only t = 1 reaches.
  Seen in DATs 330, 331, 334, 335, 337..341 (brsk / skah: Bastok Markets' chimney smoke brs1 / brs2, sk06..sk13;
  awat, tk01, lint, aalf, malf, gsaf, site, awaa, bukb, jkat, ptaf, hatm, fwsa, ptsc, paaf).
- Ours had the GPU decrement a new element on its spawn frame while the CPU's lives had already run, so the GPU was a
  birth frame's dt ahead; an element born on a long frame (dt >= 1 tick) reached GPU life <= 0 while the CPU still
  kept it and drew at the spike's alpha 0xff (additive 0x48 blend: a white puff for a few frames).
- Fix: add_elem's full write sets CTL_LIFE_SET (no decrement on the spawn frame); effects.wgsl update and update_ref
  hide an element whose life drops under 1 and skip its stream (FUN_10043ca0's order).

## Config: Extras and Settings as rows and pages; timestamps on every line; the menu keeps its page (2026-10-23)

- How a Config page is drawn (menu DAT 39542): configwi's window sprite comwin 12 and each page's windowps 465 /
  464 are one sprite: cell 0 the backing (newtex, skipped by FUN_101144b0), the header strip (gauge), bullets
  (gauge 8 x 8), every heading and label as "font    font" glyph cells (the fontshp glyphs, each a full width on),
  the slider tracks (gauge caps 4 x 8 and a 64 x 8 middle, alpha 49) and, under each button, a dark copy of its
  buttonto cells at (+2, +2) coloured (1, 1, 1, 169), alpha blended. The children (buttons) are additive
  (b[1, 0, 2, 1], colour 97, 97, 128, 169): over that dark copy they read dark slate; without it, pale lavender.
  A button's caption is its glyphs twice: at (+2, +2) alpha 49 subtracted (b[1, 2, 0, 1]), then alpha 192
  additive (b[1, 0, 2, 1]); headings once, alpha blended (b[1, 0, 1, 1]). Captions start at (+5, +1) in an 88 x 16
  button. Markers: windowps 387 (88 px) / 412 (64 px) at (+37, +4).
- The caption glyphs are the small bold set at the top of "font    font" (rows 13 px apart from y 0, ASCII order
  from '!'), not fontshp's (the larger set lower down, drawn at half size, thinner). Our table (screens.rs
  CAPTION_GLYPHS) takes each glyph's rect from retail's own label cells where one exists (all but ','), its top
  within the line from the rect; each glyph advances its width, a space 5 ("Game Settings" in 465).
- Ours (not retail): configwi gets rows Extras and Settings under Gamepad (y 214, 230, its 16-px pitch; the frame
  32 px taller), drawn as its rows (windowps 105's buttonto cells over the dark copy, the caption as above);
  Global -> Extras -> Settings -> Gameplay with Down, back with Up. Enter opens our page at conf2win's place
  (16, 48, 366 x 296): the frame, 465's header strip, the title, then rows of a bullet and a heading with
  conf2win's 88-px or conf3win's 64-px buttons (up to three choices) or a slider (track from 465's cells, fill
  windowps 384; Enter then Left / Right, as the pages' sliders) under it, the slider's value at the heading's end.
  Settings has seven pages, turned as miss00 turns nations (children 9 / 10: menu frames 25 / 26, 18 x 9, at (9, 11) /
  (343, 11); the page name in the game font at (+60, +10), FUN_1022de70). Ours (the user's rule for every paged
  window, retail's included when built): the page name is a cursor stop (the page opens on it; Up from the first
  row reaches it) where Left / Right turn the page; elsewhere Left / Right move between a row's buttons (a slider
  row turns the page), Ctrl / Alt + Left / Right and Tab always turn it, and so does a click on an arrow. Buttons
  whose captions do not fit 88 / 64 px widen together, from x 32. Extras (off
  by default): the map overlay (Ctrl / Alt + O) and its opacity, the synthesis amount, timestamps on all lines.
  The lobby's lobycwin keeps its Settings button; the page is centred there.
- Timestamps: retail's /timestamp (config 0xb5) stamps chat lines only (12-hour, FUN_101bf740) in colour 0x31; the
  stamp is now drawn by the log (each line keeps its arrival time and whether it is chat), in 0x31's colour, not
  baked into the text. The Extras switch stamps every line in 0xb5's format (seconds while 0xb5 is off).
- The main menu opens on the page it was closed on (the user's account of retail; FUN_1013b3f0 / socialme's
  open path not traced). Cursor row 1.

## Chat log fixes, first batch (2026-10-23; audit ~/ffxi/lab/research/chat_audit/REPORT.md)

- Talk (FUN_1007bf30): mounted (FUN_101cbe50 / FUN_101cbe60) refuses with group 7 0x4b before anything else; out of
  range (6.0, _DAT_1032cf50) prints system message 9 "Target out of range." (FUN_100a1ab0, file 7031). It printed
  battle message 9. Invisible (own +0x128 bit 6, 7031 0xac) not done: we do not keep that flag.
- /party (FUN_100e3400): fewer than 2 in the party (FUN_100e8f70(5)) prints 7031 0x24 "There are no party members."
  and sends nothing; the caller still echoes the line. /linkshell (7031 0x25, FUN_100e9730) not done: the
  equipped linkshell (s2c 0x0E0) is not kept.
- MsgShift (inferred): ids outside the matched segments take the nearest segment's delta (a gap of up to 8 just
  before one takes the next one's). Every zone's table is delta 0 for this install, and 263 LSB ids in 147 zones
  were dropped (zone 101: 141 "Nothing happens...", 6409, 6417 "Obtained: <item>.", 6420 now show).

## Treasure pool lines: s2c 0x0D2 / 0x0D3 (2026-10-23)

- 0x0D2 (LAB_100ecb30; LSB 0x0d2_trophy_list): Gold @0xc > 0 prints 7031 0x13 (actor us, target TargetUniqueNo
  @8, Para0 the gil). TrophyItemNo @0x10 != 0 fills slot TrophyItemIndex @0x14 (G+0x19520 item, +0x1956c the source
  actor) and, with a TargetUniqueNo (LSB sends 0 for the pool resent at zone-in), prints 0x10 "You find X on Y."
  or, IsContainer @0x16, 0xda "You find X in the Y." (actor none, target the source, Para0 the item). Item 0 clears
  the slot.
- 0x0D3 (LAB_100ecf80; LSB 0x0d3_trophy_solution) by JudgeFlg @0x15: 0 a lot, 1 won, 2 won but refused, 3 lost.
  The lot line (0 and 1, when EntryPoint @0x12 is not -1 / 0 and sLootName2 @0x26 is set): 0x11 (0x82 without
  EntryUniqueNo @8) "X's lot for the Y: N points.", string slot 2 the points ("%d", 0x10352ff0), slot 3 the lotter's
  name. Won: then 0x12 (0x83 without LootUniqueNo @4) "X obtains Y.", slot 2 sLootName @0x16. Refused: 0x0f with the
  winner, else 0x1f. 1..3 clear the slot; 3 prints nothing in the client. FUN_100a1ab0's message: +0 actor, +4
  target, +8 id, +0xc Para0, +0x20 / +0x30 string slots 2 / 3.
- Not done: the treasure pool window, lot / pass (c2s 0x041 / 0x042), the pool's timers.

## The zone's ambient loop (2026-10-23)

- Every zone has one looping ambient voice (West Ronfaure's se 1005 is wind, not the stream: streams are sound
  generators along Sph chains, below); ours played none. Retail
  (30261006): each frame FUN_1018b860 (all.c 13257) runs FUN_101874a0 (copies the weather object's +0x2c into
  world+0x20) and FUN_10036170(world+0x20): 0 stops the voice (FUN_10036b90); a different resource starts its SeSep
  through FUN_10036f00, pan 0x40, level ftol(DAT_10351708 x 127.0), voice DAT_10463910, resource DAT_10463914. The
  level is the zone channel: FUN_10035fb0 (config x event mask 4) re-levels the running voice. (The 2026-10-22 entry
  below named FUN_10035350 / DAT_10450fd8 / DAT_10346570; those are the old build's.)
- The choice (FUN_10189ca0 via FUN_1018a220): type-0x3d chunks under the weather folder (weat/<tag>, else suny);
  with an "indo" folder, covered (world+0x1bc, FUN_1018b5e0 from the eye ray) takes only those inside it, else only
  those outside; names pass FUN_10189bd0 (four digits HHMM, the first '0' / '1', the third at most '5', the parent
  not "extr"), the time by FUN_10072180; the latest at or before now (FUN_10189c60: DAT_1065cb18 or the event
  override world+0x1c8), else the last valid one in file order. West Ronfaure (DAT 200): 0600 se 1005, 1800 se
  1007, indo/0000 se 1056; East Ronfaure 1005 / 1007; La Theine 1001 / 1003, rain 1042, wind 1002 / 1004.
- Ours: Effects::ambient (fx/mod.rs) picks it each frame with our covered flag; Audio::ambient plays it centred on
  handle 0xffffff00 at 127 x effects x the zone channel (event masks 4 / 8, fades as the others), re-levelled when
  that changes, stopped outside the world. Not traced: how retail ends the old voice at a change (we stop it), the
  event override of the time.
- Positional sounds in DAT 200 (effe/aose insects, effe/aotr birds, mode/ligh/taki bonfires) are generators and
  already played; none sits on the stream.

## s2c 0x017 chat lines and our echoes (2026-10-23; full spec ~/ffxi/lab/research/chat_audit/c017_SPEC.md)

- Handler LAB_100e37a0 (registered all.c 167487, no C body) -> printer FUN_100d9b20 (all.c 161041) -> FUN_10127c00
  with the kind's mode. Layout: Kind @4, Attr @5 (bit 0 GM: "[GM]", skips block list and spam filter; bit 3 a
  window, not the log), Data u16 @6 (yell zone; assist: mastery, mentor rank), name @8 (15), text @0x17 (150).
- Kinds (byte table 0x100e3de8, jump table 0x100e3da0) -> line, mode: 0 / 0x18 "%s : %s" 9; 1 the same, 10; 3
  "%s>> %s" 0xc (and 0x15) feed the tell history; 4 "(%s) %s" 0xd; 5 / 0x1b / 0x1e "[1]" / "[2]" / "[3]<%s> %s"
  0xe / 0xd6 / 0x79 (no prefix in PvP, FUN_100e3750); 6 / 7 "----== SystemMessage ==----" then the text, 0xc8; 8
  "%s %s" 0xf; 0x1a "%s[%s]: %s" 0xb (zone from the place names, else group 9 0x40); 0x21 "{%s} %s" 0xd4; 0x22 /
  0x23 rank icon 0xEF (mentor x 10 + mastery + 0x24, 0x2f..0x4c) then "%s(J) : %s" / "(E)", 0xdc / 0xde; 0x19 "%s :
  %s" or the text alone; NS kinds 0xd..0x10, 0x1c the text (9 / 10 / 0xd / 0xe / 0xd6), 0x11..0x17, 0x1d, 0x1f,
  0x20 the text, 0x79. Dropped: 2, 9..0xb, 0xc (the GM prompt menu), past 0x23.
- Echoes (all.c 90072..90740), our name and the text: say "%s : %s" 1, shout 2, yell "%s[%s]: %s" 3, tell
  ">>%s : %s" 4, party "(%s) %s" 5, linkshell "[1]<%s> %s" 6, linkshell 2 0xd5, unity "{%s} %s" 0xd3, assist
  "%s(J) : %s" 0xdb / "(E)" 0xdd, emote "%s %s" 7. Colours by mode through the mode table; config 0xcb / 0xcc are
  colour ids 0x70 / 0x71 (FUN_10129f80).
- Not done: the block list (FUN_100e0a50), config 0xce's spam filter (kinds 1, 0x1a, 0x22, 0x23), PvP (s2c 0x0B4
  u16 @0x10), the echoes' rank icon, the profanity mask, the line cut at a control byte, Shift-JIS text (we decode
  UTF-8), c2s 0x0B5 / 0x0B6 at 150 bytes (ours 127).

## s2c 0x027 / 0x043, zone text colours, 0x009 keys (2026-10-23; spec ~/ffxi/lab/research/chat_audit/c027_SPEC.md)

- Current handlers (FUN_10094190, all.c 114176..114187): 0x036 100a0ef0, 0x043 100a1070, 0x02A 100a12a0, 0x027
  100a1dc0, 0x053 100a2000; 0x009 LAB_100fbd40 (FUN_100f9b20, all.c 183449). Older MODLOG addresses for these are
  the 2019 build's.
- 0x027: UniqueNo @4, ActIndex @8, MesNum @0xa, Type u16 @0xc, Flags @0xe, Num1[4] @0x10, String1[32] @0x20,
  String2[16] @0x40, Num2[8] @0x50. Zone dialogue by MesNum as 0x036; String1 -> string slot 0, Num1 -> num 0..3,
  Num2[4..7] -> num 4..7 (Num2[0..3] unread). Prefix "%s : ": bit 15 clear String1's, or with String2 / Flags 1 the
  NPC's own name (a PC id: String2); bit 15 set: String2 when set and Flags 2. LSB: messageName (Type 4), fishing
  catches (Type 0), caskets / treasure / assault / nyzul loot. E.g. 236 #7164 "Lnxtest caught a moat carp!".
- 0x043: UniqueNo @4, ActIndex @8, MesNum @0xa, Type u8 @0xc, sName[16] @0x10 -> string slot 0; else 0x036's path
  (LSB: fishing's monster / large box).
- Every zone line's mode: Type below 8 by table 0x1035b944 (8e a1 90 91 92 a1 94 95), else 0; a `1f xx` in the text
  overrides it. Bit 15 clear needs the speaker here with that id and index, else the line is dropped (ours keeps a
  0x02A line that names its speaker). Not done: the event-lock drop ((DAT_104808fc | DAT_1047fb40) & 0x40).
- 0x009 Data @0xd: keys found anywhere (strstr): CasUniqueNo, TarUniqueNo (stored over CasUniqueNo), Para0..3, Mode
  (atoi), string2 / string3 (24 bytes to a space / tab / NUL). 7031 line: actor from Cas (none for 0), target none,
  work Para0..3, string slots 2 / 3. /random: 88 "Dice roll! X rolls N!". Not done: the block list (Attr 0x10),
  SysMesFilterLevel (s2c 0x0B4 ConfData byte 1 bits 3..4 against table 0x1035b950), the Mode key.

## The server message: c2s 0x04B / s2c 0x04D, /servmes (2026-10-23; spec ~/ffxi/lab/research/chat_audit/servmes_SPEC.md)

- FUN_100955d0 (all.c 114627) runs "/servmes" once per lobby login, on the first zone (DAT_10487e90 per zone, and
  *(0x1048fe10 + 0x10), set on the lobby -> game step, cleared by a zone change or logout), from the zone set-up
  (FUN_10094190, EventNum 0) or the event's end (FUN_100ae3f0). /servmes and /smes (id 0x107, flags 0x61) ->
  FUN_10080340 -> FUN_100a3040: a transfer unless one runs (DAT_10487f08), kind 1, a 0x400 buffer, language 2.
- c2s 0x04B (FUN_100a2aa0), 0x18 bytes: Command (1 first, 2 the rest), Result 0, kind 1, language 2, i32 timestamp,
  size_total, offset, data_size (0xec pieces). After the first reply up to 4 pieces at once (FUN_100a2b20); a piece is
  asked twice at most, again after 3 s; 450 ticks without a reply start over, twice at most, then silence.
- s2c 0x04D (LAB_100a2f70 -> FUN_100a2dd0 for Command 1 / 2): Command @4, Result @5, kind @6, language @7, timestamp
  @8, size_total @0xc, offset @0x10, data_size @0x14, data @0x18; any check failing (timestamp, kind, language, the
  piece's offset, size_total over 0x400) ends it silently.
- FUN_100a3160: group 0xa 0x22 "<<< Welcome to %s! >>>" (the world's name), then the text by FUN_100a3240 (segments
  ending after BEL / CR / LF), all mode 200; printed every time, no "seen" check. LSB: one 236-byte piece per
  request, a repeated offset unanswered (servmesLastOffset_).
- Ours: the request also waits for the message tables; real time for the timeouts (retail counts ticks, at least
  one a frame).

## The log opens full at login (2026-10-23)

- Log window 1 (logwindo, name getter FUN_101618a0 in 30261006): its setters for the max lines, min lines and
  seconds per line (FUN_10162e60, FUN_10162ea0 and siblings) end in FUN_10162ef0: open ticks (+0x34) = ticks per
  line (+0x54) x max lines (+0x4e). The settings are applied at login, so the log comes up at its full height and
  shrinks from there (user-confirmed on retail). Per line FUN_10162560 adds one period (capped); Escape
  FUN_101624b0 takes one off.

## Auto-translate: the dictionary and the phrases in the log (2026-10-23; spec ~/ffxi/lab/research/autotranslate_spec.md)

- Dictionary: language slot 0x18 (English file 55665, ROM/168/25.DAT), loader FUN_101bb7a0: groups of a 0x4c-byte
  header (kind 2, language 2 for English, group id, 0, title[32] "【Greetings】", completion[32], u32 count, u32
  bytes) then entries (kind, language, group, phrase id, u8 length, text; languages 1 / 4 add a second string).
  42 English groups, 2685 phrases; @A / @C / @J / @Y texts name a place (group 0x14), spell, job (group 0x12) or
  ability by hex id. Ours: dat/autotrans.rs, loaded with the battle messages.
- A block in text is FD kind lang hi lo FD (kind 2 a phrase: group, id; 0 the group itself; 7..0x12 items, 0x13..
  key items, 00 bytes written as FF with kinds +1 / +2 / +3; 6 a variable block). Resolver FUN_101bc100 ignores
  the language byte (the client's dictionary answers). The log (FUN_10127c00 param 5 -> FUN_101ac990) shows a block
  as its text between EF 27 / EF 28 (the green / red brackets); an unknown block cuts the line there. Ours keeps the
  four inner bytes as U+100000 + 0x100 x position + byte through decode_sjis / encode_sjis and expands chat lines
  in push_chat_mode.
- Not done: input (Tab completion, menu fep, the category list), the item / key item groups built at run time, the
  coloured form in the line being typed, kind 6.

## Auto-translate input: Tab completion and menu fep (2026-10-23)

- The chat line (English direct path, FUN_101b5d10): Tab / Pause opens the list on the word at the end of the line
  (after the last space, FUN_101b7c20). Candidates (FUN_101b6ee0 -> FUN_101bba90 / FUN_101bbe60): the words whose
  key starts with the word, ignoring case, sorted by key, a text once, then the typed word and the names starting
  with it (tell history, party: FUN_1018d7f0); no word or no match: every group's title. Open: Tab / Space / Down
  next, Shift+Tab / Up back, PgDn / PgUp 7; Enter deletes the word (FUN_101ad0d0) and puts in the candidate (plain
  text, or the block as one unit when the line stays within 120 raw and 120 expanded bytes; a category reopens the
  list on its phrases); Escape / Backspace / Delete: from a category back to the root list, else close.
- With the list closed, Backspace on a phrase leaves its plain text minus the last letter (FUN_101abf40). The line
  draws phrases in the 7F 69 colour without brackets (FUN_101b5470). Submitting (FUN_101ad4b0): the first word's
  phrase is expanded; a chat command (table 0x1037ab98) keeps the rest's blocks, other commands get plain text.
- menu fep (FUN_1018f550): width the widest row, 90..200 + 12; "▲" at (+5) and "%d/%d" at (+0x23) on the header;
  seven rows 16 px apart; a row too wide cut with "...". Ours places it above the chat line at the word *(inf)*.
- Not done: the cursor inside the line (ours edits at the end), the item / key item groups (the 200-record item
  cache), the line history (Up / Down), macros' editor.

## Effect sound op 0x6b: the Sph name is at op+4 (2026-10-23)

- FUN_10048220 (link pass, all.c 53240): `case 0x6b: if (op[2] == 0) op[2] = *FUN_10072290(.., 0x4a, op[1])`: the
  chain's 4cc is word 1, word 2 only takes the resolved handle (emission 0x100501c9: +0x198 = *op[2]; per frame the
  element sits at the camera + the chain's nearest point, all.c 39069). Ours read word 2 (0), which matched any Sph,
  so every 0x6b sound rode the folder's first chain: 63 of 160 uses in DATs 100..399 (33 zones) on the wrong chain.
  East Ronfaure (DAT 201 f_ro/effe/sep1): skw1 se 2058 on kaw1 (north), skw2 se 2004 on kaw2 (east river), skw3 se
  2028 on kaw3 (the stream by the gate), far 40, looping; the waterfall steps sta / stb se 2003 have no 0x6b. A
  name missing from its file (DAT 170 'seal') leaves the sound at the generator.

## The log window's animation, as retail steps it (2026-10-23; spec ~/ffxi/lab/research/logwin_anim_spec.md)

- FUN_101626e0 (all.c 277148) each frame, ints in whole ticks (stock retail 30 fps: 2 a frame, FUN_10014cf0 whole, at
  most 20): fill; open -= t to min x T unless kept; slide -= t x speed (speed from the last frame); vis = ceil(open
  / T), lines drawn vis + 1 (- 1 typing); frame height eased 8 px a frame while more than 16 away, then 2, toward
  134 + (vis - 8) x 16 (6 with none; FUN_10162cc0, per frame, not per tick); offset ((slide % 20) x 16) / 20; the
  displayed line (+0x2c, the bottom row) moves one line toward the newest a frame while slide / 20 < the lines
  waiting, else slide = slide % 20 + 20 n; over 64 waiting jumps; speed 20 from 5 waiting; the frame background
  fades out over 20 ticks with no lines and in again. FUN_10162560 per new line: open + T, capped, rounded up to a
  period; slide + 20; the displayed line stays. FUN_101622e0 draws from the displayed line up at (x0 + 4, y1 + off -
  16 - 16 k), whole pixels, nothing with no visible lines.
- Ours had drawn from the newest line with a fractional offset (older lines jumped 16 px whenever two lines came
  close together) and float counters by real time; now logwin.rs runs FUN_101626e0 in frames of 2 ticks with the
  fraction carried. While typing retail moves the offset up 16 px as well as the clip (y1 and off), leaving a
  row empty above the chat line (confirmed on a retail screenshot, 2026-10-23); ours does the same.

## The world scene blends in gamma space (2026-10-23)

- D3D8 blended the frame buffer as stored (gamma); ours blended in linear space on an sRGB target, which cut faint
  additive effects to about half (Windurst Waters' canal water: effect generators WT01..WT10, t_wi/effe/mizu, mode
  0x48 additive, noon colour (0x0a, 0x28, 0x28) alpha 0x50 keyed by rtms / gtms / btms / mztm; it read almost black).
  Now the world draws offscreen into the scene texture through a plain (not sRGB) view: zone, actors, effects and
  the screen fades write gamma values and blend on them; the scaling blit reads the sRGB view, so opaque pixels are
  unchanged. The effects' scene copy (CMoDist) is read through a plain view too. Offscreen tools render to a plain
  Rgba8Unorm target (the same). Not yet: the title flyover, character select and creator scenes (still sRGB).

## Sound levels against retail (2026-10-23; spec in the session scratchpad volume/SPEC.md)

- Retail's chain matches ours step by step: the level table 0x103889f0 and pan table 0x103888f0 are our LEVEL /
  PAN_SCALE; FUN_10248920 cB = ((P x v >> 12) x 10000 >> 12) - 10000 -> SetVolume; effects T[level]
  (FUN_1024a7c0), music T[file vol] x T[level] >> 12; config 9 / 10 x 0.01 (FUN_10035e80 / FUN_100372a0); the
  ambient ftol(DAT_10351708 x 127); positional ftol(attenuated x DAT_10351704) (FUN_100364e0); the distance curve
  FUN_10036c70 is ours. The ambient files are mastered about 15 dB under the music (se 1005 rms 859, music 101 4528).
- Fixed: flag-0x20 files (every ambient loop: se 1001..1007, 1042, 1056) play at priority 20000 and are never
  stolen (+0xd0 = 0x40, FUN_1024a2c0); running effect voices are re-levelled when the effects factor or an event
  channel changes (retail compares the level after the factors); the event channels go back to 1.0 at zone-in
  (FUN_1008abe0). Not done: the resets at event end / an SE slider change (FUN_100ae3f0 / FUN_10035e80), retail's
  ambient crossfade (FUN_1024a1b0).
- Our own volume factors are gone (the user's call): the game's values alone set the levels, as retail. In game
  the Gameplay page's sliders (config 10 / 9) are the only volume controls (Settings' Sound page leaves them out);
  on the title Settings' two volume rows edit the lobby's one volume (0xa8).

## Weather changes blend the zone lighting (2026-10-23)

- s2c 0x057 (GP_SERV_COMMAND_WEATHER: StartTime @4, WeatherNumber @8, WeatherOffsetTime @0x0a Vana minutes; LSB
  zone.cpp 785 sends 4..28 at random, about 10..67 s) crossfades the weather objects in retail (FUN_101804d0 /
  FUN_10183780; the effect generators already blended by it). Ours switched the zone lighting (fog, sun, ambient,
  area lights) in one frame, so fog vanished at once about a minute after login when the server's change came.
  Now the environment of the old weather blends into the new one over the change's time (Env::lerp; the linear
  weight is inferred; StartTime not used: the blend starts on arrival). Live, East Ronfaure by the gate: !setweather
  3 (mist) then 1: still foggy 1 s after, partly clear at 15 s, clear with a blue sky after the 31 s.

## NPC shops: buy and sell (2026-10-23)

Full trace in `~/ffxi/lab/research/shop_spec.md`; ours in client/shop.rs.

- s2c 0x03E (ShopListNum @4) opens "menu    shopmain" (Buy / Sell, stacked on the log) unless a 0x20000 window is
  open: then group 7 0x45 and nothing (UI LAB_10210ca0). The store zeroes the 80-slot stock table (LAB_100f8c20).
- s2c 0x03C: Flags @6 bit 0 last; 12-byte entries from @8: price u32, ItemNo u16, ShopIndex u8 (+7 pad), Skill u16,
  GuildInfo u16. The client ignores ShopItemOffsetIndex @4 and stores by ShopIndex.
- Buy: "menu    shop" with the stock, Enter -> itemctrl quantity (max = min(stack, gil / price); no free slot:
  refusal 0x80) -> "menu    shopbuy" (cursor on child 2, Cancel) -> c2s 0x083 (body 0xc: quantity @0, ShopIndex
  @6). s2c 0x03F (ShopItemIndex @4, Count @8) -> 7031 0xe6 / 0xe7.
- Sell: the same list over the inventory; unappraised rows show "? G" (0x1038213c). The first Enter on a row sends
  c2s 0x084 (body 8: quantity 1, slot) as an appraisal; s2c 0x03D Type @9 = 0 fills the price map (Price @4, slot
  @8). A second Enter: quantity (stacks), "menu    shopsell", then c2s 0x084 (slot, qty) and 0x085 ([1,0,0,0])
  in the same frame; s2c 0x03D Type != 0 -> 7031 0xe8 / 0xe9. Grey rows (equipped, NoSale 0x1000) do nothing.
- The windows close when the player is more than 6.0 from where the shop opened (shopmain +0x14..+0x1c) or the
  zone changes. Item info for the cursor row draws in mode 1 at (130, 240).
- Live (Lnxcsfix, Ashene in Southern San d'Oria): bought a bronze dagger (gil 5000 -> 4838); appraised and sold a
  Distilled Water (2 G).
- Left: the yellow can-equip tint on buy rows (Skill / job lock), guild shops (0x082..0x086), the Up wrap at the
  list's top (not checked against FUN_101f67c0).

## Shops: the can-wear name colour and the list's paging (2026-10-23)

Notes: `~/ffxi/lab/research/shop_spec2.md`.

- Buy rows of gear (record level non-zero): green when an item of that id is worn (FUN_10131fa0: containers 0, 8,
  10..16), else yellow 0x80909040 when FUN_10132050 passes: s2c 0x119 seen; slot, race, job and level fields present
  (automaton items 0x2000..0x2200 have no race field, so never yellow); the race bit (own look +0xef) and the main-job
  bit; level <= the main job's level (retail reads the unsynced level, player +0x40c42 from s2c 0x067 @0x25, which we
  don't parse; 0x061 @0x0D is its own fallback); the superior level's low byte <= 0x061 @0x52; and none of the item's
  slots in the encumbrance mask (player +0x40df0 = s2c 0x01B @0x60, u32: equipment block in the low 16 bits, stat
  debilitations above).
- The base list (FUN_101f6c70) never wraps: Up / Down stop at the first / last row without a sound; plain Right / Left
  turn a page of 10 keeping the cursor row, and on the last / first page move the cursor to the last / first row (as
  items.rs page_keys).
- Live: Lnxcsfix (WAR 20) at Ashene: all seven weapons (levels 1..13) yellow; Right on the one page put the cursor on
  Bilbo, the last row.
- Still left: the scroll / dice / attachment colours (record +0x1554 kind, +0x1558 argument; spell names, 0x0AC bits,
  0x044 attachments), the requirement column (keytops3 192 + rank, windowps 212 + job; craft ranks from s2c 0x062,
  not parsed) and its grey rule, the sell list's settled flag (s2c 0x01D).

## Windows in WinBoat: software GPU and Helios (2026-10-23)

- Without GPU passthrough the VM has only WARP ("Microsoft Basic Render Driver", Dx12, Cpu): unplayably slow even with
  the first-start software defaults (background halved, anisotropy 1; config software_gpu_seen).
- WinBoat's pre-release Helios paravirtual GPU (a Venus Vulkan driver in the guest, virglrenderer on the host):
  - Vulkan: mesa-helios 162130a's vulkan_virtio.dll faults (null read) in vn_get_wait_semaphore_counter on wgpu's first
    vkQueueSubmit: a VkSubmitInfo that waits on a binary semaphore (the swapchain acquire) and signals a timeline one
    carries a VkTimelineSemaphoreSubmitInfo with pWaitSemaphoreValues NULL (valid Vulkan; wgpu-hal semaphore_list.rs
    fills wait values only when a wait is a timeline). A 16-byte patch of the DLL (~/Projects/helios/vulkan-virtio-patch)
    fixes it; with it the client runs. Stock Helios still crashes.
  - DX12 (its vkd3d path, "Helios vGPU", Dx12, DiscreteGpu): the device is lost while wgpu creates its 2048-sampler
    shader-visible heap (DXGI 0x887A0005), about 1 s after creation.
- WGPU_BACKEND (vulkan, dx12, ...) picks the backend; the Windows build logs to %APPDATA%\vana-client\vana-client.log.

## Guild shops (2026-10-23)

Full trace in `~/ffxi/lab/research/guild_shop_spec.md`; ours in client/shop.rs (the NPC shop's windows reused).

- s2c 0x086 {Stat @4, Time @8}: both guild tables' counts go to 0 first (store LAB_100eae80, even when refused).
  Stat 0 opens "menu    guildsho" (shopmain's layout) unless a 0x20000 window is open (guildsho itself included):
  group 7 0x45. Stat 1: group 10 0x19 "The shop is closed.\nThis shop's hours are %d to %d." (open = the lowest set
  hour bit, close = the first clear bit after it), Stat 2: group 10 0x1a with the group 0x10 weekday; both in chat mode
  0x79, silent when bit 31 (passive) is set and no guild window is open; then everything closes.
- Buy / Sell on guildsho fetch the list once per 0x086 (c2s 0x0AB / 0x0AD, header only) when the table has no rows;
  refused (SE 0004) within 5 s of the last 0x40 packet. s2c 0x083 / 0x085: 30 x {u16 ItemNo, u8 Count, u8 Max, i32
  Price} from @4, Count @0xf4, Stat @0xf5 (0x40 resets, 0x80 completes); duplicates dropped, server order.
- Buy rows: grey past the gil or at stock 0, else the NPC shop's name colours; orange while busy. Enter: affordable, in
  stock, a known item (no free-slot check); quantity up to min(stack, gil / price, stock); c2s 0x0AA {ItemNo, 0, qty}.
- Sell: the wanted list (title group 0xd 0xd "Guild Shop", help 0xe 0x33), row 0 group 9 0x2c "Select from your
  inventory." then the sell table without price-bit-31 entries, white only when held and Count < Max. Enter opens the
  inventory (row 0: all; an item row: that item only); there grey = unsellable or not on the guild's list; quantity
  up to the slot's count; c2s 0x0AC {ItemNo, slot, qty}; Escape goes back to the wanted row.
- s2c 0x082 / 0x084 {ItemNo, Count, i8 Trade}: the entry's stock takes Count (never re-requested); ItemNo 0 refusals
  (0xa3 / 0x9a / 0x9b), Trade 0 (0x81 / 0x82), else 7031 0xa5 / 0xa6 bought, 0xa7 / 0xa8 sold, a negative Trade adds
  group 7 0x83 / 0x84.
- Log lines break at '\n' (GameUi::wrap): group 7 / 10 lines such as 0x19 and 0x80 are two lines.
- Live (Lnxcsfix): Kueh Igunahmori (Southern San d'Oria, 3..18): all four views; bought 2 Distilled Water, sold 1
  through "Select from your inventory." (gil 4840 -> 4825). Doggomehr (Northern San d'Oria, 8..23) closed: "The shop
  is closed." / "This shop's hours are 8 to 23."

## Shops: the requirement column (2026-10-24)

- NPC buy rows' Skill / GuildInfo (s2c 0x03C @+8 / @+0xa) make a cell (FUN_1020f8a0): craft skill 0x30..0x3a with
  GuildInfo / 100 > 1 (LSB: (rank + 1) * 100) -> keytops3 sprite 192 + GuildInfo / 100, the rank's word (193 Amateur,
  194 Recruit, ... confirmed live: Recruit / Initiate / ... / Craftsman match LSB's woodworking stock), right edge on
  the column end, y + 2; job 0x40 + job above WAR -> the level (sprite font, right-aligned at the end, y + 4) and
  windowps 212 + job (the job's name) ending 24 px before; 0x80 -> group 0xc 0x11d + GuildInfo ("New wares!") in the
  game font ending at end - 2. The cell takes the name's tint.
- Grey (and Enter refused) when the craft rank is below GuildInfo / 100 - 1 (s2c 0x062 skill_base[skill] & 0x1f,
  copied from @0x80 + 2 * skill, only after 0x119 and never for 0x8000) or the job's level (s2c 0x01B) is below
  GuildInfo's low byte.
- Any row with a cell widens the list by 0x69 (FUN_10210880); cells end at window + 0x16e. Guild sources never have it.
- Live: Cauzeriste (`!gotoname Cauzeriste`, Northern San d'Oria) for Lnxcsfix (no woodworking): Amateur items plain,
  Recruit and above with their word and grey.

## Shops: scroll colours (2026-10-24)

- Buy rows of type 7 items with the teach kind 1 (record +0x1554, ours Item::extra 0xd54; argument +0x1558 = 0xd58, the
  spell id): the spell is found by name among the non-blue spells (FUN_1022b680 / b570: kind 6 skipped); green
  (0x80509050) when its bit is set in s2c 0x0AA, else yellow when the main or sub job can learn it (FUN_1022b7d0 =
  Actions::can_cast; the job-point input, s2c 0x063 sub 5, not parsed: 0). Then grey past the gil, as for gear.
- Not done: kind 2 (Corsair dice: green from the s2c 0x0AC bit id + 0x200, but FUN_100e6020 also needs player
  +0x2cf4a, set only by s2c 0x063 sub 4 Monstrosity data, which LSB sends only to characters with it) and kind 3 (PUP
  attachments: main / sub PUP, 0x119 seen, the s2c 0x044 UnlockedAttachments bit).
- Live: Sororo (Bastok Markets) for Lnxcsfix as WHM 20 (GM, 892 spells known): every affordable scroll green, Repose
  grey past the gil. Yellow not seen live (no character with the job but not the spell).

## Inventory settled flags (2026-10-24)

- s2c 0x01D (State u8 @4; LSB Flags u32 @8) with State 1 sets player flags (+0x3c35c) 0x800 and 0x200000
  (FUN_100ec6c0); every s2c 0x01E / 0x01F / 0x020 clears 0x800, and 0x01F / 0x020 for a storage container (1, 9, 2..7)
  set 0x200000, which nothing clears. The shop sell lists (FUN_1020ffd0, guild FUN_101ed150) have no rows while
  neither bit is set, so in practice only before the first 0x01D State 1 or storage update after login. Ours:
  World.inv_flags. Live: Ashene's sell list fills as before.

## Field manuals: op 0x43, plurals, windows on the log (2026-10-24)

- Event op 0x43 (server_update) has a sub byte: `43 00` sends c2s 0x05B mode 1 with Z1 as the option, `43 01` waits
  for s2c 0x052 mode 1 (the 0x05C params come first, into Z2..Z9). Our op table had it as a fixed op with no sub, so
  neither form matched: nothing was sent and the wait fell through, leaving every mid-event server answer at 0
  (field manual pages read "Target level range: 0～0", an empty monster line). Now Len::Sub. Live (East Ronfaure
  Field Manual, event 61): page 1 sends option 18 (bits 0..3 = 2, 4..7 = the page), LSB answers updateEvent(6, 0, 0,
  0, 0, 1, 5, 64): "6 members of the pugil family." (7842 + Z9), "Target level range: 1～5.", "Training area: East
  Ronfaure."; registering, Review (option 1 / 6: "0/6 members ..."), Cancel ("Are you certain?", "Training regime
  canceled.") all run.
- Dialogue codes `7f 86 n` / `7f 92 n` pick the `[singular/plural]` alternative by message work[n] (+0xc50) / num[n]
  (+0xc64): English the second unless the value is 1; Japanese and language 3 the second above 1 (the expander's
  -0x7a / -0x6e cases, then FUN_100bdad0). Ours always took the first.
- Region stacking: every window with registry flag 0x1000 sits on the chat log (frame top = log top - height - 2, at
  least 16; FUN_10159080 -> FUN_10158e10), not only comyn / resynus / the shops: query (event choices), gmtell,
  playermo, chatctrl, magselec, abiselec, myroom, rmpost, mogdoor, mogext, mogpost, roomlist, storage / storage2,
  jobchang, jobcsel / jobcselu, arealist, shopmain, guildsho, merityn, maplist, comyn, resyn / resynus / resynde /
  resynfr, magic, ability, emote, mount, rmlo1 / 2 / 4, plnt1..6, mp_pmode, mp_abisc. Flag 0x2000 (targetwi,
  ut_point, conq3pts, imperial, allied, colopoin) sits on the party window. Ours: GameUi::stack_dy, applied by every
  layout window helper while GameUi::log_top is set (world frames). Live: the field manual's query now touches the
  log as in retail.
- Not done: the query window's scroll bar (retail shows it when the options outrun the rows).

## NPC trade: the hand-over window (2026-10-24)

Spec `~/ffxi/lab/research/trade_spec.md`; ours client/trade.rs.
- "menu    handover" (FUN_10139840; the delivery layout: slots 1..8, 9 OK, 10 Cancel, 11 gil) with the inventory picker
  at x 200 and money, title group 0xd 10 "Trade". Reached from the main menu's child 7 and the action menu's command
  0x14 (shown by the Talk rule: an NPC target). A monster target: 7031 0x1c; no target: 7031 0x1d; FUN_101cc0d0(0x43);
  6.0 (3D) or 7031 9 "Target out of range." at open and every frame.
- Enter on a slot focuses the picker; a stack asks the count (itemctrl); an inventory slot already placed is taken out
  (picking it again under the cursor just removes it, FUN_10139e30). Picker grey: linkshells 0x201..0x203 and worn
  items (Ex, Rare and bazaar-priced are allowed for NPCs); placed rows orange 0x80804020. Child 11: moneyctr. Escape
  puts the cursor on Cancel first. OK sends c2s 0x036 (UniqueNo, counts u32[10] @8, inventory slots u8[10] @0x30,
  ActIndex @0x3a, ItemNum @0x3c; gil first as slot 0) and closes; nothing placed: SE 0004. An event closes it.
- Live: Lnxcsfix gave Endracion (Southern San d'Oria) a Distilled Water (1 of a stack of 2): his onTrade ran event
  1010 ("Listen, before you bring me something, you must first accept the mission!") and kept nothing.
- Not done: player-to-player trade (0x032..0x034 / 0x021..0x025, "menu    trade" + "menu    gift"), mouse in the
  hand-over, the target prompt when Trade is chosen with no target (FUN_10159440, group 0xe 0x2d).

## Player trade (2026-10-24)

Spec `~/ffxi/lab/research/trade_spec.md` section 3; ours client/trade.rs (P2p, World.p2p).
- Trade on a player (main menu child 7, action menu 0x13 on a player target): c2s 0x032 {UniqueNo, ActIndex} and
  group 10 0x14 "You offer to trade with %s." (mode 0x79); again within 60 s: comyn group 0xc 0x2d "Cancel trade
  transaction?" (Yes: 0x033 Cancel); after 60 s or out of reach a silent cancel; 7031 9 out of reach; nothing within
  3 s of a cancel. s2c 0x021: 7031 0x18 "{name} wishes to trade with you." and no window: choosing Trade answers
  (0x033 Start), whatever is targeted.
- s2c 0x022 Start opens "menu    trade" (the delivery layout; ours) and "menu    gift" (16, 144, theirs) with the picker
  and money, unless out of reach or a 0x20000 window is open (then 0x033 Cancel). Our offer draws from the 0x025 echo
  only; Enter on a filled slot clears it (0x034 count 0), the picker toggles an inventory slot already offered, gil
  goes in slot 0 (0x034 {n, 0xffff, 0, 0}); each 0x034 after our OK is preceded by 0x033 MakeCancell. OK = 0x033 Make
  (our name orange, keys only on OK / Cancel); the partner's Make turns their name orange; any 0x023 voids both OKs.
  Picker grey: Ex (0x4000) and worn items. Down from our bottom row goes to the gift, Up from its top row back.
- 0x022 Cancell / errors: 7031 0x1a "You cannot trade with {name} at this time."; End: 0x1e "Trade complete.";
  out of reach while trading: 7031 9 and Cancel; an event: 0x1a and Cancel.
- Not tested live (it needs a second account and window); unit tests cover the offer rule and the reset. Not done:
  the costume / blacklist checks, the Ex-by-augment and equipped-linkshell greys, mouse.

## Message formatter (FA 40 codes) and /clock (2026-10-24)

Spec `~/ffxi/lab/research/msgfmt_spec.md`; ours dat/msgfmt.rs.
- FUN_10153150 / FUN_10153180 -> FUN_10153a40 -> FUN_10153310 over group 8 / 10 strings: Shift-JIS pairs copied,
  `FA 40 <code>` interpreted: 0x82 choice (index arg, u14 count, end, offset table from after the code), 0x83 signed
  compare, 0x84..0x8a jumps (always / eq / !eq / gt / lt / ge / le, signed byte), 0x8b decimal, 0x8c string arg
  (0-based), 0x8d choice end, 0x8e item name (a / an + singular, or the plural), 0x81 nested group 0 (unused in
  English). Args (FUN_101531d0): `01 n` ints[n - 1], `02 n` literal, `03 lo hi` 14-bit. Padding and 12-hour forms are
  written in the strings (0x3f / 0x41). FUN_10153180's 5th argument = the number of leading string args; the int
  count is the XISTRING entry's i16 @+6 (not flags).
- Date / time rows by config 0x95 (date order: rows 3b/4d/48/43, 3c/4e/49/44, 3d/4f/4a/45; tables 0x1037d31c) and
  0x96 (24 / 12 hour: 3e/3e/4b/46/40, 3f/3f/4b/46/40; 0x1037d34c), both kind 2 and 0 by default. FUN_101c8fd0 = Earth
  local time (flags 1 long date with the month name g10 0x60 / 0x61, 2 date only, 8 no seconds); FUN_101c92b0 = the
  Vana'diel date (t / 0x12fc00 + 886, month, day) and time (0x46).
- /clock (FUN_1007cba0, command 0x3f): sprintf(g10 0x42 "Vana'diel: %s, %s, %s\n              %s (%d%%)", date,
  weekday (group 0x10), time, moon phase (group 0x11), moon percent) and g10 0x3a "Earth: %s", mode 0x8c; then a bare
  /clock flips the compass clock (DAT_10666e84 +0x2e: its time, day orb, weather badges), "on" / "off" set it.
  Moon percent (FUN_10103ba0): d = (t / 3456) % 84, v = (d - 4) * 2.3809524f rounded, d < 4: -v, d <= 46: v, else
  200 - v. Live: "Vana'diel: 1515/5/2, Earthsday, 2:47 / Waxing Crescent (21%)", "Earth: 2026, Oct. 23 20:40:18"; the
  compass clock went off and came back with "/clock on".
- Not done: event text codes 7f ab / 7f ac (dates through these builders), /playtime (g10 0x2e / 0x2f), the other
  callers (party join, merit prompts, auction house, level sync, /sea counts).

## Engaged state: the battle set, the gauge window, the lock-on cone (2026-10-24)

Spec `~/ffxi/lab/research/combat_mode_spec.md` (30261006 addresses).
- No separate battle mode: the window manager update (FUN_1015ef60) watches our applied status; turning 1 outside an
  event runs FUN_1015f3e0(0x20): the sub-target prompt is cancelled, every window without the battle keep bit (B bit 2
  in the registry) closes (main menu, item / magic / ability lists, config, shops, ...), then the flag-0x400 windows
  open focused: playermo (the action menu), gaugewin, targetwi. Leaving 1 runs 0x40: gaugewin, playermo, magselec,
  mp_pmode close. An event start closes them; they don't come back at its end. Ours: combat::battle_frame.
- The action menu rebuilds its rows every frame (FUN_1012b4b0 -> FUN_1012b2d0; the actionmN layout follows the count).
  Engaged layout 6: Switch Target, Magic, Abilities, Trust, Items, Disengage (Call for help while toggled by Right,
  FUN_1012acc0), Treasure (while the pool holds items), Sprint (area rule, not done), Check. Escape engaged doesn't
  close it (FUN_1012bd10: autorun off, else the log takes a line off); out of battle SE 0003 and close. Attack, Call
  for help, Disengage, Abilities, Magic, Check, Switch Target, Trust keep it; Chat / Tell / Talk close it; Items
  closes it and brings it back when the list closes (FUN_101336d0). Switch Target goes through the sub-target prompt
  over monsters (flags 0x20) and "/attack <id>". Right on Magic opens magselec. Hidden under a sub-target prompt; the
  hand only while it has the focus; drawn beneath the lists it opens.
- Title slot 0 while engaged: retail shows group 0xd 5 "Commands" (seen live twice by the user), although FUN_1012b4b0
  reads as 7 "Battle Mode" when +0x170 == 1: that field is not the engaged status (inferred). Ours: 5.
- Choosing Magic / Abilities / Trust / Items closes the battle menu (its close animation plays) and the list opens
  with its own; engaged, the menu comes back when the list (and any target prompt from it) is done (seen in retail).
- "menu    gaugewin" (16, 48), up only while engaged (draw FUN_101393a0): framesus 8 labels and troughs at (+4, +8);
  windowps 97 fills at (+25, +8 / +18) scaled by HP% / MP%, tints 0x80808080 / 0x8080e080, blend word 8; TP at
  (+25, +28): below 1000 scaled tp / 1000 in 0x800060b0, above a full bar in the pulse colour then (tp % 1001) / 1000
  in the next tier's (0x8000b060, 0x802060d0); numbers "%4d" right-aligned at x + 123, y + 7 / 17 / 27, 0xc0808080,
  TP capped at 3000. Pulse (FUN_10139660, per 2 ticks): crossing 1000 starts 1.0 rising, x1.25 to past 2.0, x0.75 to
  below 1.0; colour = tier base with alpha min(trunc(p 127), 255) - 127. While open the cast bar moves to y 94 and
  the macro palette 130 to the right.
- Region stacking applies through the window's registry name: the actionmN layouts stack as "menu    playermo"; the
  magic / ability lists and category windows (drawn by screens-level pushers) take the offset through
  GameUi::stacked_menu.
- Lock-on camera (FUN_1001ee60 locked branch): the eye may swing atan(4 / (near + d)) either side of the target-to-
  player line (all.c 23071..23080; x3 only in Monstrosity): about 35 degrees at melee range, not 106; near / far by the
  current focal length. With the battle menu focused the arrows move its cursor (camera only with Shift).
- Live: Lnxcsfix on Wild Rabbits in East Ronfaure: the battle menu, gauge and "Battle Mode" bar on engage, Escape kept
  the menu, Down moved it, Magic / Abilities opened over it, all closed at the kill.

## UI sounds and window animations along the menu chain (2026-10-24)

Audit `~/ffxi/lab/research/sound_audit.md`.
- Retail's generic window event method FUN_10118260 gives every window cursor 0001 (when the cursor child changed),
  Enter 0002 (0004 on a disabled child) and Escape 0003, unless its open hook clears one (+0x84 / +0x85 / +0x86, set
  by the constructor). SE functions: 10037b30 0001, b90 0002, bf0 0003, c50 0004, d70 0009 (target from none, sub-
  target confirm, NPC talk), dd0 0010 (target switch), e30 0011 (action menu opened), e90 0013 (focus to another
  window), ef0 0014 (main menu page opened), f50 0015 (spool next page), fb0 0016 (shutter), 10038010 0039 (netstat).
- Done: the main menu's row picks play 0002 (they read as a cancel before: the menu closing), a page swap 0014; the
  action menu's open 0011 and Enter 0002; every item list's Up / Down 0001 (list_keys reports a move), the item list's
  Escape 0003, the equipment grid's moves / Enter / Escape, itmsort2's moves. Left: event choices' moves / Enter,
  NPC talk 0009, the chat line's Enter / Escape, the full log's Escape, mogdoor, map0, the macro palette, the tell and
  `<call>` sounds.
- Windows placed by screens-level drawers went around the open / close animation (FUN_101177d0): the category windows
  (mgcmenu, abimenu, abiselec, magselec, commenu), the magic / ability / emote / mount lists, subwindo, the item list
  (and the direct-use list), the equipment windows (persona, money, equip, the candidate list), the picker beside the
  delivery box / trade / hand-over, money, the shop list. Now GameUi::gate_layout / gate_rect: each zooms in, and
  closes with its zoom when it stops being drawn (a list hidden for a target prompt too). A list that swaps its
  contents in place (an ability type list over its parent and back, an emote folder) plays its open again
  (replay_on_change; retail's close of the old contents is not drawn).
- Live: battle menu -> Abilities -> Job Abilities -> Escape: each step's close and open zooms seen.

## Skill actions: holds, op reactions, messages, the death after the hits (2026-10-24)

Spec `~/ffxi/lab/research/ws_reaction_spec.md`.
- The 0x028 handler plays no reaction, message or death for categories 3..6, 0xb, 0xd..0xf: FUN_100a9230 raises the
  busy counter (+0x11c) of the caster and every target until the effect file's "main" starts (FUN_100a9500 /
  FUN_100a9610; 1800-tick safety), and main's own ops drive the rest: op 0x20 holds actor2 for dur x speed (its
  server status waits: a kill's status 3 applies when the hold ends, about at the message), op 0x59 holds actor1 with
  no reactions, op 0x25 (actor2) / 0x21 (actor1) is the hit reaction at its frame (FUN_100cdd60: amount f32 @8, rate
  @0xc, count @0x10, blend-in @0x14, fade @0x18, side @0x1c; level = the current record's scale & 3; standing dfi /
  dbi by side (0 by position), moving dfm unless side 2), op 0x2b (DAT 0 `mdam`) prints the first record not yet
  printed (FUN_1018c810), and the rest print when the action ends (FUN_1018c620) or its file fails.
- Ours: fx/tasks.rs pushes holds / releases / reacts / action_prints (melee keeps combat.rs's plan); combat::one_action
  holds the caster and targets for skill actions and no longer prints or reacts at arrival; react_op; actmotion
  Overlay rate and blend-in.
- Live: Shoulder Tackle (DAT 33244) killing a Forest Hare: main -> op 0x20 / 0x59 -> op 0x25 at the hit (dfi, weight
  1, count 2, fade 37) -> mdam's line -> ded -> cor. Before: ded at the packet, the reaction once at once.
- Not done: knockback (op 0x5e), guard / parry ops 0x5a / 0x5b, op 0x71, the skillchain burst trigger.
- The action menu's key arm takes only arrows / Enter / Escape (it swallowed Space, so the chat line could not open
  while the battle menu was up).

## UI sounds: the rest of the audit (2026-10-24)

Spec `~/ffxi/lab/research/sound_audit.md`.
- Event choices (FUN_1014e410): SE 0001 when Up / Down or a click moves the cursor, 0002 on Enter.
- NPC talk (FUN_10158db0): SE 0009 once the talk packet went out.
- Chat line (text engine FUN_101b00b0): Enter 0002, Escape 0003. The full log's Escape 0003.
- Map menu (map0), Mog House door menu, macro palette: the generic handler's 0001 cursor / 0002 Enter / 0003
  Escape. Ctrl / Alt + H (key 0x3d): 0002 before /heal.
- Not done: the tell sound "0p40" (not resolved); the `<call>` sounds 0017..0023.

## Skillchains: the burst file, the proc message, load holds by count (2026-10-24)

Spec `~/ffxi/lab/research/skillchain_spec.md`.
- FUN_1006b2b0 writes 0x80 over the first op of DAT 0's global `proc` (op 01 on disk) at start-up; 2147 skill files
  call `proc` next to `mdam`. Ours patches the parsed ops (fx/tasks.rs sched_ops).
- Op 0x80 (FUN_10057fb0, FUN_100a8e30): record 0's proc kind -> DAT_1035bc3c[kind] (18 x u16: 0, 479, 480, 475, 476,
  477, 478, 467, 469, 474, 471, 472, 473, 470, 468, 1013, 1014, 0), file 0xaf0 + it, "main" on (actor1, record 0's
  target) with a copy of the action whose records are all printed (FUN_1005e380); caster and targets load-held until
  main starts.
- Op 0x71 n (FUN_1018c870, DAT 0 `mprc` / `wapr` / `sb0?`): message n (1 proc, 2 react) of the record op 0x2b printed
  last (action +10). Op 0x2b and the action's end print only main messages; a proc / react message no op 0x71 prints
  is dropped.
- Load holds are busy counts (FUN_100a4140 / FUN_100a9610) apart from the op 0x20 / 0x59 timed holds: a release no
  longer cuts a running op 0x20 hold (World.loads, combat::held).
- Live (Lnxtest WAR 15, H2H 40, Snipper in Valkurm Dunes): Shoulder Tackle twice -> proc kind 14 -> DAT 3268 on the
  target, "Skillchain: Impaction." / "The Snipper takes 10 points of damage." about 1 s after the WS line.
- Grammar kind 0x14 (FUN_1021d650 -> FUN_1021d8e0): field 1 of the status name entry (English adjective: "stunned",
  "KO'd"); kind 0x13 is field 0 (the noun). Message 204 "is no longer <adj>." printed an empty name before.

## On-screen damage numbers (config 0x13) (2026-10-24)

Spec `~/ffxi/lab/research/damage_display_spec.md`.
- FUN_100ad8f0: kind table 0x1035cae0 (1024 bytes; 0 none, 1 damage, 2 recovery, 3 miss), config 0x13 (0 Log, 1 Both,
  2 Screen). A kinded message is off the screen in Log mode and off the log in Screen mode; a kindless one is never on
  screen. Chat Filters (0x32 / 0x33) drop both. 0x029 / 0x02D only ever ask for the log channel (no numbers; their
  kinded lines vanish in Screen mode). Found by its first 0x40 bytes (same in the 2019 base and 30261006_0).
- FUN_1009e090 (0x028 record print, which 0 main / 1 proc / 2 react): needs caster and target to exist (no line
  otherwise); starts DAT 27 (resident with DAT 0, FUN_10072ed0) `damv` / `curv` / `misv` on (caster, target), on
  (target, caster) for reactions, task +0xa8 = the record's value of that message, +0xa4 = the line's colour for the
  relation (FUN_1009e700 -> FUN_10129c70).
- Kind 0x5c DamValueProg (FUN_10041f40 / FUN_10041af0 / FUN_10041c20): "%d" of the task number (":" = the "Miss!" art
  under `misv`), glyphs = "menu    dmgnum" sprites (char - 0x30) of file 39542, screen quads centred at the element's
  view origin + 1 toward the camera, (W, H) x plate scale x 0.00234375 per unit; diffuse = the element colour, or the
  message colour where that is 0x808080. Ours: fx Draw::DamValue / dk::DAMV (GLARE = rgb | count << 24, MORPH_W..+1 =
  glyphs), render/effects.wgsl vs_damv, variant 10 (the generic two-stage combine).
- Live (Lnxtest vs a Forest Hare): "5" / "4" in the dmgnum art under the target's name plate, the hare's hits over
  Lnxtest, `misv` on a miss.
- A zone change drops queued swings, messages and holds (combat Pending / rounds / holds / loads): a melee line
  printed after zoning with its target gone ("Lnxtest misses the .").

## Emission op 0x3c and op 0x45; the double "Miss!" (2026-10-24)

Spec `~/ffxi/lab/research/emit_3c_45_spec.md`.
- Emission op 0x3c (2019 0x1004de52, current 0x1004ee0e) and the element-death op 1 (FUN_1004d770) run the named
  generator **resource** (link step FUN_10047270: by 4cc in the folder scope): countdown +0xcc = 0, then its update
  (FUN_10048740 / current FUN_100496f0) once with the new element as parent. No clone, and the running clones of that
  generator are untouched. The resource's task link +0x64 is 0 (FUN_1003a800), so a kind 0x5c element it emits has an
  empty string and draws nothing (FUN_10041f40 writes ":" / "%d" only under a scheduler task). Clones (ops 0x44 / 0x53 /
  0x6a) copy +0x64 from a clone parent.
- Emission op 0x45 (2019 0x1004e929, current 0x1004f8e5), parent element only: copies the parent generator's attach
  bits and actors; flag 0x800000 clear and no frame of its own: borrows the parent generator's frame (elem +0x100);
  set: copies its matrix and scale vectors; then pos += the parent's local pos. *(Ours: only the pos add.)*
- `misv`: mis0 (red ":" at 0.40), mis1 clone from the scheduler (purple glow at 0.398, on the red text, no parent),
  and mis0's op 0x3c mis1 from the resource at 0.798, never drawn. Ours copied the number into that burst and drew a
  second purple "Miss!" below; `burst` no longer passes it.

## Per-record reactions from the files: guard, parry, block, counters, added-effect / spike lines (2026-10-24)

Spec `~/ffxi/lab/research/knockback_guard_spec.md` section 2.
- dam0 (T+5) runs by miss value 1 `sway` (voice only: no evade motion in retail), 2 `gurd`, 3 `pary`, 4 `gur1`, react kind
  63 `cnt0`, 1..10 `sb0?`; ranged `daml` has no block and no spikes / counter. Op 09 runs them on the target with actor1
  = target.
- Ops 0x5a (actor1) / 0x5b (actor2), FUN_100cde70 / FUN_100cdf00: guard `gdi?` / `gdm?` or (@0xe != 0) parry `pyi?` /
  `pym?` by moving, amount 1.0, rate @8, count @0xc, blend-in @0x10, fade @0x14; gate: no-reaction counter, riding; no
  dead check, no level, no direction, no fallback between the standing and moving names. Monsters' gurd / pary play a
  one-shot `gud?` (op 05). PC block (gur1) = the attacker's chit + pain voice + the guard overlay.
- Op 0x5c: the counter swing `cni0` standing, `cnf0` / `cnr0` / `cnb0` / `cnl0` moving, and the parent does not wait;
  its `dcnt` prints the react line (op 0x71 2) and runs `damg` on the original attacker.
- Ours now: ops 0x21 / 0x25 / 0x71 run in every task; melee / ranged tasks whose actor1 is not the attacker play their
  own motions and holds (fx/sched.rs Task::new), so monster guard / parry and counter swings show; combat.rs no longer
  plays melee / ranged reactions itself and prints only main lines at T+4 (proc / react lines from op 0x71: wapr
  T+21, sb0? +55, dcnt at the counter's hit). Melee / ranged ActionCtx records start printed (combat prints the mains).
- Live: hits on both sides react from damg's op 0x21 one tick after the line.

## Knockback: scheduler ops 0x5e / 0xbf (2026-10-24)

Spec `~/ffxi/lab/research/knockback_guard_spec.md` section 1.
- Level = the record's `scale >> 2` (FUN_1005e360; bits 0..1 are the hit distortion). Op 0x5e: mode u16 @8 (0 = the
  level from DAT_1035bc60: speed per tick, decay, constant ticks; level 0 = nothing; no action = 1), pull u16 @0xa,
  explicit speed f32 @0xc a second, decay @0x10, ticks @0x14 (also the bf0? count; 0 loops). Op 0xbf: the same after
  delay u16 @6 x task speed. Table found by its row 1 (dat/tables.rs KNOCKBACK).
- FUN_100aa940 on actor2: refused for applied status 2, 3, 6, 0x26..0x2b, 0x32..0x35, 0x38..0x3e and riding; velocity
  = normalize(reactor - pusher) x speed (any facing); motion `bf0?` with the pusher in front, `bb0?` behind (swapped
  for a pull); no-reaction and busy counters while it runs (ops 0x21 / 0x25 / 0x5a skip it; a knockback kill falls at
  the end).
- FUN_100aab70 per whole tick: move by the velocity (a pull stops 0.9 from the puller), decay once the constant phase is
  over, end at |v|^2 < 9e-6; FUN_100ca8c0: `bf1?` / `bb1?` once when the constant phase ends. Drawn position moved
  directly (no easing, no walk motion), the own player through the walking collision (0x015 tells the server);
  entities ease back to the server's position afterwards (LSB never moves them, inf).
- Monsters have no bf0? / bb0? motions: they slide without one.
- Live: the Forest Hare's Ram Charge (skill 266, knockback 2, file 3910) on Lnxcsfix: level 2 push, bf0 then bf1 at
  tick 5, then idle.

## Title buttons: sounds and the press animation (2026-10-24)

Spec `~/ffxi/lab/research/select_buttons_spec.md`.
- loby2win (FUN_102405f0): arrows / mouse onto a button 0001; Enter or click 0002 and the title locks (+0x1c, window
  +0x5d) until the screen changes; a disabled Select / Delete 0004 (Create never refused); Escape moves the cursor to
  Back with 0001 and does not leave. Config gives the title back when lobycwin closes (FUN_102406d0).
- Ours lost the 0002s when the lobby fade moved screen changes out of the UiSnap diff; now explicit. The creator's
  race-step Escape back to the title plays 0003 (FUN_10237730) for the same reason.
- Press animation FUN_10119fb0: window +0x5e = 9 on every Enter (generic handler), counted down by ticks; while > 0 the
  button sits +1, +1 instead of -1, -1; a taken press draws the button gold 0x80805020 and two gold rings, M = 0.625 N,
  D = (N - M) / 2, t = N - c, e = t - D k: scale 1 + 0.3 e / M, alpha 128 (1 - e / M), k = 0 the cell's blend, k = 1
  blend 0x44, centred by the whole scale (retail's off-centre quirk at W != 1024 kept). N = 9 on the title, 20 in
  generic windows (not done yet: the character list rows, Config pages, the login screen).
- The action does not wait: the fade-out starts on the press frame.
- title-shot: FFXI_TITLE_PRESS=c draws Select Character pressed at counter c.

## Actor sounds 1: voices, model init, sub-animations (2026-10-24)

Spec `~/ffxi/lab/research/actor_sounds_spec.md`.
- Every actor sound is a SeSep from a scheduler op (0x0a / 0x0b / 0x53 / 0x60 / 0x78), a generator sound element or
  the footstep code; motion chunks carry no sound track.
- Voices: scheduler sounds start through FUN_100370d0 -> FUN_10036130 and a full voice table (0029, 12) steals the
  oldest non-looping voice of lower or equal priority (FUN_1023fb90); only the plain call FUN_10037510 refuses at
  DAT_1044417c. At most 20 distinct SeSeps start a frame, repeats free (FUN_10036460; MODLOG's 12 was wrong).
- FUN_1008c480: once a model is in (kind not 3..5), `init` once (entity +0x128 bit 29), `inte` too for status 0x22,
  none seated (0x3f..0x53) or fishing (6 / 0x32 / 0x38; those run their status scheduler instead).
- FUN_1008ef60 every frame: entity +0x124 bits 1..3 = the wanted sub-animation (0x00E mask 0x04: @0x2A & 7 when @0x20
  bit 0 or u32 @0x28 bit 26; LSB animationsub), bits 4..7 applied (8 = none yet), +0x8a5 the model's own. Names
  `init ini1 ini2 ini3` by value & 3. Below 4: on a change the monster runs `ini<n>` unless first sight or already
  running; 4 <-> 0 only records; 4 and up (LSB spawns a sub-state as 4 | sub) stops `ini<applied>` and runs
  `ini<wanted>` once init ran.
- Tunnel Worm (model 424, DAT 1724): `init` = emerge (alpha 0, se 17024 at t30, `sp0?`, dirt, fade in), `ini1` =
  burrow (`sp1?`, se 17025 at t16, fade out by t112, invisible until the next init); LSB burrows with animationsub 1
  and surfaces with 0. `ntob` cries se 252001 on engaging; `dead` plays `vded` (252014..16).
- Live: every model's init on arrival; the Tunnel Worms near Lnxtest emerged with se 17024. A burrow (ini1) was not
  seen in 45 s.

## Actor sounds 2: status schedulers and the fall thud (2026-10-24)

- FUN_1008c6c0 on an applied status change: a model-id actor (not a PC skeleton) leaving battle runs `bton`, engaging
  `ntob`; a death runs `dead` (not done: `1ead`..`3ead` by entity +0x130 bits 5..6, `sdep` for trusts, the fishing
  names). Ours runs `dead` with its motions off: die() plays the same ded? / cor?. combat::status_schedulers, from
  apply_status and the entity update.
- Op 0x78 (correction: not a race death voice): the body hitting the floor, SeSep `0a` + actor1's footstep kind
  (+0x879) + '1' + shoe (+0x87a) in the zone folder its last footstep used (model +0x800: fses, or fser in the
  rain), at a snapshot, near f32 @0xc, far @8; nothing for an actor that never stepped. PC `dead` runs it at t92.
- Live: a Forest Hare's `ntob` played se 204001 when it engaged; its `dead` played `vded` se 204016 (no op 0x78 thud:
  the hare model has no footstep kind).

## Actor sounds 3: swing pick at op 0x24, whole schedulers behind motion-only paths (2026-10-24)

- FUN_100d4600 picks the melee swing inside op 0x24 once the attacker's files are in; ours picks at the packet when
  it can (combat::start_attack) and otherwise op 0x24 picks (retrying while the model loads; `atf?` moving, a random
  `ati?` standing) and the attacker's tasks play the swing motion and holds themselves (ActionCtx.client_swing).
  Before, such a hit had no swing, sparks, hit sound or grunt.
- Schedulers whose motions actmotion plays (event motions, the engage draw `in N` and sheathe `out N`, event steps
  `@tl0` / `@tr0`, synthesis results) now also run in fx with their motions off (actmotion::fx_too), so their sounds
  and generators come: `in 1` -> `sinr` -> se 6083 live.
- Not done (spec 4): the `shi<n>` / `ska<n>` sound variants and the swing letter by sub-animation (multi-form
  monsters), op 0x53's nearest-to-player target, `pop0` / `pop1` (0x05B / 0x065 modes 3 / 6), the entity +0x138
  hide skip, `?ead` / `sdep`.

## Status icons: "menu    buff" (2026-10-24)

Spec `~/ffxi/lab/research/status_icons_spec.md`.
- Its own window (object DAT_106671b8, registry A 0x93 / B 0x00400015): open in the field, out of battle, engaged and
  in events, closed on zoning / logout; not tied to gaugewin. Cells = the layout's children (142 + 2 + 26 (k % 9),
  48 + 2 + 26 (k / 9)), 24x24, up to 32 icons; the window's own 240x4 line is not drawn (the user's retail shows none).
- s2c 0x037: icon i = hi << 8 | lo from u8 @4 + i and 2 bits each @0x4C (none: lo 0xff with hi 0). s2c 0x063 sub 9:
  u16 ids @8, u32 end times @0x48 = (remaining + Vana'diel timestamp) x 60, wrapping; 0x7fffffff no timer; used only
  where the id matches the 0x037 id at that index. rem = (i32)(end - now x 60 + 59) / 60.
- Status info files: language slot 0x3a (English 87, 32x32, drawn x0.75) / 0x3b (39557, 16x16, x1.5) by config 0x99
  bit 2 (slots in current numbering: dat::msg::current_slot_file); 640 entries of 0x1800: the first 0x1fc masked
  (FUN_1021e870), byte @2 cancellable, @3 0xff never timed, string table @4 = the description; the icon @0x284 (32 bpp
  BGRA bottom-up; the size field @0x280 is not read). xiPrime replaces file 87 whole (its alpha runs 0..255).
- Icon at cell + 1; in its last 15 s it dims to rgb 0x40..0x80 on a 40-tick triangle (+0x288). Label (config 0xc3):
  "%dh" from an hour (at most 99h), "%dm" from a minute, seconds below, none at 5 s or less; fontshp x0.85 centred
  at (cell x + 14, y + 20), 0x80808080 (FUN_101c97b0).
- Live: Haste "21" (dimming at 12), Protect "1m", Regen "9m" beside each other at (142, 48).
- Not done yet: Num+ focus (cursor, name / description, Enter cancels with c2s 0x0F1), the party member icons (0x076).

## Status icons: Num+ focus and cancelling (2026-10-24)

- Num+ (key 0x78) cycles logwindo -> buff -> none; the buff step is skipped (and given up) with no icons.
- Focused (FUN_10227970 / FUN_10227bf0): the hand scaled 0.74 at the cell's cursor point - (5, 5); Left / Right one
  icon, Up / Down a row (SE 0001); title slot 2 = the status name (file 55725 field 0; each word capitalised,
  inferred for FUN_102588c0(2)), help slot 2 = the entry's description; Escape does nothing.
- Enter (FUN_10227eb0) on a cancellable entry (byte @2): c2s 0x0F1 {u16 id} and the icon goes at once.
- Live: Haste and Regen up, the bars read "Regen" / "HP are gradually restored.", Enter on Haste cancelled it.
- Not done: the party member icons (s2c 0x076, config 0xc2).

## Lobby: fade waits, the list press, the prompt, login sounds (2026-10-24)

- Lobby fades: the next screen starts 34 frames after the fade-out began (FUN_1023e5b0; 68 ticks at the lobby's
  30 fps), and the faded-in screen takes no screen change for 8 frames (FUN_1023e550; 16 ticks). Ours had the two
  waits swapped (60 ticks of fade, then 8 of our frames).
- dbnamese press (generic handler +0x5e = 9 on a used row only; FUN_101177d0 -> FUN_10119f80): the row sprite 77 and
  its name sink to +1, +1 for 9 ticks with the tail of the N = 20 rings; the cursor row's name otherwise sits at -1, -1
  with its sprite (FUN_10241a10).
- ptc6yesn (FUN_1023d050, user: copy retail): Left / Right 0001 every time, Up / Down nothing, Enter 0002 for Yes and
  No alike, Escape nothing.
- Our login screen (not retail): the generic window sounds, 0001 on focus moves and Escape (to Exit), 0002 on an
  enabled action, 0004 on a disabled one.

## Status icons: party members (2026-10-24)

- s2c 0x076 (FUN_10098f70 / FUN_10098f90): 5 x {u32 UniqueNo, u16 ActIndex, pad, u64 bits, u8 icons[32]}; icon i =
  (hi << 8 | lo) & 0x3ff with hi = 2 bits from the u64, none when lo 0xff with hi 0; LSB sends members in our zone,
  not us.
- The party icon object (DAT_106671bc, draw FUN_10228440, config 0xc2), only while the buff window is up: the target
  window's target when it is a party member other than us (name in "< >"), else the party cursor's member; icons 8 a
  row at 14 px (24 x 0.6) from (log x + 200, log y - 8 - rows x 14), above the party window instead when x + 144
  reaches it; the name 16 above in fontshp, 0x80808080 in our zone, else 0x80404040 (FUN_10202cc0, inferred).
- Untested live (needs two characters in a party).

## Actor sounds 4: sub-animation variants, op 0x53, pop0 / pop1 (2026-10-24)

- +0x8a5 (the model's sub-animation): `shit` / `skaz` SeSeps take it (& 3) as their last digit when such a SeSep
  resolves (all.c 68810..68830), and FUN_100d4600's swing names take `"t123t123"[sub]` as their second letter
  (combat::swing_patterns; `ati?` / `atf?` for 0).
- Op 0x53: the action target nearest our own actor (FUN_1005e300 -> FUN_10081550), actor1 without a player.
- 0x05B / 0x065 Mode @0x16 (entity +0x144, FUN_1008f740): 3 runs `pop0`, 6 `pop1` on the actor.
- Left: the hide skip (entity +0x138 bit 31 with model +0xb2 bit 1; +0x138 not traced), `?ead` / `sdep`, fishing.

## Chat call sounds (2026-10-24)

- FUN_10127990 (from the chat line print FUN_10127c00 with the text and the line mode byte): modes 5 / 0x0d (party)
  with a tag "<...>" not wrapped in quotes that matches a row of DAT_1036eba0 (0x14-byte rows: name, u32 byte-swapped
  SeSep name) play that SeSep through FUN_100380f0 (one at a time): <call> / <call0> 0017 .. <call21> 0038. Rows with
  0x800000 / 0x400000 (<ncall>...) run FUN_10127ab0 first: not done. Linkshell lines are not checked.
- Tell (mode 0x0c) with config 0x47: SeSep "0p40" (0x30703430 byte-swapped), in no resident DAT (0, 27, 52743),
  so retail finds nothing either *(inf)*; not played.

## Engaged menu: Check through the prompt, Conflict rows, Call for help block (2026-10-24)

Spec `~/ffxi/lab/research/engaged_menu_spec.md`.
- FUN_1012b880 Check: engaged, the sub-target prompt over monsters first (FUN_10159440 flags 0x20, LAB_1012bec0);
  otherwise at once. FUN_101347b0 (also /check, /checkname): the id or the target; not the same id again within 2 s
  (DAT_105781c4); c2s 0x0DD only for a player or a monster (+0x120 bit 13), any other NPC nothing. Check is hidden on
  a pet, trust or charmed monster (FUN_1008e150: not a PC, team byte 1).
- Sprint (cmd 0x18), Scout (0x17), Quarry (0x16): Conflict only (FUN_101cc350: our team byte +0x1ff = s2c 0x037 u32
  @0x34 >> 21, 2..99 but 8 / 9 / 0x28..0x2b); Treasure is hidden in Conflict. The rows run /quarry /scout /sprint
  (ids 0x8c..0x8e): c2s 0x01A on us with 0x15 / 0x17 / 0x16, else group 7 0xa7. LSB sends team 1: never shown.
- /blockhelp (0x96, FUN_1012ad00): on / off / toggle, group 9 0xf7 / 0xf8; while set, Right on Disengage prints 0xf7
  with SE 0004. The Call for help toggle itself plays nothing; Switch Target with no candidate plays nothing.
- /say carries flags 0xc3 (not allowed while KO'd, group 7 0x4f); /tell and /party 0xe3 are.
- Live: engaged on a Snipper, Check -> prompt -> one "The Snipper seems incredibly tough." line; a second Check within
  2 s sent nothing.
- Not done: Treasure's `menu    loot` window, the mouse hold (60 ticks on a row with a Right function), the party-list
  cursor cancel on engaging, the inspect window (s2c 0x0C9).

## Text commands: toggles, /ver, /playtime, /chatmode, /localsettings (2026-10-24)

Spec `~/ffxi/lab/research/textcmds2_spec.md` parts A and B (handlers, config ids and group 9 lines per command).
- Config toggles through `cfg` (set, save with cnf.dat, cfgapply): /ignorepet 0x97, /ignorefaith /ignoretrust 0xaa,
  /aoe 0xc0, /hidetrust 0x99 bit 5, /emotetrust bit 9 (set = not linked), /furcc bit 6 (set = off), /layoutctrl 0xc1,
  /statusparty 0xc2, /statustimer 0xc3, /gtst 0xc4 (atoi, 1..100), /battlebgm bit 0, /bellsw bit 1, /yellsw
  /assistjsw /assistesw chat filter bits 0x2e / 0x31 / 0x32. Two argument styles: on / off with a report when no
  word (FUN_10079170), and /battlebgm /bellsw /blockhelp /blockaid /mutebgm /mutese compare the word only when it is
  the one argument (token count 2).
- /blockaid: c2s 0x01A category 0x18, param off 0 / on 1 / else 2; LSB answers with its own lines (live: "Blockaid is
  currently active." then "... inactive.").
- /names (FUN_100790e0): session state, cleared at zone-in (inf); off hides every plate, "otherpc" (the last word)
  only other players'.
- /mutebgm /mutese: session factors multiplied into the configs 10 / 9 volume (DAT_103517f8 / DAT_103516fc).
- /ver: "version %s" in mode 0xbf; retail reads patch.ver, ours prints the lobby version string (30261006_0).
- /playtime: s2c 0x00A u32 @0xA0 plus the seconds since it came; group 10 0x2e or 0x2f (with days), mode 0x8b. Live:
  "Total time played is: 2 hours, 48 minutes and 21 seconds."
- /chatmode (FUN_10080830): the prefix for a line with no command (was always "/say "); tell needs a name (name_case,
  else group 9 0x65 and failure); no argument prints "(chatmode : prefix)" in mode 0x9d.
- /hide: the away flag cleared (as /online) and Msg std 0x79. /localsettings: shadows 0x3a, charanum 0x3c (25..50),
  distance 0x3e (N * 2, 0..25), footeffect 0x3f, hidearmor 0xa5 (0xff / 0x100), blureffect 0xc8 (0x145 / 0x146).
- Ours, not retail: a /say whose text starts with '!' (LSB GM commands) skips the K.O. / mounted / event gate.
- Not done: the consumers for /ignorepet, /ignoretrust, /hidetrust (our entities carry no pet owner or trust flag),
  /battlebgm (party battle music), /bellsw (scheduler op 0xb3), /furcc (furniture collision); /displayhead,
  /partyinfo, /jobmasterdisp, /focustarget, /autotarget.

## Model-file generators: the stop match, var 0x3a, op 0x9a (Raminel's dust, 2026-10-24)

Spec `~/ffxi/lab/research/model_gens_spec.md`.
- FUN_10083970 [FUN_10085060] restarts a model file's 0x1000 generators when the file comes in (before `init`) and on
  un-hide, gated by vtable +0x124 = entity +0x130 bit 1 (FUN_10084a10, the same in all 16 actor vtables). The bit is
  set only for ship ids {0,1,2,5,6} and by scheduler op 0x9a (u32 @8 & 1). So skeleton NPCs pass, and Raminel's km00
  (model 2474, file 52769) starts in retail too.
- Retail then stops it: `init` -> op 0x57 `mse0` -> var 0x3a == 1 ? `ids1` (`strt`) : `ids0` (`stop`, op 0x2d km00).
  var 0x3a = actor1's sub-animation (vtable +0xa4, +0x8a5 & 3), read in FUN_10061950 before any action. 361 NPC
  model files use this ids0 / ids1 pattern; the names are model-local (nothing in FFXiMain builds them).
- FUN_10052a50 (op 0x1e / 0x2d stop): actor1 null = every clone of the generator; else clones on actor1 whose actor2
  is null or equals the task's actor2 (asm 0x10052b38..51). Ours had the actor2 test reversed, so `init`'s stop (a2 =
  the actor) never reached the (actor, none) model clone.
- Fixed: stop_generator's match, var 0x3a without an action, op 0x9a as an entity flag that keeps model_generators
  off. Live: Raminel walks without the smoke. 257 model files had stops that silently missed and now work.
- Not done: the un-hide restart (FUN_10084320 re-runs FUN_10083970; ours never restarts).

## /pcmd live test with the server bots (2026-10-24)

- LSB bots (another session of the user's) log in when a real player does, up to 15 over 3 minutes, and accept party
  invites. Tested as Lnxtest in Southern San d'Oria: /pcmd add (two bots joined), kick (the member left, the plate
  went back to the normal colour), looter, breakup (Solo), leader (the leader dot moved to the bot), leave (Solo).
- LSB sends the inviter no line on a sent invite or a join (0x06e_group_solicit_req, 0x074_group_solicit_res): the empty
  chat log is the server's.
- Not done: /pcmd add NAME for someone outside the zone (FUN_1007c740, a search server name lookup).
- Test tool: script step follow:PATH runs lines appended to PATH as scripts until a line "end", so a live run can be
  steered once the bots' names are known.

## The Mog House door and the visibility cull (2026-10-24)

- The client-made "Door: Back to Town" (entity +0x12c bit 17, set only by FUN_101d2cc0; vtable +0x148) is never culled
  (tabcull spec step 6). Ours had made World::visible a candidate rule (5596cc4) but never put the door in it: it has
  no model and door 0, so Tab and the mouse skipped it and nobody could leave the Mog House. Now always visible, and
  the mouse pick gives it the doors' 0.5-tall box (FUN_100ad320; it is an XiDoorActor, kind 3). Confirmed by the user.
- Test tool: `act:goto:X,Z` adds a walk waypoint (as --goto) from a script.

## Search: the search server client and name lookups (2026-10-24)

Spec `~/ffxi/lab/research/search_spec.md` (protocol 1.1..1.8, grammar, windows, other users, order).
- net/search.rs: one TCP connection per query to the lobby's search address (MapLogin.search, LSB xi_search 54002);
  request = 0x10 header ("IXFF", data length, 0x80, wire type) + payload padded to 64 k + session word, MD5 of
  [8, P-8), salt; Blowfish (the map server's variant, no zero cut) keyed MD5("0s=m<1IZ2zBCc8{~" ++ salt) over
  [8, P+8). Replies keyed MD5(fixed ++ salt echo ++ session word), MD5 checked, entries `[u8 n][bits]` decoded by
  5-bit type (FUN_101061d0) into the 0x48 record; read until reply +0x0a bit 7 (LSB sends every page at once and
  answers retail's "more" with empty auction packets), then close. Conditions so far: all areas, one area, name
  only. Unit test: the spec's fixed vector (request bytes exact, reply decrypt + 3 records); an ignored live test.
- FUN_1007c740 name lookup (client/search.rs): name normalised (first upper, rest lower, 15 chars), all areas, name
  only; the callbacks take only an exact name match (LSB matches a case-insensitive prefix).
- /pcmd add NAME, /acmd add NAME with nobody by that name in the zone (LAB_1007d5b0): c2s 0x06E {UniqueNo rec+0x18,
  ActIndex 0, kind 5 when the record has flag 8 (party leader), else 0} for both commands; no match: silent. Live:
  Lnxtest in Southern San d'Oria invited Rokuro in East Ronfaure, who joined.

## Party rows for members in other zones (2026-10-24)

Spec `~/ffxi/lab/research/party_zone_spec.md`.
- In our zone (FUN_10202cc0, also FUN_10202530 / FUN_10202720 / FUN_102033c0): the slot used, record zone (+0x2e) ==
  ours (G+0x3c350) and marker (+0x30) == 0. 0x0C8 writes zone and marker together when the zone is new or changed;
  0x0DD sets the marker to its ZoneNo (@0x20); 0x0E2 sets the zone and clears the marker; 0x0DF touches neither; a
  0x0DD / 0x0E2 with an empty name is dropped whole. LSB's 0x0DD for a member elsewhere carries ZoneNo only (HP, MP,
  TP, ActIndex and jobs 0).
- Party window away row: the name in 0x80606060 and "(%s)" (0x10382120) with the short area name (FUN_101cbc10,
  group 4) right-aligned in fontshp at (+0x69, +0xb), no gauges; alliance rows: the grey name only. The gauges ease
  only for rows in our zone. The party status-icon name turns 0x80404040 by the same test.
- Fixed: our text pass counted rows per text kind, so after an away row every row showed the previous member's name
  and HP; the slots now carry their row. The party cursor's hand on an away member keeps the base tint 0xa04040d0
  and flashes nothing (it matched entities by a stale ActIndex).
- LSB bots teleport to their leader at once (modules/bots/bot_ai.cpp), so an away row needs a bot leader. Live:
  Gavrel (leader) and Korrin in Southern San d'Oria, Lnxtest in West Ronfaure: "Gavrel (SSandOria)", "Korrin
  (SSandOria)" grey, the leader dot on Gavrel.

## Search: /search and the results window (2026-10-24)

Specs `~/ffxi/lab/research/search_spec.md` (2, 3) and `~/ffxi/lab/research/scresult_draw_spec.md` (drawing; corrects
search_spec 3.2: 15 visible rows and 7 columns, page by 15, the zone cell plain "%s", "Search" in the title bar).
- /search /sea (0x3a, FUN_1007c570): nothing while the results window is up; no argument = the current zone, ALL =
  every area (the full grammar FUN_10104f00 is not done yet). FUN_1020a9d0 mode 1: the window opens on " [Searching]".
- scresult: layout (16, 48) 366 x 248, no anchor bits, registry flags 0x30000 (no placement meaning: only 0x100000,
  scaling, feeds FUN_101193f0), and FUN_1020bf90 does not move it: top-left like scanlist. Rows at (x + 4, y + 4 +
  16 k), columns {0, 0, 18, 146, 150, 238, 270, 300}. Header: windowps 323 and " [%d]" in shape font at +150,
  0x60808080. Person row: status icon (keytops3 by flag priority), job / level cell (pname brackets, jobs 1..22,
  digits 30+, race 40+, nation crest 27+, rank; Num+ the master-level view), comment dot, name fitted into 80 px
  (ncol 5 leader, ncol 1 party member), language letters, the short zone name when not ours. Cursor bar windowps 75
  while focused; scroll bar (menu scroll 0..3) past 15 rows. "Search failed." (7 0x65) in the game font at (+36,
  +28), 0x80804040.
- Chat: group 10 0x05 / 0x35 / 0x06 with the total, then 9 0xbf when fewer rows came than were found; both mode 0x79.
  Title slot 2 group 0xd 0 "Search"; help slot 2 the header's 0xe 0x13, mentors 0xe 0x259.
- scoption (top right, anchored): Tell (the chat line on "/tell Name "), Befriend (dim: the Friend List is
  PlayOnline's), Invite (0x06E, kind 5 to a leader; dim on a party member, a member who is not a leader, recruiting,
  MON, ourselves), "/partyrequestcmd add / remove Name" (remove dim on ourselves).
- Live (Lnxtest): /sea "3 people found in this area", /sea all "10 people found in all known areas" with the bots'
  rows (BST14 Hum, zones Pashhow, Konschtat ...), scoption over a row, Tell opened "/tell Kakaru ".
- Not done: the grammar and its tables, scsibori / searchma and the criteria windows, the comment fetch on Enter,
  acoption / /mutelist, /recruitlist, the LS member list, s2c 0x008 known-zone bits.

## Search results: first cursor row (2026-10-24)

- Retail (vanilla, the user's screenshot) shows the cursor on the first person once the rows are in, not on the header;
  ours now does too. The window position (16, 48) and the drawing match that shot with mods off; with the xiPrime menu
  pack (it replaces ROM/119/51, the in-game menu DAT, including menu ustats and the windowps bands) the status icons
  turn into green arrows, the header band reddish and the cursor bar whitish.

## Cast tones and the sound grace (2026-10-24)

Spec `~/ffxi/lab/research/cast_fx_spec.md`.
- The cast-start tones are generator sound elements: neri gn2s se 5032 (25.3 s, no loop), gn1s 5015, gn4s 5037,
  ner5 5039, element life 30. st?? / sp?? kill them with op 0x1e on (caster, caster); the clone is (caster, caster)
  (FUN_10052710 takes the task's actor1 / actor2 as they are, asm 0x10058b4c..0x10058b60).
- FUN_10083d60 stores its argument after running a stored st?? (asm 0x10083d83). The decompile's 0 is a stack
  misread: +0x298 takes three arguments and returns with RET 0xc.
- Sound element grace +0x1e4 = 3 passes, -1 a pass, once a frame at retail's 2 ticks a frame = 6 ticks; ours counts
  ticks (it counted frames: at 300 fps the grace ran out before our mixer had taken the voice, so the element
  dropped it as finished).
- FUN_10035cd0 fades the voice (cmd 5, element life) whenever a generator's sound element goes, however it went;
  ours now does too.
- Our audio counts a voice alive from the play request until the mixer has taken it (ours, for the callback
  latency; a failed read counts as taken).
- Op 0x11 follow lerps per 2-tick frame (k^(dt/2)).
- `switch_zone` clears the stored casts *(inf: retail rebuilds the actors)*.
- Tests: FFXI_AUDIO_DEVICE=null is a silent output mixed at real time. ALSA's null device is not paced (cpal's
  thread spins a core and every sound ends within milliseconds), so sound timing could not be checked under it.
- Live (Lnxcsfix WHM 20, Cure on self, null output): 5032 starts with the cast and gets its 30-tick stop at the
  finish (2.07 s later) and at an interrupt (category 8 `spwh` after walking off). The Cure sound 3037 starts twice
  in one frame from ops 0x53 and 0x0b; FUN_10036460 lets the same resource start again in a frame, so retail does too.
- Left: F5 (a file that fails to load blocks the +0x298 lookups), F7 (the start cap per pass; the two specs
  disagree: 12 a pass in FUN_10035640, 20 distinct a frame in FUN_10036460).

## Bard songs: ops 0x76 / 0x77 and the range slot (2026-10-24)

Spec `~/ffxi/lab/research/song_sound_spec.md`.
- caso / shso run op 0x76 / 0x77 (asm 0x1005adb5): `lc` / `ls` + two digits of actor +0x886 (the range part file's
  info byte 0xe, FUN_100d0b50 case 8; vtable +0x3d8 = &+0x878) on actor1's own files, a plain child; mode 1 above 2
  -> 0, mode 2 not 11 -> 10, nothing while moving (+0xf8).
- lc00 sings (face file `vsng`: se 21003..21008 by race / sex), lc01 wind `sefl` se 21001, lc02 string `sehp` se
  21002, at t60; stso t30 `kf&h` / `ksng` cut them.
- Retail loads all nine look slots, id 0 included; ours now loads the range slot (class only, its wep2 mesh left
  out: `init` hides it). Without a range file the first part file's info byte 0xe >= 0 is the class (FUN_100cf210).
- Live (Lnxcsfix BRD 20, Valor Minuet on self, null output): se 21005 from 1 s into the cast until its stop 0.5 s
  after the finish, then 20069. Before, the song was silent until the finish.

## Scheduler op 0x05: the motion rate (2026-10-24)

Spec `~/ffxi/lab/research/raise_anim_spec.md`. Corrects "Scheduler op 0x05 (motion) and what ends it".
- FUN_100cb8e0's second argument (op 0x05 f32 @0x10) is the queue entry's weight (+0x1c; FUN_1001a400 blends it into
  the region queue +0xc, which only the overlay regions 5 / 6 use as their pose weight), not a speed. The third is
  the rate (+0x28 via FUN_10019a40). FUN_10019ab0 advances `frame += ticks x speed(+0x36) x 0.5 x rate`.
- FUN_10057000 case 5 (asm 0x1005bc95..0x1005bcd9; 30261006_0 0x1005cc45..0x1005cc83): `rate = 2 x frames /
  speed (the first matching chunk: the scheduler's own scope, else the actor's files) / (i16 @6 x task speed)`, or 1
  when that length is <= 0.01 or no chunk is found. So an op 0x05 motion always spans its u16 @6 x task speed
  ticks. Region-digit names take the same rate unguarded (FUN_100cb4a0).
- u16 @0x1c: entry +0x20, the fade-out ticks when the count ends in regions >= 3 (FUN_1001a760). u32 @0x1c >> 28:
  1 = the first half (wrap at (frames + 1) / 2 - 1), 2 = the second half (start at (frames + 1) / 2): `@tr0` /
  `@tl0` walk steps (not modelled).
- Raise: 0x028 category 4 actor = target = self, animation 511 / 512 / 496 / 847 (files 3311 / 3312 / 3296 /
  3647). `main` holds actor2 (op 0x20, 230..320) and runs the skeleton's `stnd` on it (op 0x2e lock 176; op 0x05
  `std?` 200 ticks: the float-up). Elvaan M's std is 51 frames at speed 1 (102 ticks natural), so it plays at rate
  0.51. LSB sets status 0 2 s after the accept; the hold delays it past `std`.
- Op 0x2e (FUN_10061360) / 0x2f (FUN_10061500): timed actor locks, vtable +0x30c / +0x314 for dur x speed (+0x30c
  = FUN_100a8e80, counter +0xfc). The end of op 0x2e runs FUN_100c8970: +0x7d8 is cleared unless the status motion
  is idl / dl1-3 / ltb / lt1-3 / cor / rx1.
- Ours: actmotion::op_rate; one-shots end by real ticks; the engage and death motions take the same rate.
- Live: the Hume M raise `std` ran 3.35 s (200 ticks), as the user had seen it too fast before.

## Window groups, region stacking and the raise prompt (2026-10-24)

Specs `~/ffxi/lab/research/raise_prompt_spec.md`, `mpcost_spec.md`.
- Menu registry (30261006_0: 0x103722d8, 0x2c-byte rows): name, handler, u32 flags @0x24, u8 state @0x28, u8 layer
  @0x29 (draw order), u16 group @0x2a. Flags 0x1000 / 0x2000 / 0x4000 / 0x10000000 put a window in region 0 (on the
  chat log), 1 (on the party window), 2 (on the target window), 3 (query); masks 0x1037629c. 0x20000 marks the
  main-window class.
- FUN_1015fa00 suspends a lower window whose group bits an open higher one holds: its close animation plays, then
  its open when the bits are free. Groups: scresult 0x1c, scoption 0x81, inspect 0x10, mapframe 0x5e, gaugewin 0x10,
  playermo 0x04, targetwi 0x02, buff 0x40. So the search results hide gaugewin and the action menu, not the status
  icons (they draw under it, layer 0 against 2); the map hides the icons.
- Region 2 (only subwindo, the MP Cost box): on the target window when it is up, else on the party window; regions
  restack 0..4 every frame. subwindo's text is fontshp throughout, labels too; MP Cost and Next on one line each.
- FUN_101626e0 holds the log (no decay, Escape does not shrink it) while a flag-0x1000 window is open
  (FUN_1015ecc0(0x1000)): the raise prompt, comyn, the event choice, shops and lists. When the held log changes
  height with the pointer on such a window, the pointer moves with it (asm 0x10162a52); ours shifts the drawn
  software cursor (no warps on Wayland). Stacked windows sit on the log's bottom (h - 16) while it shows no lines
  (FUN_1015f790).
- resynus (FUN_102110c0, FUN_10125360): the hover moves the cursor only when the child under the pointer changes
  (SE 0001); the release confirms it (FUN_1015de50, SE 0002); Up = Accept, Down = Decline, no wrap; Escape is
  refused (+0x39 = 0: SE 0004) and does not reach the log or the target.
- Status icons: the focus box is the window's cursor, anc_equi at 0.74 over the icon (FUN_10227970); placed on the
  icon as in the user's retail shot. Escape leaves the focus (seen in retail).
- Arrow keys go to the camera only while no window holds them (`world_window_open`, palettes, the log focus, the
  K.O. window and the raise prompt, the event choice and input).
- F12 lines: the windows holding the keys, log focus, lock, party cursor and the held DIK codes (for the intermittent
  stuck-input reports).

## /check on a player: "menu    inspect" (2026-10-24)

Spec `~/ffxi/lab/research/check_spec.md`.
- /check on a PC sends c2s 0x0DD; LSB answers s2c 0x0CA (bazaar message, name, title), then 0x0C9 kind 3 (equipment,
  8 a packet) and kind 1 (jobs, linkshell). 0x0CA prints group 10 0x33 "%s's title: %s" in chat mode 0x29 (not
  for our own name) and arms View Wares.
- The first 0x0C9 after the check opens "menu    inspect" (16,48 182x190) with an empty iteminfo; later packets fill
  the store, the window reads it every frame. A second check is ignored while it is up (holder +0x74).
- 4x4 slot grid (children 1..16, the equipment layout), child 17 View Wares (the cursor starts there; enabled with a
  bazaar, 0x00D flags1 u32 @0x20 bit 31, once the message arrived or 120 ticks passed). Title slot 2 the name, help
  slot 2 "Lv.12 Ranger" (group 10 0x58 / 0x79 + " / " sub job, "？　？　？" when anonymous). The linkshell icon
  and name at the bottom. The item info follows the cursor; on View Wares the bazaar message ("menu    comment").
- Escape closes (SE 0003); so do zoning, engaging, an event start and losing the target.
- Left: the bazaar list behind View Wares (shop list window + c2s 0x105).
- Live: Brannoc (a bot) showed "Lv.16 Warrior" and its gear; the cursor moves by the column rule.

## Chat line history (2026-10-24)

Spec `~/ffxi/lab/research/input_history_spec.md`.
- FSTEXTINPUT (DAT_10662d40, built once, freed at shutdown) keeps the history at input +0x7ed8: 10 slots of 0x97
  bytes, slot 0 the working copy, 1..count the sent lines newest first, count at most 9; not a ring (a push shifts
  and the oldest falls off).
- Only the chat line's submit (mode 4, FUN_101b6e40) records: every non-empty Enter, no duplicate check, commands
  too, recorded before the command runs. Raw bytes kept (auto-translate blocks, Shift-JIS).
- Up (FUN_101ac2c0) / Down (FUN_101ac3d0): no sound, no wrap, Shift / Ctrl the same, Alt nothing (WM_SYSKEYDOWN is
  not passed on). A move first stores an edited line (dirty flag +0xc78) into the slot it came from: the half-typed
  line comes back on Down, an edited recall stays edited. Kept across logins (ours: moved between the App and the
  World).
- Live: two Ups recalled "/echo one".

## Effect lookups: failed files and the +0x298 order (2026-10-24)

Spec `~/ffxi/lab/research/cast_fx_spec.md` F5.
- A DAT whose load fails is remembered (Effects.failed): actor lookups pass it by, a file start ends at once, and
  model generators start from the files that did load. Before, the failed id stayed in `loading` for good, every
  lookup on that actor waited on it, and a pending st?? / sp?? was dropped after 600 ticks: the cast's stop was lost.
- vtable +0x298 (FUN_100cce00, CXiSkeletonActor 0x10330f40 + 0x298) looks in FUN_100cc440 (the motion files, then
  the model's) and DAT 0, never the part files. Ours uses that order for the cast starts, cast_state's st?? and
  0x038; ops 0x09 / 0x57 / 0x85 keep part files first, the others after the motion files.

## Entity size only from General updates (2026-10-24)

- GraphSize (0x00D / 0x00E Flags1 u32 @0x20 bits 9-10) comes with SendFlg.General (mask 0x04) only, like the other
  @0x20 fields: LSB's CEntityUpdatePacket and CCharUpdatePacket fill Flags1 under General, and a position update
  (mask 0x01) carries zeros there.
- Ours applied it on every update: a mob of size 1 (most LSB mob pools: 364 at 1, 147 at 2, 46 at 3, 234 at 0)
  went to info[10] (85 %) at each position update and back at the next General one. The user's "mobs constantly
  changing sizes" in Valkurm.
- Live (La Theine, 45 s): 79 mask-0x01 updates, all size bits 0; General updates (0x04 / 0x05 / 0x0f) carried 0 or
  1. The size now holds through the position updates.

## Search: the /search grammar (2026-10-24)

Spec `~/ffxi/lab/research/search_spec.md` 1.5 / 2.2, checked against the decompile (FUN_10104f00, FUN_10105520,
FUN_10105640, FUN_10105930, FUN_10105bd0, FUN_10105de0, FUN_10105ab0, FUN_10105fa0).
- Word lists ({i32 value, char *word}, NULL-ended) in a row from 0x10331cf0: categories (14), the UNITY keyword
  (0x10331d68, FUN_101055f0), areas (460 words, ALL = -1), nations, races, jobs, status words; unity leaders at
  0x10364880. Found at run time by the pointer to "NAME" / "PIEUJE" (the packed FFXiMain.dll keeps its data
  sections plain). Regions: 0x32 pointers at 0x103858a8 to 0-ended u16 zone lists (entry 30 points into .bss);
  zone flags DAT_103669b8, u32 words, 8 zones a word, low nibble first, zones < 300 (FUN_101070f0); FUN_1020c1c0
  keeps a region's zones with flag 2 (searchable), 16 at most.
- FUN_10105520: a token (quoted, or up to whitespace) against an upper-case word, whole and case-insensitive; the
  first table entry that matches wins. No match returns the terminator's -1.
- FUN_10104f00 per token: `+` / `-` + a category (a `-` before a digit is a range): STATUS / UNITY, LINKSHELL(2),
  FRIEND, a set or sorted category end the parse; the sort goes in, and a value after it is optional (not consumed
  when it does not parse). A keyword: a set category ends the parse (STATUS / UNITY may repeat); FRIEND sets +0x4e
  bit 2 and flag 0x1000; LINKSHELL / LINKSHELL2 take the equipped LS ids (ours: the LS item's exdata u32 @0,
  LSB GroupId *(inf for retail)*); REGION copies the searchable zones of the current region when no area is set;
  other values through FUN_10105640, a failure ends the parse. A bare word: a status word, an area (no area set or
  sorted, not starting with a digit), level, master level, job, nation, race, name, each only while its category is
  free and unsorted; nothing ends the parse.
- FUN_10105640: NAME keeps up to 15 characters as typed (quotes allowed; it never fails); AREA puts the zone first
  unless listed (16 at most) and sets +0x22 (ALL -1); levels and master levels share +0x1c / +0x1e (`/sea 75 30-40`
  sends 30..40 as both); UNITY with nothing after it = the own unity bits (+0x64), an unknown word sets 0x0f800000
  and is read again.
- FUN_10105930: an area word for zone 1..0x17f fails without its known-zone bit, and with no bits at all.
- FUN_10105bd0: skip whitespace; digits with at most one `-` / `<`; a single value over 255 is 0; a pair clamps to
  0..255 unless the side over 255 is the larger one (then 0..0); no swap. FUN_10105de0 (rank): a single value as a
  byte, unclamped; a pair: below 0 -> 0, over 99 -> 100.
- End: no area -> the current zone and +0x4e bit 1 (wire type 3); an area with +0x22 > 0 sets the bit too.
- Encoder FUN_10104200 now covers every field: the sort list (typed sorts, then name and the set fields' types),
  per entry type / descending / present and its value, the LS / friend / status-high markers.
- s2c 0x008 (LSB GP_SERV_COMMAND_ENTERZONE after every 0x00C): EnterZoneTbl @4, 0x30 bytes, kept in World.
- Ours does not track the own unity yet (0x0f800000, "none").
- Live (Lnxtest, bots): `/sea all war` 2 WARs; `/sea all 10-20` the 8 players from 10 to 20; `/sea region` "4 people
  found in this region." (zones 230..233); `/sea all elv -level` 4 Elvaan, levels 18, 15, 10, 8; `/sea name Doran`;
  `/sea "Southern San d'Oria" war` area 230 + WAR; "Northern San d'Oria" (not visited by Lnxtest) became a name, as
  FUN_10105930 refuses it.

## Window group walk (2026-10-24)

Spec `~/ffxi/lab/research/window_groups_spec.md`.
- The window list (0x10621838) is in raise order: the head is the bottom, the tail the window created or raised
  last; layer only decides draw order (every layer >= 5 row has group 0). FUN_1015fa00 walks it from the tail
  (0x1015fe10..0x1015fe9c) with mask 0: a visible window sharing a bit with the mask is suspended
  (FUN_1015e6e0(w, 1, 1, 0, 1)) and adds none of its bits, even the free ones; any other visible window adds its
  group; a hidden one that is resumable and wanted is resumed (FUN_1015e530) and adds its group. No flag exempts a
  window; group 0 never takes part (the party window is "menu    partywin", group 0).
- A suspension is a hide, not a close: the close zoom plays, the handler, cursor, contents and list place stay. The
  walk itself brings it back once nothing above holds its bits: the open zoom, no raise.
- Deferred reopens (FUN_1015e350 with defer, registry flag 0x400: gaugewin, targetwi, playermo, mp_pmode): an
  existing hidden window whose bits are taken goes under every window sharing a bit (the requeue loop
  0x1015fb35..0x1015fbb6) and waits; a new one goes on top.
- Consequences read from the code: abiselec (0x04) suspends playermo; the magic / ability / emote / mount lists
  (0x0c) suspend playermo, money, magselec, abiselec, iteminfo, chatctrl; subwindo (0x80) hides the 0x81 menus below
  it; loot, persona, trade, shop, inspect hide gaugewin; iuse (0x81) replaces itmsort2 (0x01).
- Ours (GameUi::want / run_walk): every window that passes the gate, and the ones drawn outside it (playermo, the
  target window, the main menu, the status icons), says it is open each frame; its stamp is the frame it opened
  (a new top), targetwi's under the windows holding its bits *(inf: gaugewin and playermo taken as new)*. At the
  end of the frame the walk runs over them newest first and a suspended window is not drawn next frame. Not
  modelled: raise tickets (a window re-raised while open), handler hides with their own flags.
- Live (Lnxcsfix): the main menu's Abilities page and the action menu's ability types show alone, as before (our
  code closes their parents too); the item list's Use / Drop window takes the Options window's place.

## Near plane 0.1; rain and frame rate (2026-10-24)

- The scene projection's near plane is retail's 0.1 (FUN_100146e0, MODLOG "name plates"); ours was 0.5 since the first
  renderer. With the eye kept only 0.17 off the floor below it (FUN_1017a310), a 0.5 near plane cut into the slope
  beside a low camera and the clear colour showed through as a flat grey or white wedge (La Theine at the
  Lnxcsfix login spot, camera tilted level). Depth32Float at 0.1 / 2000 keeps about the precision of retail's 24-bit
  buffer at 0.1 / 65535: zone shots of zones 230 and 102 from street level differ in 7..34 pixels.
- Rain (`!setweather RAIN` in La Theine, 2560 x 1366, frame limit 144, the camera level): 143 fps clear and in rain,
  CPU 1.5..1.8 ms a frame. Not reproduced: the user's drop to about 70 fps in rain. The 0.3..0.4 s perf spikes in
  test runs are the script's own screenshots (the capture runs in the encode span).

## Action menu: the mouse hold, release confirm, party cursor on engaging (2026-10-24)

Spec `~/ffxi/lab/research/engaged_menu_spec.md` 3 and 4.
- FUN_1012b4b0's tail: with playermo focused and the pointer on a row, the left press sets the hold to 60, holding
  counts it down, and at 0 (not on the release frame) the row's Right function runs (table 0x1036f240 @8: Disengage /
  Call for help FUN_1012acc0, Magic FUN_1012ab00 -> magselec, Chat / Tell FUN_1012aae0 -> chatctrl, not done) and the
  hold starts again; a row change by the mouse restarts it. No sound of its own. Ours times it in real time (1 s).
- FUN_1015de50: the left release over the focused window's cursor row confirms it (ours used egui's click, which
  drops presses longer than 0.8 s); on the window but no row SE 0004. A right release is msg 6, Escape (ours: a right
  click over the menu, so a right drag still turns the camera).
- FUN_1015f3e0(0x20) also runs FUN_10202f00: the party-list cursor (<stpt> / <stal>) is cancelled when we engage, its
  macro stopped. Confirming the target with the menu up runs its open again (FUN_1012ada0): the rows reload, Call for
  help resets, the cursor stays *(inf)*.
- Script steps `ldown:X,Y` / `lup` press and release the left button separately.
- Live (Lnxcsfix): held 1.3 s on Magic -> magselec; a click on Abilities -> the ability types; a right click on the
  menu closed it.

## Treasure pool window: loot, lootope, lnowin (2026-10-24)

Spec `~/ffxi/lab/research/loot_spec.md`.
- Only the action menu's Treasure row opens it (command 0xc -> FUN_1013f060; also the Monstrosity menu): no text
  command, key function or main menu row. The row is in every layout (table 0x1036f390) while the pool holds an item,
  not in Conflict (FUN_1012af30 case 0xc); its help is 0x1b in a party, else 0x1c. The action menu stays open under it.
- Slots at G+0x1951c, 0x58 bytes: state @0 (2 in the pool, 3 gone), item @4, quantity @8, our action @0x30 (0 / 1
  passed / 2 lotted), our lot @0x34 (-1 passed), the highest lot / lotter @0x36 / @0x38 / @0x40, the source @0x50,
  StartTime @0x54. 0x0D2 fills them (Entry @0x15 into @0x30, own lot only when IsLocallyLotted @0x1c); 0x0D3 sets
  ours when EntryUniqueNo is us (EntryFlg ? 2 : 1) and the highest lot always.
- loot (16, 48) 182 x 190, group 0x10: up to 10 rows, no scroll bar, oldest StartTime first; a 16 x 16 icon at x + 2,
  the name at x + 20 (white, 0x80804020 once lotted, 0x80404040 once passed), "%2d" for stackables at window x + 0x12;
  the cursor row 1 px up-left. Down / Up one row without wrap, Right / Left the last / first row, Enter lootope (a
  passed row SE 0004), Escape closes all three, Num+ the focus to lnowin (SE 0013). It closes on an empty pool,
  engaging, an event start and zoning; the item info (mode 0) follows the cursor.
- lootope at the row's right edge: Cast Lot / Pass; Cast Lot disabled once we lotted (the cursor still starts on it,
  FUN_10118870). Cast Lot refuses a Rare item held in any container (group 7 0x7e) and a full inventory (0x7d), else
  c2s 0x041 [slot, first free inventory slot, 0, 0]; Pass c2s 0x042 [slot, 0]. It closes when the cursor row changes
  under it. lnowin (384, 48) "Option / Done": Done passes every slot not acted on; hidden while lootope is up.
- Bars: help 0xe 300 / 301 / 302 / 922, title 0xd 73 "Treasure Pool" (clipped at titlewin width - 8 like any title).
- The party window shows the chest (fontshp 0x7a) on its top edge while the pool holds items (FUN_102041d0).
- No timer in retail (StartTime only sorts). LSB resolves an item 297..300 s after the drop, or at once when one
  member with room is alone in the pool.
- Draw order: the item info is drawn after the action menu: both sit on the log (flag 0x1000) and it opened later.
- Live (Lnxcsfix in a party with the bot Rokuro, `!addtreasure 4096` / `4112`): the chest, the Treasure row, the rows
  with icons and " 1" on the crystal, Cast Lot -> "Lnxcsfix's lot for the fire crystal: 210 points." and an orange
  row, Pass -> a grey row, Cast Lot greyed on the lotted row, Num+ -> Done, Escape; at the expiry "Lnxcsfix obtains
  a fire crystal." / "Rokuro obtains a potion." and the row and chest went.
- Not done: the zone-pool columns (Treasure-flag zones), lnowin's raid1 dodge, the mouse.

## List scroll bars (2026-10-24)

Spec `~/ffxi/lab/research/scroll_bar_spec.md`.
- A window has a bar when its handler calls FUN_10118530 (11 sites): the generic row list (base vtable 0x1033a730,
  open FUN_101f68b0: scresult, inventor, bank, shop, cmbhlst, maplist, scanlist, the social, merit and auction lists),
  the paged list (base 0x1033bf78, open FUN_10221ef0: magic, ability, emote, mount, key items, quests, missions,
  guides) and own handlers (query, fulllog, cfilter / fxfilter, k1assign / k2assign, textcol1, faqmain / faqsub,
  hnhead). No bar: loot, delivery, the moogle storage menus, conf11s, arealist, chswin, logwindo, itmsortw / itmsort2
  / itmstora / iuse / shopmain.
- Every bar is the same (FUN_10234680, drawn by the window draw at 0x10117e97 after the contents and the hand, focused
  or not, while window +0x69 / +0x74 are set and visible < total): `menu    scroll` 0 top cap (x, y) 8 x 4, 3 thumb
  (x + 1, y + 4 + off) 6 x len, 1 track (x, y + 4) 8 x (h - 8), 2 bottom cap (x, y + h - 4) 8 x 4, colour 0x80808080;
  len = ((h - 9) visible + total) / total, off = (h - 8 - len) top / (total - visible), integer.
- Rect: x1 - 8, y0, the frame height at open; a later move or resize (FUN_1011a5a0, asm 0x1011a6d9 SUB 6) puts it at
  the new x1 - 6 (inventor, moved at every open; the shop, resized; cmbhlst when placed; fulllog); region stacking
  (FUN_1015f790) back at x1 - 8 on the stacked frame (magic, ability, emote, mount, query, maplist).
- Ours: screens::push_scroll_bar / GameUi::scroll_bar on the item lists (inventor x1 - 6, bank x1 - 8), the magic /
  ability / emote / mount lists (12 rows), the shop (10), the event choice (3), the full log (bottom-anchored top),
  maplist (5), scanlist (10), the synthesis history (11) and scresult (15).
- The mouse on a bar (focused window only): caps 0x16 / 0x17 one line, the track 0x18 / 0x19 a page, the thumb drags
  (0x1b, top = start + dy total / (h - 8)); the wheel sends 0x16 / 0x17 to the focused window. Not done in ours.
- Left: the config list pages (cfilter / fxfilter / k1assign / k2assign 14 rows, textcol1 11, which also scrolls in
  retail: its category 0 has 13 rows, ours stops at 11), the key item / quest / mission lists.
- Live (Lnxcsfix, 25 items): the item list's thumb at the top, at the bottom on the last row; the magic list's bar.

## Chat mode window: chatctrl (2026-10-24)

Spec `~/ffxi/lab/research/chatctrl_spec.md`.
- Right (or the mouse hold) on the action menu's Chat / Tell row runs FUN_1012aae0: "menu    chatctrl" (vtable
  0x10336420; registry flags 0x21202, group 0x08), the action menu stays under it. Rows 1 Say, 2 Tell, 3 Party,
  4 Linkshell, 5 Linkshell 2, 6 Unity, 7 Shout (keytops3 label sprites), child 8 the tell-name arrow; Up / Down wrap
  7 <-> 1. Init (FUN_1012ccd0) puts the cursor on the current mode's row (2 / 7 / 8 / 0xa -> 1, 3 / 9 -> 7, 4 -> 2,
  5 -> 3, 6 -> 4, 0xb -> 6, 0xc -> 5) and disables 4 / 5 without linkshell 1 / 2 and 6 without a Unity faction
  (s2c 0x061 u32 @0x58 & 0x1f, FUN_100a3600).
- Enter (FUN_1012d2b0): SE 0002, the mode through FUN_101ad670, then the chat line opens empty (the prefix is added on
  send) and the window closes; a disabled row SE 0004 only. Tell by the selector: the current tell name (no change),
  the target, a sent-tell name; none: SE 0004 and no mode. Right on Tell cycles the selector silently, Left closes
  silently, Escape with SE 0003. Bars: help 0xe 0x253 "Set your default chat mode.", title 0xd 0x81 "Chat".
- FUN_1012ce90: the tell name at (+100, +23) in fontshp (current 0x80509050, target 0x80909040, history white); each
  linkshell's icon at (+0x4f, +0x34 / +0x44) tinted from extdata u16 @6 (each nibble x 13 + 0x38) and its name
  (FUN_100ed650 of extdata +9) at (+0x5f, ..) in 0x80806060, else group 0xc 0x55 "No Linkshell"; the Unity at
  (+0x50, +0x56): group 0xc 0x1a2 + faction, 0x80803040 for none, grey 0x80404040 outside the Unity chat (@0x60 bit 0).
- subwindo in mode 0 (FUN_1014f960 case 0): windowps 283 "Chat Range" at (+5, +8) and the cursor row's range at
  (+15, +26): Say 284 Vicinity, Tell 285 Target only, Party 286, Linkshell 287, Linkshell 2 709, Unity 710, Shout 289
  Wide area; stacked like the MP Cost box.
- FUN_101ad670(mode, name) keeps the previous mode unless the current one is shout or message; FUN_101adb80 restores it
  after each channel send, so Shout lasts one line. Tell sets "/tell Name " and the current tell name; other modes
  clear it. Sent tells (FUN_1018d6e0) go to the front of an 8-name list (retail saves it in acq.dat; ours keeps it for
  the session). /chatmode goes through the same function.
- The log's channel caption follows the chat mode (FUN_10161990 by the prefix's first word). The action menu's Chat /
  Tell Enter opens the line with "/tell <target> " on a target (FUN_1012b880 cases 3 / 4).
- Disabled children: the hand is drawn on one under the cursor (FUN_10118870 does not skip them), for every window.
- Live (Lnxcsfix, F1 then the action menu): the window with No Linkshell twice and No Unity, Chat Range / Vicinity over
  the target window, the Chat bars; Party -> "hello" gave "There are no party members." and "(chatmode : /party )",
  the log caption Party, the action menu still up; Tell showed "Lnxcsfix" in yellow and set "/tell Lnxcsfix ".
  Shout was not tried (it reaches the zone).

## Config lists: bars, textcol1 scrolling (2026-10-24)

- cfilter / fxfilter / k1assign / k2assign (FUN_10197b80 / FUN_1019d3b0 / FUN_1019cff0): the bar at x1 - 8 over 14
  rows; conf11s has none. textcol1 (FUN_10198620, 11 rows, total the category's colours) scrolls like them
  (FUN_10198800: Down on row 11 moves the top while top + 11 < count; Left / Right page): its category 0 has 13
  colours, and ours stopped at 11. Enter and textcol2's sample take the scrolled row.
- Checked offscreen (`topbar-shot --config "conftxtc:1,textcol2:1,textcol1:11:2"`): rows 3..13, the bar's thumb at
  the bottom, the "yell" sample; k1assign scrolled by 5 with its bar.

## Resent datagrams (2026-10-24)

- FUN_100f76b0 (the zone receive): once connected (state 3), a datagram whose header u16 @0 is not newer than the
  last one handled ((last - seq) as u16 < 0x8000) is dropped and counted (FUN_100f7ab0 slot 2); a sub-packet whose
  sync is not newer is skipped (slot 4). Only an accepted datagram stores the new sequence (+8) and ack (+10).
- LSB (map_networking.cpp recv_parse): when our header u16 @2 is not its latest server_packet_id, it resends its last
  datagram unchanged. Our 400 ms datagram leaves before a late reply arrives, so a slow map server (15 bots) sent
  some datagrams two or three times, and we printed every battle line in them again (user's run, 2026-10-24). We
  now drop them as retail does.

## Battle message relations: party and alliance (2026-10-24)

- FUN_1009e700 (30261006_0; the old build's FUN_1009cda0) picks the mode table: caster or target missing -> others
  (0x10359b30); target us (0x10358730); caster us (0x10358330); caster in our party (mode 0 walk, FUN_100e8b80 /
  FUN_100e8c70, not us: 0x10358b30); target in our party (0x10358f30); caster in the alliance (mode 5, everyone
  listed: 0x10359330); target in the alliance (0x10359730); else others. Members match by id (+0x14 vs entity
  +0x78).
- The chat filter (FUN_100abf20): [1] target us or our pet, [0] caster us or our pet, [2] / [3] party caster /
  target (a member's pet too, entity +0x1f6 with +0x12c bit 22 clear: not modelled, we keep no pet index for
  others), [4] / [5] the alliance likewise, [6] / [7] a mob claimed by us or a party member (+0x120 bit 13, claimer
  +0x184; the first test on our entity's +0x188 / +0x130 bit 25 is not modelled), [8] / [9] a PC (id class 0), [10]
  otherwise or a missing actor.

## Log focus and the camera keys (2026-10-24)

- FUN_10151550's state (ours `log_focus`) stays at 1 when Escape (logwindo message 6) or another window unfocuses
  the log; only the focus itself takes the arrow keys. Ours gated the camera on the state, so after Escape or a menu
  the arrows did nothing until the next Num+ reset it (user's run, 2026-10-24: `log_focus 1`, no window open). The
  camera gate now reads the focus (`log_active`, or the status icons at state 2); F12 shows both.

## Target flash: the ambient bytes (2026-10-24)

- DAT_104448c0 is D3DRS_AMBIENT (FUN_10029590: SetRenderState 0x8b, then SetLight / LightEnable for slots 0..3 from
  DAT_104449a0, LIGHTING 1). Its bytes are ftol(env actor ambient / 255 x 128) (FUN_10181de0 mode 1; 0x1031fcd0 =
  128.0; current FUN_10188e00). Actor material (FUN_1002a8c0, template DAT_10346380, current DAT_10351518): diffuse
  0.5, ambient = actor colour +0x660 (0x80808080) / 128 = 1.0, specular / emissive 0. No vertex colour (COLORVERTEX
  falls back to the material); the light ambient DAT_106174dc is never written *(inf: 0)*. Stage 0 MODULATE2X, stage
  1 MODULATE2X by TFACTOR 0x80 (neutral).
- So an actor's vertex colour is clamp(Ga + 0.5 x sun x ndl), Ga = the RS bytes / 255; the flash mixes those
  (already halved) bytes toward a full 220. Ours halved after the mix, so the flash peaked at 110: at noon in Valkurm
  (actor ambient 167..173, sun 196 x 1.5) a shaded texel 0.5 went 84 -> 110 where retail goes 83 -> 220. Without a
  flash both formulas agree (within the ftol rounding). lit_actor now follows retail; flash-shot zone 103 hour 12:
  peak difference 155 (was 43).
- Not done: each 0x8010 material record's light factor f (f32 at record byte 18): body path scales the RS ambient
  bytes and light diffuse by f, cloth path diffuse x f and emissive max(f - 1.5, 0); Hume faces carry 1.2..1.6.
- Research notes and scripts: `~/ffxi/lab/scratch/flash2/`.

## The raise prompt: placement and the K.O. focus (2026-10-24)

Research `~/ffxi/lab/scratch/raise2/` (30261006_0). Corrects "Window groups, region stacking and the raise prompt".
- FUN_1015f790 moves the window's own rect (element 0: layout pos + anchor shift, layout size; FUN_10118f70 /
  FUN_10119110), not its frame: rect top = log top - h - 2 (log bottom - h with no lines), at least 16; the frame
  keeps its header offset from it. resynus is the only stacked window in 39542 with a frame offset (0, -8), so its
  frame sits 10 px above the log (H - 224 with a full 8-line log); ours stacked the frame, 2 px above.
- playermo (flags 0x08011c02) and resynus are both region-0 windows at x 16, 112 wide; retail never shows both:
  FUN_1012e0c0 (the K.O. window) closes the 0x800 set twice (FUN_1015f3e0: every row whose state @0x28 lacks bit
  0x4: playermo, mp_pmode, menuwind, comyn, resynus); the K.O. window or resynus holds the focus (FUN_1015e7e0 /
  FUN_10119200: DAT_1057876c+0x74 bit 0) and FUN_101585f0 (Enter, the mouse release, the target keys) returns while
  that bit is set; Num- goes to the focused window (message 7). Groups: resynus / dead / comyn 0, playermo 4.
- FUN_10096c90 mode 2 (the offer): comyn closes every frame; with the K.O. window not visible it reopens it
  (FUN_1012e0c0, closing the 0x800 set again).
- Ours: world_window_open counts the K.O. window and the prompt (world clicks, the hit cursor); the target keys,
  Minus and Pause's log-out prompt wait; the K.O. window under the prompt shows no hand and takes no click; mode 2's
  rules in ko_step. A plain left click while dead used to open the action menu over the prompt (the user's report).
- Live (Lnxtest, Valkurm, killed by goblins, `!raise 3`): the prompt 10 px above the log, a world click / Tab /
  Minus open nothing, Enter accepts and the character rises.

## Closing the window in the world (2026-10-24)

- Retail takes Close off its window menu (RemoveMenu(SC_CLOSE), all.c 16267; WM_CLOSE / WM_DESTROY both
  PostQuitMessage at 0x100168f7), so the window cannot be closed while playing. Ours (the user's choice): a close in
  the world runs "/shutdown on" and the client exits when the server ends the session; at once outside the world or
  with the link quiet for 10 s; a second close exits when no logout could start (engaged, K.O.). Live: closed by the
  user, logout 1 s later (LSB leavegame.lua: GMs and the Mog House log out at once, others count 30 s).

## Material light factor (2026-10-24)

- Each 0x8010 material record carries a light factor f (f32 at record byte 18; research `~/ffxi/lab/scratch/flash2/`
  matscan.py: Hume M parts 1.0, faces 1.2 / 1.3 / 1.6). Body path (FUN_1002cdf0, asm 0x1002d2dc..0x1002d3fb):
  every light slot's diffuse r, g, b x f (a copy, then SetLight), the D3DRS_AMBIENT bytes x f clamped to 255 (alpha
  as is). Cloth path (FUN_100216a0, mesh flags & 0x7f == 1): the lights' diffuse x f, the material emissive =
  f - 1.5 per channel when f > 1.5 (0x1031f21c = 1.5), ambient unscaled.
- Ours: Group.light, MeshPart.cloth; SkinVertex.mat and the CPU path's vertex colour carry (f, cloth) in red
  (0x80 = 1.0) and green; lit_actor: lit = clamp(ga + em + 0.5 f (sun ndl + lamps)), ga = min(ambient bytes x f,
  255) / 255 (body) or the bytes (cloth), em = max(f - 1.5, 0) on cloth. flash-shot zone 103 noon: only the face
  changes (up to +77).

## Name plates while K.O. (2026-10-24)

Research `~/ffxi/lab/scratch/nameplate/`. No K.O. case in retail: FUN_100ce480 [current FUN_100d04c0] projects
vtable +0x1c4 of point +0x8a7 (2 from the constructor) plus f32 +0x8c0; FUN_1002b5e0 point 2 is the rest translation
times the size (no bone, no rotation, no status), lifted only for status 5 / 0x55 (-1.3) and 0x3f..0x53 (-0.5). A
dead PC's plate stays at standing head height, as ours (plate_point). The PC `dead` scheduler (7072) has no 0xb4 /
0x9f.
- Not done: scheduler op 0xb4 (+0x8c0 for the op's duration; only 8 `cst0` schedulers, -1.5 for 600 ticks); op 0x9f
  plate points (29 `init` + 3 `nam0` monster schedulers: those plates follow the posed bone, death motions
  included); the current build's mount sub-case (asm 0x100d4619..0x100d4714: -0.5, or per-race tables for mount
  entity +0x1db 0x1d {0, .5, .5, .5, 1.1, -.7, -.7, .5, .05} / 0x25 {0, .4, .4, .4, .4, -.8, -.8, .4, .4}; -1.3
  without a mount actor or with k = FUN_10022340(race) 0 / 6, k not decoded).

## List scroll bars: the mouse and the wheel (2026-10-24)

Spec `~/ffxi/lab/research/scroll_bar_spec.md` sections 2..5. client/scrollbar.rs: the focused lists register their
bar each frame (GameUi::bar_site, whether it shows or not); the next frame's HUD runs FUN_102347f0's zones (1 / 2
caps, 4 / 5 the track, 3 the thumb), the press and the key repeat (16 ticks, then every 2; FUN_10234930) as 0x16 /
0x17 / 0x18 / 0x19, the thumb's drag top = trunc(start + dy x total / (h - 8)) as 0x1b (FUN_10234a20), and each wheel
notch as 0x16 / 0x17 to the focused list (FUN_10125360; else the wheel zooms the camera, the user's FOV choice). While
the bar holds the mouse the windows see no press and no pointer.
- Messages per list: the row lists (inventory, the equip pick, both storage sides, the read-only container, shop,
  Change Map, Wide Scan, search results; FUN_101f6c70): a row moves the top under the hand, a page as Left / Right,
  0x1b the top. The paged lists (magic, ability, emote, mount; FUN_10222150): rows move top and cursor together,
  0x18 back by min(top, 12) (cursor 0 at the top), 0x19 the cursor a whole page (the top with it, clamped). query
  (FUN_1014e410): rows the selection (wrapping, as the keys), pages 3 keeping the screen row, 0x1b the top with the
  selection shifted. The Config lists (cfilter, fxfilter, k1assign, k2assign, textcol1): rows move top and hand
  against each other (the selection stays), 0x1b silent. The full log: its handler is not traced *(inf: the
  scroll-only Up / Down, Left / Right pages, 0x1b the bottom-anchored top)*.
- Script step `act:wheel:N`. Live (Lnxcsfix, Items, 23 rows): wheel -3 scrolled 3 with the hand on its row; the
  bottom cap 1 row, the track a page and at the end the hand to the last row, the thumb dragged to the end with the
  hand kept. Unit tests scrollbar::{row_list, paged_list, hit_zones}.
- Not done: the post / delivery / trade / synthesis lists (post.rs draw_rows) and the synthesis history.

## Actor collision and lifts (2026-10-24)

Spec `~/ffxi/lab/research/collision_actors_spec.md` (30261006_0).
- Actor collision: only XiModelActor boxes (Mog House furniture) join the player's sweep (FUN_1016e930, gate
  DAT_106631d4 = the Mog House object and FUN_101d5fe0). Characters (CXiSkeletonActor) are not in that sweep: they
  have their own test, see "Character bump" (that pass missed it). Corrects "actor-body boxes" in "Player movement
  collision". The furniture boxes (FUN_1016d580, spec 1.3) wait for the furniture.
- Lifts: the only moving platforms, vertical only (RID '@' records in zones 9, 141, 143, 149 and 237; ships are their
  own zones). The record: +0x30 bits 4..7 the surface type, +0x34 / +0x36 i16 floors (pos.y + v / 256), in memory the
  height +0xdc (the box centre until a lift actor exists). The lift NPC (0x00E SubKind 3, XiLiftActor) carries the
  id @0x34, the move start @0x38 (Vana'diel-epoch seconds) and end start + u16 @0x3c on every update; animation - 10
  is the floor. At first sight the height goes to floor 0 and `mv0<floor>` plays; a state change (10..17) waits for
  the start time, then `mv<old><new>`. Op 0x1d (FUN_10057fb0): from the height now to the floor (the scheduler's
  name byte 3) linearly by the packet's end time (its own duration @6 is unused); FUN_100c3920 sets each part's world
  y and the box height. FUN_100a4ed0 (every collision actor, every frame, events and standing still too): a box
  holding the frame's start or end (inclusive; the face test is world-space dead code) sets the feet's y to the
  height, grounded, normal (0, -1, 0), its surface, riding (+0x5e2), and skips the no-floor undo. Riding: the camera
  eye follows the height change with a 10-tick hold (FUN_1001ee60), the walk / run speed is horizontal, the map is
  refused with group 7 0x61 (FUN_101f9a50). c2s 0x015 stays absolute.
- Ours: client/lifts.rs (shafts from the RID, lift actors, the moves on the zone's tick clock, the GPU parts through
  base * T(t)), Ground::step's ride query, the drawn y on a lift, other characters at the height, footsteps with the
  shaft's surface, camera::ride, the map refusal. Live (Lnxcsfix, Metalworks '@6l0' / '@6l1'): first sight mv00, then
  mv01 / mv10 every 18 s over 480 ticks, the character riding on the planks with the camera; /map refused. Tests
  lifts::metalworks_shaft, ground::rides_a_lift.
- Not done: the zone-in fade's wait for a lift actor while standing in a shaft (FUN_1008abe0 state 4); the camera's
  look point; LSB's Metalworks door data (spec 2.7) not checked against the doors.

## Lifts: the zone-in wait (2026-10-24)

- FUN_1008abe0 state 1: standing in a lift shaft with no lift actor yet (DAT_10482f45), the zone-in fade waits for one.
  Ours the same, giving up after 10 s *(not retail: retail waits)*. Live (Lnxcsfix logged in inside Metalworks @6l0):
  zone-in 37.58, the lifts first seen 40.82, then the fade; standing on the planks.

## List scroll bars: the pickers (2026-10-24)

- The inventory list at x 200 (post_rows / draw_rows: the post send picker, the hand-over, player trade, synthesis and
  its crystal list) registers its bar while focused; one owner (Picker) finds whichever is open (FUN_101f6c70 rows).
  Live: the synthesis picker, focused, scrolled 4 on 4 wheel notches with the hand kept; unfocused (the slots
  focused) it did not.

## Character bump (2026-10-24)

Spec `~/ffxi/lab/research/collision_chars_spec.md` (30261006_0).
- FUN_100a8770 (old FUN_100a6e00): called at the end of the local player's move input FUN_100a6240 (old FUN_100a48d0)
  in both camera modes, with no input too, before the delta is applied and before the MZB sweep; only the local
  player. Off when our GmLevel (+0x128 bits 11..13; 0x037 @0x28 bits 29..31) >= 3. The nearest eligible actor (3D,
  squared distance < 64) to pos + delta; contact when distance - R(me) - R(it) < 0, R = 0.8 x the horizontal offset of
  skeleton point 44 (bone 0 + (0, 0, -r): Hume / Elvaan / Mithra 0.32, Taru 0.24, Galka 0.64; FUN_100aaef0). First
  contact with an actor holds the move for 30 ticks (+0x5ac, minus the frame ticks; +0x5b0 flag; +0x5a0 handle);
  then you pass through while you overlap it; a frame with no contact clears it (letting go restarts it). No slide,
  no push; the input tier stays, so the player runs in place.
- Eligible: main body actors of kind 0/1/2/6/7/8 (not doors, lifts or ships), +0xb2 == 0 (drawn, not culled), not
  +0xa4 1..4 / 0x54 *(inf: mount get-on / off)*, status not 2 / 3, look[0] not 0x32..0x3b (SubKind 0 marker models),
  +0x59c >= 1 (faded in), not event op 0xab 0x0b-flagged (+0x120 bit 6; 0x0c clears), and PCs (id < 0x01000000) only
  while +0x5ae > 0: FUN_100a57f0 sets it to 1800 ticks when the actor moved > 0.02 in a frame or its status is not 0 /
  0x21 / 0x2f / 0x3f..0x53. So players idle for 30 s are never solid (the user's "no collision with players"); NPCs,
  monsters, trusts and pets always count.
- Ours: client/bump.rs (hold, radius from the rest pose *(inf: retail reads the current pose)*, active_tick,
  eligible, blocks), called once per 2-tick physics step (retail's 30 fps frame) unless in an event. Not done: event
  op 0xab subs 0x0b / 0x0c (op 0xab is R12), the +0xa4 states. Live (Tuxedo, GM 0, Northern San d'Oria, walking into
  Elesca): 16 held steps (30 -> 0, 0.5 s) at gap -0.062, then through to the other side. Test bump::hold_then_pass.

- Event op 0xab subs 0x0b / 0x0c (FUN_100bac90) set / clear the event actor entity +0x120 bit 6, the bump test 3 flag:
  ours e.no_bump from the VM generic Effect::Op (the actor is the VM own NPC). The other subs bits have no known
  reader yet (R12).

## Party menu (2026-10-24)

Spec `~/ffxi/lab/research/party_menu_spec.md` (30261006_0); ours client/partymenu.rs.
- Main menu child 6 (FUN_10140400 case 6, SE 0002 always) -> FUN_102016e0: prtyjoin while an invite is pending, prty3
  for the alliance leader or a party leader with no alliance, prty7 for a party leader in someone else's alliance,
  prty2 for a member, else prty1. The row is disabled (its kind-4 sprite, silent) while the party is us and trusts
  only (FUN_10201f00 over all 18 slots); HudState::menu_off draws disabled main-menu children.
- The windows close the main menu (FUN_10205570) and bring it back on the Party row when the bottom one closes
  (FUN_10205660, which also closes the search results and a menu pick on the party list). prty4 / prty8 / prty5us /
  alarm stack over their parent; the walk (group 0x81) hides the parent meanwhile. A window watching the party state
  ([7]: prty1 / prty5us 2, the others 3; FUN_10201690) closes when it changes; ours closes the whole stack *(inf)*.
- Labels swap per frame through the child's own sprite (FUN_1011ae50; GameUi::lobby_window_swaps), from the s2c 0x0B4
  SAVE_CONF bytes (World.conf_sys): Seek Party 78 / Play Solo 257 (InviteFlg), Autogroup 474 / 484 (AutoPartyFlg; 485
  and disabled where zone flag 8 is off), Request 778 / Stop Request 780 (RecruitFlg), Level Sync 611 / Lv. Sync Off
  613 (a member with GAttr 0x100). The Find Member child is hidden and drawn by the window (FUN_10205790): 82 / 79
  Invite with the help 0xe 0x10 / 0xf, by whether the target is someone else seeking a party or in autogroup.
- Rows done: the three 0x0DC toggles (which 2 / 6 / 0xb; textcmd::config_flag gained 6 and 0xb), Find Member (the
  seeker search `/sea [region] inv`, title slot 2 0xd 0x13 "Party": FUN_10150ce0 is titlewin) / Invite (0x06E {id, 0,
  0}), Languages (prty5us: config 0x98, check marks keytops3 431, c2s 0x0DB kind 1 on open and on a changed close),
  Alarm (config 0xc7, windowps 483 mark, sounds 0027 0031 0033 0034 0038 0029 0022), Distribution view (group 10 0x02 /
  group 9 0x00, mode 0x79), Leave (0x06F 0), Disband (0x070 0 with own GAttr 4), Dissolve in prty7 (0x06F 5), prty4
  Form (0x06E {target, index, 5}) / Dissolve (0x070 5 with GAttr 8), prty8 Lottery (0x077 change 5 per quartermaster)
  / Quartermaster, Chg. Leader (the menu closes after the pick), Rmv. Member (us: nothing). The picks use the party-list
  cursor (SubThen::Party). The enables of FUN_10205ff0 / FUN_102061e0 / FUN_102062c0.
- s2c 0x0DC: unless AnonFlag bit 1, group 10 0x04 / 0x03 in mode 0x87 (the name as string 0, the pronoun FUN_101cc590 as
  int 1: "Ann invites you to his party.", "... to join a party." for pronoun 2; LSB sends RaceNo 0); a party invite plays
  the alarm; the invite is pending (keytops3 69 centred on the party window's top-left corner) until prtyjoin's Yes / No
  (0x074) or /join / /decline. prtyjoin's help is the question (group 10 0x00 / 0x01, printf %s). Not done: the name's
  0x7F 0xFC / 0xFB codes in the party line (FUN_100da580; the log draws no 0x7F codes yet).
- Not done: Member List, View Request, Level Sync (c2s 0x078 / s2c 0x0E1 and GROUP_LIST searches, levelsyn), Focus
  (/focustarget), Rearrange (local roster order), the request history prompt (comyn 9 0x140), the auto-pool area rule
  (actor +0x18c, group 9 0xc2), Monstrosity.
- Live (Lnxtest, Southern San d'Oria, bot Rosalind): prty1 Seek Party -> "Play Solo" once 0x0B4 came, and back;
  Languages and Alarm; prty3 with Alliance dim (no target); Rmv. Member through the party cursor (the menu stays, Chg.
  Leader / Rmv. Member dim with one member: LSB keeps a party of one); Chg. Leader (menu closed, main menu back on Party,
  the leader dot on the bot); prty2 (its leave row's windowps 80 reads "Disband"); Leave -> Solo and the window closed
  itself; Find Member with no target: 8 seekers in the region. The receiving side is untested live (no bot invites).

## Status window (2026-10-24)

Spec `~/ffxi/lab/research/status_window_spec.md` (30261006_0); ours client/status.rs.
- Main menu child 1 (FUN_10140400 case 5, SE 0002): persona in Status mode at (16, 16) with its group 0x30 (titlewin's
  bit too: GameUi::set_group; the walk hides the title bar), level at its layout place, statcom2 at (384, 48); c2s 0x061
  {0} with the busy strip until the next 0x061. Ours closes the main menu and brings it back on the Status row (retail:
  statcom2's group 0x81 suspends it).
- statcom2: links, SE 0001 / 0002 / 0004 / 0003; Enter on rows 1..5 / 12 shows profile, joblevel, btlskill, mgcskill,
  trdskill, masterle at (130, 48) (the shown row again: nothing); Play Time runs /playtime; Escape closes the page, else
  statcom2 (persona and level with it). Children 3 / 4 / 5 / 12 / 13 start disabled: 3..5 enabled once the stats are
  ready by the skills there are, 12 at level 99+ with 0x061 @0x66 bit 0, 13 at 99+ with key item 0xd21; 7 needs 0x063
  type 2 bit 13, 8 type 5's access and level 99+.
- Pages (two a row alternating left / right, FUN_10148030): job names windowps 212 + job (gold main, blue support),
  skills keytops3 208 + id (blue capped), crafts with the rank keytops3 193 + rank and "(level)"; heights ((n + 1) / 2)
  x 14 + 0x16, crafts n x 14 + 0x16. Profile: crest keytops3 74 + nation, race (table 0x10364cb0), home point in group 4
  (the 10-letter names: "SSandOria", so that is what retail shows), rank and the framesus 103 / 98 gauge, with the
  0x063 type 3 / 4 data the gladiator rank, prestige, mastery rank and mentor; "Title: <name>" in help slot 2. Level:
  "Level N" (0x067 @0x25), EXP / NEXTLevel with the gauge, or the master level's with an unlocked one.
- Persona (shared with Equipment): ILv is 0x061 @0x55 + 99 ("---/" for 0); the job names are windowps 212 + job (was
  text); the main job blue on 0x063 type 2 bit 14; encumbrance (0x01B @0x60 bits 0x10..0x18) reds the stat / max HP /
  max MP with keytops3 430; only the name until the stats are ready.
- New data: 0x061 exp / title / rank / rank points / home / nation / master level fields, 0x01B mentor / mastery / master
  levels, 0x063 types 2 / 3 / 4 / 5, 0x067 @0x25.
- Not done: Currencies 1 / 2 (evitem modes), Unity (ut_menu), Merit Points (merit1 / levmerit), Job Points (merit2ca),
  Alter Ego Points (fp_cat): logged; Monstrosity (mp_stat); the stats request retries.
- Live (Lnxtest, WAR 1): persona "Lv 1 Warrior", level 499 / 500, profile (New Adventurer, San d'Oria, Hume,
  SSandOria, rank 1, Gla, prestige 0), job levels, combat skills (blue capped Hand-to-Hand 4 / Evasion 5), Magic Skl.
  refused (SE 0004, no magic skills), crafts "Amateur (0)"; Escape twice back to the main menu on Status.

## Party window hand over the log (2026-10-24)

- FUN_10202110's hand (anc2s) sits at the party / alliance row's top-left and reaches left over the chat log. The
  registry layers (0x103722d8 +0x29) decide draw order: logwindo 1, raid1 / raid2 2, partywin 3, so retail draws it
  over the log. Ours drew it inside hud_screen, before the log, which covered it (the user: "behind the chat window").
  Now screens::push_party_hand runs after the log and the party rows' text (GameUi::party_hand; ui-shot too).
- Live (Lnxtest, F1): the hand left of our row, over the log's edge.

## The chat line's 120-byte limit (2026-10-24)

- FUN_101abc50 inserts typed or pasted text only when the raw line (E+0x888) and the expanded line (E+0xa50) stay
  within E+0xa54 (0x78 for the chat line); else nothing goes in (a whole paste is refused, not cut). Ours applied it to
  auto-translate only, so a typed line could reach 150 bytes; LSB's GP_CLI_COMMAND_CHAT_STD takes packets of 6..136
  bytes and dropped those (seen with a 133-char !exec, "Bad packet size ... got 140, expected [6, 136]"). The sender's
  own cap (FUN_100e3400, 0x96) stays.
- Test tool: the script's `type:` step now sends one Text event a character, as typing does.
- Live (Lnxtest): "/echo" + 125 digits stopped at 120 characters and echoed.

## Missions (2026-10-24)

Spec `~/ffxi/lab/research/quest_mission_spec.md` sections 2, 3, 5 (30261006_0); ours client/journal.rs (the store
and the row rules), client/missions.rs (the windows), dat/journal.rs (the name files).
- s2c 0x056 (LAB_10098ad0): data u32[8] @4, Port @0x24 into the store: quest areas' current (0x50 + 8 a, 0xE0, 0xF0,
  0x100) and completed (+0x40, 0xE8, 0xF8, 0x108) bits, 0xD0 the nations' and Zilart's completed missions, 0xD8 ToAU /
  WotG, 0x30 / 0x38 Campaign, 0xFFFF the mission record (CoP sub-path nibbles clamped to 9), 0xFFFE TVR; other ports
  refused; complete at received == 0x83ffffff. Reset per world entry.
- Name files: d_msg {u32 number, name, description}: missions 55715..55724 / 55735..55742 by log, quests 55706..55713,
  55722, 55739, 55740 by area (English); read on the first open.
- Social child 1: missionm (SE 0002) over the social menu (group 0x81 suspends it; Escape brings it back): children
  2..5 and 7 disabled without Zilart / CoP / ToAU / WotG / Adoulin (World.expansions = the lobby's bits & installed,
  FUN_1022dad0). Enter: the category's log (Nations = ours; Add-ons RoV until RoV 340, then TVR).
- miss00: waiting ("Preparing data." / "Please wait a moment.", the busy strip) until the store is complete; then
  "Current:", the current row (FUN_1022dfa0 per log family) or 0xc 0x38 / 0x39, "Completed:", every completed entry
  in file order (FUN_1022e3a0) or 0xc 0x3a; the title at (60, 10); entries with framesus 35, '_' indents of 6 px
  (none on the current row), wrapped names; the cursor row 0x80803040; headers skipped by Up / Down; Left / Right the
  log cycle (expansion bits, TVR after RoV 340), Ctrl / Alt a page; Enter the description box; the scroll bar and
  wheel (Owner::Journal). /mission (0x4e) opens it alone on the titled log or the nation's.
- Inferred: row bands beyond the count hidden; the held-key header stop not done.
- Live (Lnxtest, San d'Oria rank 1): "Smash the Orcish Scouts" current, its description; Bastok "Not allowed under
  current allegiance."; Zilart "No current missions."; Escape back to the social menu on Missions; /mission.

## Cursor memory and the held action menu (2026-10-24)

- FUN_1015e6e0 (every hide, and FUN_1015e790's close) runs FUN_10118830: the window's cursor child goes into its
  layout entry (window +4, the menu DAT's element, alive all session: +0x1a / +0x1c). FUN_1015e350 (open by name, new
  or existing window) runs FUN_10118870: without registry flag 0x10000 the cursor comes back from there (1 the first
  time); with it, child 1. Flag 0x10000 in this build: playermo, inventor, iuse, link5, quest00, quest01, miss00,
  evitem; not: menuwind, socialme, statcom2, prty*, missionm, chatctrl, mgcmenu / abimenu / magselec / abiselec,
  commenu, myroom / mogext, configwi, cmbmenu, searchma. Handlers' own open hooks may set it after (chatctrl does).
- The action menu's Magic / Abilities / Items / Trust rows open their list without closing playermo (FUN_1012b880
  cases 7 / 8, abiselec); the lists' groups (magic / ability 0x0c, abiselec 0x04) share playermo's 0x04, so the walk
  only suspends it, and resumes it with its cursor when the list closes (the cast callback LAB_1022d140 closes only
  the list), in battle or not. Reopened by name (Enter on a target, engaging) it starts on its first row.
- Ours: World.held_menu replaces reopen_menu (which rebuilt the menu on row 0, in battle only); World.cursor_mem
  per window name (kept every frame: statcom2, the party windows, missionm, the category menus) and menu_mem per main
  menu page (Minus opens on the last row). Not done: the other KEEP windows' own cursors (myroom, configwi, cmbmenu).
- Live (Lnxtest): action menu Items -> the item list -> Escape: the menu back on Items; the main menu closed on Magic
  reopened on Magic.

## Quests (2026-10-24)

Spec `~/ffxi/lab/research/quest_mission_spec.md` section 6; ours client/quests.rs.
- Social child 2 runs /quest (0x4d, FUN_1007d240, SE 0002): quest00 beside the social menu (group 0x1c, the menu stays
  drawn unfocused); its setup sends c2s 0x061 {0}; waiting with the busy strip until the 0x056 store is complete.
- quest00: five rows moved to y 14 / 46 / 110 / 142 / 174 (FUN_1022f670): Current, Completed, the RoE "Current" with
  "%s" + 10 spaces + "%d/%d" of the objectives taken (records 0..29 with an id, + 63, + 42) over 30, Objective List,
  Vana'Bout (enabled while a record 43..62 is set, else grey and not a cursor stop); the RoE panel (FUN_10233560):
  Sparks / Deeds (s2c 0x110, 0 before one), Unity Accolades (0x061 @0x58 bits 10..26 once stats are ready), Number
  Completed (the set bits of 0x112's first 0x173 bytes once Offset 3 came), Plaudits; 99999 in 0x80407090.
- quest01 over quest00 (its frame under it): one area's quests in file order: current (bit set and '+' or not
  completed) / completed; numbers 1039..1045, 1047..1049 read area 9 bits 39..45 / 47..49 (FUN_102333f0); the area cycle
  (5 Zilart, 6 ToAU, 7 WotG, 8 Abyssea, 9 / 10 Adoulin; 0..4 always), the titles "<area>: Current / Completed
  Quests", "No current / completed quests.", the description box, the scroll bar.
- Not done: Enter on the RoE rows 2..4 (their lists, FUN_10232b80, not traced): logged.
- Live (Lnxtest: empty lists, the panel at 0; Lnxcsfix with !addquest 0 10 / !completequest 0 11: "A Squire's Test"
  current with its description, "Grave Concerns" completed; both removed again with !delquest and checked).
