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
  flags1 @0x2C, server_status @0x30, dead_counter1 u32 @0x38 (60 Hz; includes 6 min, the client forces
  home point below that), dead_counter2 @0x3C.
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
  Quads are XYZRHW (FVF 0x144, rhw 1); anchors up to 100 px off screen still draw. Matches retail video frames. Colour = actor +0x78, chosen by FUN_1008c430
  from "menu    ncol" (23 sprites, first-cell colour): 0 white (players), 1 cyan (party), 4 green (NPCs),
  5 pale yellow (monsters), 6 red (claimed by us / party), 7 magenta (claimed by others), 8 orange, 9 grey,
  0x12..0x16 team colours. LSB marks living mobs with 0x00E @0x25 bit 0x08 and the claimer at @0x2C.
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
  out = mix(fog, rgb, f). Actors the same with vc = 0.5 (inferred). Sky: e = asin(-ray.y)/(pi/2), lerp
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
  0x100 from +0x12c bit 13). The client sends monsters (0x00E @0x25 bit 0x08) down the menu path (inferred).
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
  The random name itself (FUN_1022db10, tables DAT_10623958 / 70) is not implemented.

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
  Kept open while typing with more than 2 lines, and during events by FUN_1015ba60's rule (see "Messages" below).
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
- Stored and shown only: 0x13, 0x3a, 0x3b, 0x3e, 0x3f, 0x40, 0x42, 0x99 bits 2 / 6, 0x9a, 0xca, 0x9d, 0x9e, 0xa5,
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
  UNORM) and converted for the sRGB target, so blending happens in linear space unlike D3D8; batched sprites use
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
