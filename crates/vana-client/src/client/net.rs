//! The zone session on its own thread: decodes what the server sends into events and
//! keeps the session alive with our position. Follows zone changes to the next server.

use crate::net::lobby::MapLogin;
use crate::net::map::compress::Codec;
use crate::net::map::session::{MapSession, SubPacket, build};
use anyhow::Result;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Npc,
    Player,
}

#[derive(Clone, Debug)]
pub struct EntityUpdate {
    pub id: u32,
    pub index: u16,
    pub kind: EntityKind,
    pub pos: Option<[f32; 3]>,
    pub dir: u8,
    /// HP bit 0x04 also carries the status byte @0x1F; speed @0x1C comes with the position bit.
    pub hpp: Option<u8>,
    pub name: Option<String>,
    pub despawn: bool,
    /// Model ids (GrapIDTbl) when the update carries them.
    pub look: Option<[u16; 9]>,
    /// An NPC or monster model, when a 0x00E carries one.
    pub npc: Option<crate::dat::npc::NpcLook>,
    pub motion: MotionState,
    /// Claim owner u32 @0x2C (claim bit 0x02), 0 when unclaimed.
    pub claim: Option<u32>,
    /// The monster flag (actor +0x120 bit 13): @0x20 bit 0, read with masks 1 / 2 / 4. (@0x25 is the hitbox size.)
    pub mob: Option<bool>,
    /// Name-colour marks (mask 0x04), FUN_1008db40's inputs.
    pub marks: Option<Marks>,
    /// 0x00D mask 0x04, u32 @0x20 bit 15: male (the emote lines' [his/her]).
    pub male: Option<bool>,
    /// u32 @0x24 bits 29 (named: no article) and 30 (plural verbs), read by the 0x00D/0x00E handlers.
    pub name_flags: Option<(bool, bool)>,
    /// u16 @0x18 & 0x1fff, the movement counter (FUN_1008b610), and whether y is snapped to the ground:
    /// NPC ids without u32 @0x18 bit 15, kind (@0x30 & 7) not 2 / 3 (LAB_10099c80).
    pub counter: u16,
    pub ground: bool,
    /// Update mask (u8 @0x0a) and status bits u8 @0x20 & 7 (actor +0x120 bits 13 / 14 / 18, 0x1009a397).
    pub mask: u8,
    pub status: u8,
    /// u8 @0x30 & 7 (0x00E): the model kind byte.
    pub look_kind: u8,
    /// SubKind 2 (a door): the door id at @0x34 (FUN 0x1009cb37: actor +0xf8), sent on every update.
    pub door: Option<u32>,
    /// 0x00E with mask 0x04 (0x1009adbb): u8 @0x22 bit 0 sets actor +0x130 bit 20 and clears +0x128 bit 3,
    /// else +0x128 bit 3 = u8 @0x2B bit 0x20. Either hides the target gauge; bit 3 also the name plate.
    pub hp_bits: Option<(bool, bool)>,
    /// u8 @0x2B bit 0x08 with mask 0x04 (actor +0x130 bit 7, 0x1009ad2e): an attackable target is dropped.
    pub untargetable: Option<bool>,
    /// The face target's ActIndex, (u32 @0x18 >> 17) & 0x7fff with the position (actor +0x1f8; LSB facetarget).
    pub face: Option<u16>,
    /// Mask 0x04, u8 @0x28 bit 0x04 (LSB PetNewFlag; actor +0x12c bit 23): the model is made with 'spop'.
    pub spawn_fx: Option<bool>,
    /// 0x00E u32 @0x18 bit 16 (KingFlag): counted toward 0x00A SendCount before c2s 0x01A 0x14 (all.asm 167839).
    pub king: bool,
}

/// The bytes FUN_1008db40 colours a name by (0x00E / 0x00D, mask 0x04): charm @0x27 bit 3, trust @0x28 bit 0,
/// pet @0x28 bit 6, @0x2B bit 1, allegiance @0x29, GM level @0x23 & 7, yell (0x00E: call for help) u32 @0x20 bit 13;
/// PCs only (0x00D): LFG bit 11, anonymous bit 12, auto-party u32 @0x24 bit 31, LFG master @0x28 bit 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marks {
    pub charm: bool,
    pub trust: bool,
    pub npc_pet: bool,
    pub b2b1: bool,
    pub alleg: u8,
    pub gm: u8,
    pub yell: bool,
    pub lfg: bool,
    pub anon: bool,
    pub auto: bool,
    pub lfg_master: bool,
}

/// A battle message packet: 0x029 (FUN_1009cf70) or 0x02D (FUN_1009d1b0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BattleMessage {
    pub actor: u32,
    pub target: u32,
    pub actor_index: u16,
    pub target_index: u16,
    pub data: [i32; 2],
    pub message: u16,
}

/// 0x029: @4 caster, @8 target, @0xc / @0x10 data, @0x14 / @0x16 indices, @0x18 message.
pub fn battle_message(p: &SubPacket) -> BattleMessage {
    BattleMessage { actor: p.u32(4), target: p.u32(8), data: [p.u32(12) as i32, p.u32(16) as i32], actor_index: p.u16(0x14), target_index: p.u16(0x16), message: p.u16(0x18) }
}

/// 0x02D: @4 / @8 ids, @0xc / @0xe indices, @0x10 / @0x14 data, @0x18 message.
pub fn battle_message2(p: &SubPacket) -> BattleMessage {
    BattleMessage { actor: p.u32(4), target: p.u32(8), actor_index: p.u16(0xc), target_index: p.u16(0xe), data: [p.u32(0x10) as i32, p.u32(0x14) as i32], message: p.u16(0x18) }
}

/// Speed, status and size as POS_HEAD carries them (0x00A/0x00D/0x00E).
#[derive(Clone, Copy, Debug, Default)]
pub struct MotionState {
    /// Speed @0x1C and SpeedBase @0x1D, x0.1 = units per second.
    pub speed: u8,
    pub speed_base: u8,
    /// Status @0x1F (LSB Animation): 0 standing, 1 engaged, 2/3 dead, 33 resting...
    pub status: u8,
    /// GraphSize: u32 @0x20 bits 9-10.
    pub graph_size: u8,
}

fn motion_state(p: &SubPacket) -> MotionState {
    MotionState { speed: p.u8(0x1C), speed_base: p.u8(0x1D), status: p.u8(0x1F), graph_size: ((p.u32(0x20) >> 9) & 3) as u8 }
}

/// An event start: s2c 0x032 / 0x034 / 0x033, or the 0x00A login event.
#[derive(Clone, Debug, Default)]
pub struct EventStart {
    /// Target UniqueNo / ActIndex (the NPC, or ourselves).
    pub target: u32,
    pub index: u16,
    /// EventNum (zone), EventPara (event id), Mode, EventNum2, EventPara2.
    pub zone: u16,
    pub event: u16,
    pub mode: u16,
    pub zone2: u16,
    pub para2: u16,
    pub params: [u32; 8],
    pub strings: Vec<String>,
}

/// A zone dialogue line sent by the server: 0x036 (talk) or 0x02A (with num[4]).
#[derive(Clone, Debug, Default)]
pub struct ZoneText {
    /// The packet: 0x036, 0x02A, 0x027 or 0x043.
    pub packet: u16,
    pub actor: u32,
    pub index: u16,
    /// MesNum; bit 15 = no speaker name.
    pub mes: u16,
    /// Type: the log mode by table 0x1035b944 below 8.
    pub kind: u8,
    pub num: [i32; 8],
    /// 0x02A String: the speaker name when the server sends it.
    pub name: String,
    /// String slots from 0 (0x027 String1, 0x043 sName); 0x027's String2 and Flags.
    pub strings: Vec<String>,
    pub name2: String,
    pub flags: u8,
}

#[derive(Clone, Debug)]
pub enum NetEvent {
    /// `game_time` @0x3C (Earth seconds since the Vana'diel epoch), `weather` number @0x68; `area` the log line.
    ZoneIn { zone: u16, pos: [f32; 3], dir: u8, char_id: u32, index: u16, name: String, look: [u16; 9], motion: MotionState, game_time: u32, weather: u16, music: [u16; 5], event: Option<EventStart>, area: Option<AreaLine>, sub_map: u16, scene: u16, myroom: Option<MyRoom>, room_flag: bool, dead_counter: u32, mog_zone: bool, send_count: u16 },
    /// s2c 0x053 (GP_SERV_COMMAND_SYSTEMMES): para @4, para2 @8, Number @0xc (file 7031).
    SystemMessage { id: u16, para: u32, para2: u32 },
    /// s2c 0x0E0 (GP_SERV_COMMAND_GROUP_COMLINK): LinkshellNum @4, ItemIndex @5, Category (bag) @6.
    LinkshellEquip { num: u8, index: u8, bag: u8 },
    /// s2c 0x04D (GP_SERV_COMMAND_FRAGMENTS): the whole packet (client/servmes.rs reads it).
    Fragments(Vec<u8>),
    /// s2c 0x009 (GP_SERV_COMMAND_MESSAGE): MesNo @0xa, Data @0xd as keys (see std_data).
    MessageStd { id: u16, cas: u32, para: [i32; 4], strings: [String; 2] },
    /// s2c 0x0D2 (GP_SERV_COMMAND_TROPHY_LIST): TargetUniqueNo @8, Gold @0xc, TrophyItemNo @0x10, TrophyItemIndex @0x14,
    /// IsContainer @0x16.
    TrophyList { slot: u8, item: u16, target: u32, gold: u16, container: bool },
    /// s2c 0x0D3 (GP_SERV_COMMAND_TROPHY_SOLUTION): LootUniqueNo @4, EntryUniqueNo @8, EntryPoint @0x12,
    /// TrophyItemIndex @0x14, JudgeFlg @0x15, sLootName @0x16, sLootName2 @0x26.
    TrophySolution { slot: u8, judge: u8, loot: u32, entry: u32, entry_point: i16, loot_name: String, entry_name: String },
    Event(EventStart),
    /// 0x05C: event params into Z2..Z9.
    EventParams([u32; 8]),
    /// 0x052: release mode (low byte) and, for mode 2, the event id.
    EventRelease { mode: u8, event: u16 },
    /// 0x05B / 0x065: a position set by the server (WPOS), with its mode.
    Wpos { id: u32, pos: [f32; 3], dir: u8, mode: u8 },
    ZoneText(ZoneText),
    /// s2c 0x067 kind 2 (char sync): the character, the owner of the Mog House being visited (@0x20, our own client
    /// only; 0 = none / our own) and the 2F flag (@0x27 bit 0, the owner's when visiting).
    CharSync { id: u32, owner: u32, floors: bool, open: bool },
    /// s2c 0x11A (the answer to c2s 0x119): unlocked job emotes (u32 @4, bit job - 1) and chairs (u16 @8).
    EmoteUnlocks { jobs: u32, chairs: u16 },
    /// s2c 0x04B (GP_SERV_COMMAND_PBX_RESULT).
    Pbx(super::post::Pbx),
    /// s2c 0x01B (GP_MYROOM_DANCER): get_job_flag @0xc, job_lev @0x10 (jobs 0..15), job_lev2 @0x48 (16..23),
    /// sjobflg @0x44.
    JobInfo(super::moogle::JobInfo),
    /// s2c 0x02E (header only): open the moogle menu.
    MogMenu,
    /// s2c 0x0AE: the owned mounts, 64 bits (LSB: key items 3072 + n).
    Mounts(u64),
    /// s2c 0x119 u32 @0xfc: the mount recast in seconds (one shared timer).
    MountRecast(u32),
    /// 0x01C: container sizes (ItemNum, 1 + size).
    ItemMax([u8; 18], [u16; 18]),
    /// 0x01E: count of a slot (0 = emptied).
    ItemNum { container: u8, index: u8, count: u32 },
    /// 0x01F / 0x020: item in a slot.
    /// 0x01F / 0x020; 0x020 carries the 24-byte extdata (Attr @0x11).
    /// LockFlg (0x01F @0xc, 0x020 @0x10): 0 free, 5 equipped, 0x0F in use, 0x13 linkshell, 0x19, 0x1B mannequin.
    Item { container: u8, index: u8, id: u16, count: u32, ext: Option<[u8; 24]>, lock: u8 },
    /// 0x050: equipment slot -> container slot (index 0 = removed).
    Equip { slot: u8, index: u8, container: u8 },
    /// 0x061: our stats.
    CharStats(super::items::CharStats),
    /// 0x05F: a music slot changed.
    Music { slot: u16, id: u16 },
    /// Weather change (0x057): number @8, crossfade in Vana'diel minutes @0x0a.
    Weather(u16, u16),
    /// 0x058 (GP_SERV_COMMAND_ASSIST): UniqueNo @4, AssistNo @8 (the client reads only these).
    Assist { player: u32, target: u32 },
    /// s2c 0x055: key item table TableIndex @0x84, owned bits GetItemFlag[16] @4.
    KeyItems { table: u16, bits: [u32; 16] },
    /// Our own equipment changed (0x051).
    SelfLook([u16; 9]),
    /// The server is moving us to another zone; a ZoneIn follows.
    Zoning,
    Entity(EntityUpdate),
    /// Our own status (0x037): LSB Animation @0x30, HP% (u32 @0x28 bits 16-23), and while dead the
    /// seconds until the forced return home (dead_counter1 @0x38, 60 Hz, includes a 6 minute floor).
    /// `dead`: dead_counter1 u32 @0x3C (60 Hz ticks) and dead_counter2 u32 @0x40 (Earth seconds since the Vana'diel
    /// epoch), the time the server sends us home (@0x38 is Flags3).
    SelfStatus { status: u8, hpp: u8, dead: (u32, u32), pet: u16, flags: u32 },
    /// Our own HP, MP and TP (0x0DF).
    Vitals { hp: u32, mp: u32, tp: u32, hpp: u8, mpp: u8 },
    /// s2c 0x0C8 GROUP_TBL (kind 0 party / 5 alliance), 0x0DD / 0x0E2 GROUP_LIST (0x0E2 creates), 0x0DF GROUP_ATTR.
    PartyTable(Vec<super::party::TableEntry>),
    PartyList(super::party::ListUpdate, bool),
    PartyAttr(super::party::AttrUpdate),
    /// s2c 0x05A MOTIONMES (LAB_100a06e0): caster @4, target @8 (0 none), emote u16 @0x10, Param @0x12, Mode @0x16.
    Emote { caster: u32, target: u32, emote: u16, param: u16, mode: u8 },
    /// s2c 0x0BF REGISTRATION (battlefield registration NPCs): Result u16 @6, ActIndex u32 @0xC.
    Registration { index: u32, result: u16 },
    /// s2c 0x0A0 MAP_GROUP: a party member's position for the map.
    MapGroup { id: u32, zone: u16, pos: [f32; 3] },
    /// s2c 0x0F4 TRACKING_LIST: a Wide Scan result.
    TrackEntry(super::widescan::Entry),
    /// s2c 0x0F5 TRACKING_POS: the tracked target's position and state.
    TrackPos { index: u16, level: u8, pos: [f32; 3], state: u32 },
    /// s2c 0x0F6 TRACKING_STATE: the list state (1 start, 2 end).
    TrackState(u32),
    /// s2c 0x031 RECIPE, 0x059 FRIENDPASS, 0x10E REQSUBMAPNUM: answers to event ops, whole packet with its header.
    EventAnswer { id: u16, data: Vec<u8> },
    /// s2c 0x0F9 RES {UniqueNo @4, ActIndex @8, u16 type @0xA}: 1 raise offered, 2 tractor (handler 0x100a2540).
    ResOffer { id: u32, kind: u16 },
    /// s2c 0x119 ABIL_RECAST: 31 x {u16 seconds, u8, u8 timer id, u16, u16} (FUN_10099430).
    AbilityRecasts(Vec<(u16, u16)>),
    /// s2c 0x0AA MAGIC_DATA: learned spells, one bit per spell id.
    MagicKnown(Vec<u8>),
    /// s2c 0x0AC COMMAND_DATA: weapon skill, job ability, pet command bits (64 bytes each), traits (32).
    CommandsKnown(Vec<u8>),
    /// s2c 0x017: Kind @4, Attr @5 (bit 0 GM), Data @6 (the yell's zone; assist: mastery, mentor rank), name @8,
    /// text @0x17.
    Chat { kind: u8, attr: u8, data: u16, from: String, text: String },
    Battle(BattleMessage),
    Action(crate::net::action::Action),
    /// 0x038: run scheduler `name` (a 4cc such as "kesu") on an actor.
    Scheduler { actor: u32, target: u32, name: [u8; 4] },
    /// s2c 0x11E: ActIndex @4 used /jump.
    Jump(u16),
    /// s2c 0x030 (GP_SERV_COMMAND_EFFECT): UniqueNo @4, ActIndex @8, EffectNum @0xa, Type @0xc, Status @0xd, Timer @0xe.
    SynthEffect { id: u32, index: u16, effect: i16, kind: i8, status: i8, timer: u16 },
    /// s2c 0x06F / 0x070 (COMBINE_ANS / COMBINE_INF).
    SynthResult(super::synth::Answer),
    SynthOthers(super::synth::Answer),
    Logout { state: u32 },
    Status(String),
    Error(String),
}

/// What FUN_100f7de0 prints on zone-in by LoginState (0x00A u32 @0x80).
#[derive(Clone, Copy, Debug)]
pub enum AreaLine {
    /// "=== Area: <group 5 name> ===" (FUN_100f8730).
    Zone(u16),
    /// Group 9 0x126 + floor, "=== Area: Mog House 1F ===" (FUN_100f87c0).
    MogHouse(u8),
}

/// The Mog House from a MYROOM 0x00A (LoginState 1): MyroomMapNumber @0xaa, floor (u8 @0xa8 >> 1) & 3 (0 = 1F),
/// MyRoomExitBit @0xae.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MyRoom {
    pub map: u16,
    pub floor: u8,
    pub exit_bit: u8,
}

/// LAB_100facf0 / FUN_10094190: the scene (map number) a 0x00A loads: MyroomMapNumber in the Mog House, else
/// MapNumber @0x42 (+ 0x1bc from 0x100, e.g. 256 Western Adoulin -> 0x2bc).
pub fn scene_of(p: &SubPacket) -> (u16, Option<MyRoom>) {
    if p.u32(0x80) == 1 {
        let room = MyRoom { map: p.u16(0xaa), floor: (p.u8(0xa8) >> 1) & 3, exit_bit: p.u8(0xae) };
        return (room.map, Some(room));
    }
    let map = p.u16(0x42);
    (if map >= 0x100 { map + 0x1bc } else { map }, None)
}

/// FUN_100f7de0: GAME (2), 0 and 6 print the area; MYROOM (1) the area for maps 0x118 / 0x11d, else the Mog
/// House floor when FUN_100f6740 says there is one; 3..5 (exits) nothing.
pub fn area_line(p: &SubPacket) -> Option<AreaLine> {
    let zone = p.u16(0x30);
    match p.u32(0x80) {
        1 if matches!(p.u16(0x42), 0x118 | 0x11d) => Some(AreaLine::Zone(zone)),
        1 => {
            let floor = (p.u8(0xa8) >> 1) & 3;
            (floor != 0 || (0x121..=0x123).contains(&p.u16(0xaa))).then_some(AreaLine::MogHouse(floor))
        }
        3..=5 => None,
        _ => Some(AreaLine::Zone(zone)),
    }
}

#[derive(Debug)]
pub enum NetCommand {
    /// c2s 0x0B5: kind 0 say, 1 shout, 4 party, 5 linkshell, 0x1a yell, 0x1b linkshell 2, 0x21 unity.
    Chat { kind: u8, text: String },
    /// c2s 0x0B6: a tell (name 15 bytes, then the message).
    Tell { name: String, text: String },
    /// c2s 0x0E8 {u32 Mode}: 0 toggle, 1 on, 2 off (/heal).
    Heal(u32),
    /// c2s 0x0E7 {u16 Mode, u16 Kind}: Mode 0 toggle, 1 on, 2 off, 3 shutdown on; Kind 1 logout, 3 shutdown.
    Logout { mode: u16, kind: u16 },
    /// Accept "Return to home point" while dead (0x01A action 0x0B).
    HomePoint,
    /// 0x01A on a target: 2 attack, 4 disengage, 0x0f change target (GP_CLI_COMMAND_ACTION).
    Act { target: u32, index: u16, action: u16 },
    /// 0x0DD equip inspect: kind 0 /check, 1 /checkname, 2 /checkparam.
    Check { target: u32, index: u16, kind: u8 },
    /// Any c2s packet: id and body (after the 4-byte header).
    Packet { id: u16, body: Vec<u8> },
    /// c2s 0x076 {u8 Kind = 0}: the roster's names (FUN_100e9550); LSB answers with 0x0C8 and 0x0DD.
    GroupListReq,
    /// Touched a zone line: its tag, and where we are.
    ZoneLine { tag: u32, pos: [f32; 3] },
    /// c2s 0x05B: mode 0 end, 1 update.
    EventEnd { target: u32, index: u16, option: u32, zone: u16, event: u16, mode: u16 },
    /// c2s 0x05C: event position update (mode 1).
    EventPos { pos: [f32; 3], dir: u8, target: u32, index: u16, option: u32, zone: u16, event: u16 },
    /// c2s 0x050: equip container slot `index` (0 = remove) into equipment `slot`.
    Equip { index: u8, slot: u8, container: u8 },
    /// c2s 0x061 (FUN_100967e0): ask for our stats (0x061 and friends); 1 from the Equipment command.
    StatusRequest(u8),
    /// c2s 0x016 CHARREQ (FUN_100ad230): ActIndex @4; LSB answers with a full 0x00E / 0x00D.
    CharReq(u16),
    /// c2s 0x05E RectID 'zmrq' (FUN_1008aec0): leave the Mog House, MyRoomExitBit @0x16, MyRoomExitMode @0x17 (0 the
    /// area entered from, 1..4 the exit list, 0x7d / 0x7e change floors, 0x7f Mog Garden).
    MyRoomExit { bit: u8, mode: u8 },
    /// c2s 0x0F2 (FUN_100fc680) {u16 State @4, u16 SubMapNumber @6}: 1 a sub-area loaded (0 = none), 2 from an
    /// event (op 0x75 sub 2).
    SubMap { state: u16, id: u16 },
}

/// c2s 0x05B body (FUN_1009ebb0 / FUN_1009ed00).
pub fn event_end_body(target: u32, index: u16, option: u32, zone: u16, event: u16, mode: u16) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&target.to_le_bytes());
    b.extend_from_slice(&option.to_le_bytes());
    b.extend_from_slice(&index.to_le_bytes());
    b.extend_from_slice(&mode.to_le_bytes());
    b.extend_from_slice(&zone.to_le_bytes());
    b.extend_from_slice(&event.to_le_bytes());
    b
}

/// s2c 0x032 / 0x034 / 0x033 as LSB writes them (offsets include the header).
pub fn event_start(p: &SubPacket) -> EventStart {
    match p.id {
        0x034 => EventStart {
            target: p.u32(4),
            params: std::array::from_fn(|i| p.u32(8 + 4 * i)),
            index: p.u16(0x28),
            zone: p.u16(0x2a),
            event: p.u16(0x2c),
            mode: p.u16(0x2e),
            zone2: p.u16(0x30),
            para2: p.u16(0x32),
            strings: Vec::new(),
        },
        0x033 => {
            let mode = p.u16(0xe);
            EventStart {
                target: p.u32(4),
                index: p.u16(8),
                zone: p.u16(0xa),
                event: p.u16(0xc),
                mode,
                zone2: p.u16(0xa),
                para2: 0,
                // Data[8] counts only with Mode & 8 (1009ea60).
                params: if mode & 8 != 0 { std::array::from_fn(|i| p.u32(0x50 + 4 * i)) } else { [0; 8] },
                strings: (0..4).map(|i| p.str(0x10 + 16 * i, 16)).collect(),
            }
        }
        _ => EventStart { target: p.u32(4), index: p.u16(8), zone: p.u16(0xa), event: p.u16(0xc), mode: p.u16(0xe), zone2: p.u16(0x10), para2: p.u16(0x12), ..Default::default() },
    }
}

/// What we report about ourselves every tick.
#[derive(Clone, Copy, Default)]
pub struct SelfState {
    pub pos: [f32; 3],
    pub dir: u8,
    pub moving: bool,
}

pub struct NetHandle {
    pub events: Receiver<NetEvent>,
    pub commands: Sender<NetCommand>,
    pub me: Arc<Mutex<SelfState>>,
    pub stats: Arc<NetStats>,
}

/// Datagram bytes sent and received (whole UDP payloads, as FUN_100f9b80 / FUN_100f9b90 count them) and
/// when the last server datagram came, in ms since the session started.
#[derive(Default)]
pub struct NetStats {
    pub sent: std::sync::atomic::AtomicU64,
    pub received: std::sync::atomic::AtomicU64,
    pub last_rx_ms: std::sync::atomic::AtomicU64,
    pub start: std::sync::OnceLock<Instant>,
}

impl NetStats {
    pub fn now_ms(&self) -> u64 {
        self.start.get_or_init(Instant::now).elapsed().as_millis() as u64
    }
}

/// c2s 0x016 body: u16 ActIndex, u16 pad.
pub fn charreq_body(index: u16) -> [u8; 4] {
    let [lo, hi] = index.to_le_bytes();
    [lo, hi, 0, 0]
}

/// c2s 0x01A body (FUN_10077120, size 0x1c): UniqueNo, ActIndex, ActionID, ActionBuf[4] zeroed.
pub fn action_body(target: u32, index: u16, action: u16) -> Vec<u8> {
    let mut body = vec![0u8; 24];
    body[0..4].copy_from_slice(&target.to_le_bytes());
    body[4..6].copy_from_slice(&index.to_le_bytes());
    body[6..8].copy_from_slice(&action.to_le_bytes());
    body
}

/// s2c 0x009's Data (LAB_100fbd40): each key found anywhere (strstr), numbers by atoi, string2 / string3 up to 24
/// bytes to a space, tab or NUL. TarUniqueNo lands in CasUniqueNo's slot (retail's), so it wins when present.
/// Returns (cas, Para0..3, [string2, string3]).
fn std_data(data: &str) -> (u32, [i32; 4], [String; 2]) {
    let after = |key: &str| data.find(key).map(|i| data[i + key.len()..].trim_start_matches([' ', '\t']));
    let num = |key: &str| {
        after(key).map_or(0, |v| {
            let end = v.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && c == '-'))).map_or(v.len(), |(i, _)| i);
            v[..end].parse::<i64>().unwrap_or(0) as i32
        })
    };
    let word = |key: &str| after(key).map_or(String::new(), |v| v.split([' ', '\t', '\0']).next().unwrap_or("").chars().take(24).collect());
    let cas = if data.contains("TarUniqueNo") { num("TarUniqueNo") } else { num("CasUniqueNo") };
    (cas as u32, [num("Para0"), num("Para1"), num("Para2"), num("Para3")], [word("string2"), word("string3")])
}

/// Chat text as sent: Shift-JIS cut to 150 bytes at a character's end (a pair or a phrase block is not split).
fn chat_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    let mut block = Vec::new();
    for ch in text.chars() {
        let b = crate::dat::font::encode_sjis(ch.encode_utf8(&mut buf));
        // A phrase is four chars; it goes whole or not at all.
        let cp = ch as u32;
        if (crate::dat::font::PHRASE_BASE..crate::dat::font::PHRASE_BASE + 0x400).contains(&cp) {
            block.extend_from_slice(&b);
            if (cp - crate::dat::font::PHRASE_BASE) / 0x100 < 3 {
                continue;
            }
        }
        let piece = if block.is_empty() { b } else { std::mem::take(&mut block) };
        if out.len() + piece.len() > 150 {
            break;
        }
        out.extend_from_slice(&piece);
    }
    out
}

fn text(p: &SubPacket, at: usize, len: usize) -> String {
    p.str(at, len)
}

fn look_at(p: &SubPacket, at: usize) -> [u16; 9] {
    std::array::from_fn(|i| p.u16(at + i * 2))
}

fn entity(p: &SubPacket, kind: EntityKind) -> EntityUpdate {
    let flags = p.u8(10);
    let name_at = if kind == EntityKind::Npc { 0x34 } else { 0x5A };
    EntityUpdate {
        id: p.u32(4),
        index: p.u16(8),
        kind,
        pos: (flags & 1 != 0).then(|| [p.f32(12), p.f32(16), p.f32(20)]),
        dir: p.u8(11),
        hpp: (flags & 0x04 != 0).then(|| p.u8(0x1E)),
        // 0x00E (asm 0x1009b666..): index >= 0x700 takes @0x34; below 0x400 only text marked 0x01 at @0x34 (from @0x35);
        // other NPCs are named from the zone list (the client side). Doors carry their door id there: not a name.
        name: (flags & 0x08 != 0 && p.data.len() > name_at)
            .then(|| match kind {
                EntityKind::Npc if p.u16(8) >= 0x700 => Some(text(p, name_at, 16)),
                EntityKind::Npc if p.u16(8) < 0x400 && p.u8(name_at) == 0x01 && p.u8(name_at + 1) > 0x20 => Some(text(p, name_at + 1, 15)),
                EntityKind::Npc => None,
                _ => Some(text(p, name_at, 16)),
            })
            .flatten()
            .filter(|n| !n.is_empty() && n.chars().all(|c| !c.is_control() && c != '\u{fffd}')),
        despawn: flags & 0x20 != 0,
        look: (kind == EntityKind::Player && flags & 0x10 != 0 && p.data.len() >= 0x48 + 18).then(|| look_at(p, 0x48)),
        motion: motion_state(p),
        // LSB writes the look on every update but sets bit 0x10 only for equipped looks (spawn mask
        // 0x57); monsters spawn with 0x0F.
        npc: (kind == EntityKind::Npc && flags & 0x20 == 0 && p.data.len() >= 0x30 + 20).then(|| crate::dat::npc::parse_look(&p.data[0x30..0x44]).ok()).flatten(),
        claim: (kind == EntityKind::Npc && flags & 0x02 != 0).then(|| p.u32(0x2C)),
        mob: (kind == EntityKind::Npc && flags & 7 != 0).then(|| p.u8(0x20) & 1 != 0),
        male: (kind == EntityKind::Player && flags & 0x04 != 0).then(|| p.u32(0x20) >> 15 & 1 != 0),
        marks: (flags & 0x04 != 0 && p.data.len() >= 0x2C).then(|| {
            let pc = kind == EntityKind::Player;
            Marks {
                charm: p.u8(0x27) & 0x08 != 0,
                trust: p.u8(0x28) & 0x01 != 0,
                npc_pet: p.u8(0x28) & 0x40 != 0,
                b2b1: p.u8(0x2B) & 0x02 != 0,
                alleg: p.u8(0x29),
                gm: p.u8(0x23) & 7,
                yell: p.u32(0x20) >> 13 & 1 != 0,
                lfg: pc && p.u32(0x20) >> 11 & 1 != 0,
                anon: pc && p.u32(0x20) >> 12 & 1 != 0,
                auto: pc && p.u32(0x24) >> 31 != 0,
                lfg_master: pc && p.u8(0x28) & 0x02 != 0,
            }
        }),
        name_flags: (flags & 0x04 != 0 && p.data.len() >= 0x28).then(|| (p.u32(0x24) >> 29 & 1 != 0, p.u32(0x24) >> 30 & 1 != 0)),
        counter: p.u16(0x18) & 0x1fff,
        ground: p.u32(4) >> 24 == 1 && p.u32(0x18) & 0x8000 == 0 && !matches!(p.u8(0x30) & 7, 2 | 3),
        mask: flags,
        status: p.u8(0x20) & 7,
        look_kind: p.u8(0x30) & 7,
        door: (kind == EntityKind::Npc && p.u8(0x30) & 7 == 2 && p.data.len() >= 0x38).then(|| p.u32(0x34)).filter(|d| *d != 0),
        untargetable: (kind == EntityKind::Npc && flags & 0x04 != 0 && p.data.len() >= 0x2C).then(|| p.u8(0x2B) & 0x08 != 0),
        hp_bits: (kind == EntityKind::Npc && flags & 0x04 != 0 && p.data.len() >= 0x2C).then(|| (p.u8(0x22) & 1 != 0, p.u8(0x22) & 1 == 0 && p.u8(0x2B) & 0x20 != 0)),
        face: (flags & 1 != 0).then(|| ((p.u32(0x18) >> 17) & 0x7fff) as u16),
        spawn_fx: (flags & 0x04 != 0 && p.data.len() > 0x28).then(|| p.u8(0x28) & 0x04 != 0),
        king: kind == EntityKind::Npc && p.u32(0x18) >> 16 & 1 != 0,
    }
}

impl EntityUpdate {
    /// A full update (actor +0x141): 0x00E needs mask 0x07, 0x17 for kinds 1 / 7 (0x1009a097); 0x00D 0x1f
    /// (0x100982f6).
    pub fn complete(&self) -> bool {
        let need = match self.kind {
            EntityKind::Player => 0x1f,
            _ if matches!(self.look_kind, 1 | 7) => 0x17,
            _ => 0x07,
        };
        self.mask & need == need
    }
}

pub fn spawn(login: MapLogin, account: String, codec: Codec) -> NetHandle {
    let (events_tx, events) = channel();
    let (commands, commands_rx) = channel();
    let me = Arc::new(Mutex::new(SelfState::default()));
    let shared = me.clone();
    let stats = Arc::new(NetStats::default());
    stats.now_ms();
    let st = stats.clone();
    std::thread::Builder::new()
        .name("zone-net".into())
        .spawn(move || {
            if let Err(e) = run(login, &account, &codec, &events_tx, &commands_rx, &shared, &st) {
                let _ = events_tx.send(NetEvent::Error(format!("{e:#}")));
            }
        })
        .expect("spawning the network thread");
    NetHandle { events, commands, me, stats }
}

enum Outcome {
    /// Moved to another zone server; the key advances by two per zone.
    Zone(SocketAddr),
    Done,
}

fn run(mut login: MapLogin, account: &str, codec: &Codec, tx: &Sender<NetEvent>, rx: &Receiver<NetCommand>, me: &Mutex<SelfState>, stats: &NetStats) -> Result<()> {
    let mut addr = SocketAddr::from((login.map.0, login.map.1));
    loop {
        match session(addr, &login, account, codec, tx, rx, me, stats)? {
            Outcome::Zone(next) => {
                let counter = u32::from_le_bytes(login.key[16..20].try_into().unwrap()).wrapping_add(2);
                login.key[16..20].copy_from_slice(&counter.to_le_bytes());
                addr = next;
                tx.send(NetEvent::Zoning)?;
            }
            Outcome::Done => return Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn session(addr: SocketAddr, login: &MapLogin, account: &str, codec: &Codec, tx: &Sender<NetEvent>, rx: &Receiver<NetCommand>, me: &Mutex<SelfState>, stats: &NetStats) -> Result<Outcome> {
    use std::sync::atomic::Ordering::Relaxed;
    let mut map = MapSession::connect(addr, &login.key, codec.clone())?;
    let (mut rx_seen, mut tx_seen) = (0u64, 0u64);
    let mut attempt = 1u16;
    map.send_login(login.char_id, &login.name, account, attempt)?;
    let mut answered = false;
    let mut zoned = false;
    let mut queue: Vec<Vec<u8>> = Vec::new();
    let mut last_send = Instant::now();
    let mut last_login = Instant::now();
    let mut my_index = 0u16;
    loop {
        if let Some(packets) = map.recv(Duration::from_millis(20))? {
            answered = true;
            stats.received.fetch_add(map.bytes_in - rx_seen, Relaxed);
            rx_seen = map.bytes_in;
            stats.last_rx_ms.store(stats.now_ms(), Relaxed);
            for p in packets {
                match p.id {
                    0x00A => {
                        let pos = [p.f32(12), p.f32(16), p.f32(20)];
                        let (scene, myroom) = scene_of(&p);
                        *me.lock().unwrap() = SelfState { pos, dir: p.u8(11), moving: false };
                        zoned = true;
                        my_index = p.u16(8);
                        queue.push(build(0x00C, &[0u8; 8]));
                        queue.push(build(0x011, &[2, 0, 0, 0]));
                        // EventNum @0x62, EventPara @0x64, EventMode @0x66, EventNo @0x40: the client starts
                        // the login event itself, on the player (FUN_10094ea0).
                        let (event_num, event_para) = (p.u16(0x62), p.u16(0x64));
                        let event = (event_num != 0).then(|| EventStart { target: p.u32(4), index: p.u16(8), zone: event_num, event: event_para, mode: p.u16(0x66), zone2: p.u16(0x40), ..Default::default() });
                        tx.send(NetEvent::ZoneIn { zone: p.u32(48) as u16, pos, dir: p.u8(11), char_id: p.u32(4), index: p.u16(8), name: text(&p, 132, 16), look: look_at(&p, 68), motion: motion_state(&p), game_time: p.u32(0x3C), weather: p.u16(0x68), music: std::array::from_fn(|i| p.u16(0x56 + 2 * i)), event, area: area_line(&p), sub_map: if p.u16(0x60) != scene && myroom.is_none() { p.u16(0x60) } else { 0 }, scene, myroom, room_flag: matches!(p.u32(0x80), 1 | 4 | 5), dead_counter: p.u32(0xA4), mog_zone: p.u8(0xaf) & 1 != 0, send_count: p.u16(0xac) })?;
                    }
                    0x00B => {
                        let state = p.u8(4);
                        // Acknowledge with the old key before leaving (0x00D).
                        map.send(&[build(0x00D, &[0u8; 4])])?;
                        if state == 2 {
                            let ip = Ipv4Addr::from(p.u32(8).to_le_bytes());
                            return Ok(Outcome::Zone(SocketAddr::from((ip, p.u32(12) as u16))));
                        }
                        tx.send(NetEvent::Logout { state: state as u32 })?;
                        return Ok(Outcome::Done);
                    }
                    0x00D => tx.send(NetEvent::Entity(entity(&p, EntityKind::Player)))?,
                    0x037 => {
                        let status = p.u8(0x30);
                        // u32 @0x34 >> 3: the pet's ActIndex (effect_filter.md).
                        tx.send(NetEvent::SelfStatus { status, hpp: (p.u32(0x28) >> 16) as u8, dead: (p.u32(0x3C), p.u32(0x40)), pet: (p.u32(0x34) >> 3) as u16, flags: p.u32(0x28) })?;
                    }
                    0x05A => tx.send(NetEvent::Emote { caster: p.u32(4), target: p.u32(8), emote: p.u16(0x10), param: p.u16(0x12), mode: p.u8(0x16) })?,
                    0x031 | 0x059 | 0x10E => tx.send(NetEvent::EventAnswer { id: p.id, data: p.data.clone() })?,
                    0x0F4 => tx.send(NetEvent::TrackEntry(super::widescan::Entry {
                        index: p.u16(4),
                        level: p.u8(6),
                        kind: p.u8(7) & 7,
                        dx: p.u16(8) as i16,
                        dz: p.u16(0xa) as i16,
                        name: text(&p, 0xc, 16),
                    }))?,
                    0x0F5 => tx.send(NetEvent::TrackPos { index: p.u16(0x12), level: p.u8(0x10), pos: [p.f32(4), p.f32(8), p.f32(0xc)], state: p.u32(0x14) })?,
                    0x0F6 => tx.send(NetEvent::TrackState(p.u32(4)))?,
                    0x0A0 => tx.send(NetEvent::MapGroup { id: p.u32(4), zone: p.u16(8), pos: [p.f32(0xc), p.f32(0x10), p.f32(0x14)] })?,
                    0x0BF => tx.send(NetEvent::Registration { index: p.u32(0xC), result: p.u16(6) })?,
                    0x0F9 => tx.send(NetEvent::ResOffer { id: p.u32(4), kind: p.u16(0xA) })?,
                    0x0AE => tx.send(NetEvent::Mounts(p.u32(4) as u64 | (p.u32(8) as u64) << 32))?,
                    0x119 if { let _ = tx.send(NetEvent::MountRecast(p.u32(0xfc))); false } => {}
                    0x119 => tx.send(NetEvent::AbilityRecasts((0..31).map(|i| (p.u8(4 + 8 * i + 3) as u16, p.u16(4 + 8 * i))).collect()))?,
                    0x0AA => tx.send(NetEvent::MagicKnown((0..128).map(|i| p.u8(4 + i)).collect()))?,
                    0x0AC => tx.send(NetEvent::CommandsKnown((0..224).map(|i| p.u8(4 + i)).collect()))?,
                    0x0DF => {
                        if p.u32(4) == login.char_id {
                            tx.send(NetEvent::Vitals { hp: p.u32(8), mp: p.u32(12), tp: p.u32(16), hpp: p.u8(0x16), mpp: p.u8(0x17) })?;
                        }
                        let jobs = std::array::from_fn(|i| p.u8(0x20 + i));
                        tx.send(NetEvent::PartyAttr(super::party::AttrUpdate { id: p.u32(4), index: p.u16(0x14), hp: p.u32(8), mp: p.u32(12), tp: p.u32(16), hpp: p.u8(0x16), mpp: p.u8(0x17), jobs }))?;
                    }
                    // Kinds 0 (party) and 5 (alliance) share list 0; LSB sends no others.
                    0x0C8 if matches!(p.u8(4), 0 | 5) => {
                        let entries = (0..20).map(|k| (p.u32(8 + 12 * k), p.u16(12 + 12 * k), p.u8(14 + 12 * k), p.u16(16 + 12 * k))).collect();
                        tx.send(NetEvent::PartyTable(entries))?;
                    }
                    0x0DD | 0x0E2 if matches!(p.u8(0x1C), 0 | 5) => {
                        let jobs = std::array::from_fn(|i| p.u8(0x22 + i));
                        let u = super::party::ListUpdate { id: p.u32(4), index: p.u16(0x18), number: p.u8(0x1A), name: text(&p, 0x28, 16), hp: p.u32(8), mp: p.u32(12), tp: p.u32(16), hpp: p.u8(0x1D), mpp: p.u8(0x1E), zone: p.u16(0x20), flags: p.u32(0x14), jobs };
                        tx.send(NetEvent::PartyList(u, p.id == 0x0E2))?;
                    }
                    0x029 => tx.send(NetEvent::Battle(battle_message(&p)))?,
                    0x02D => tx.send(NetEvent::Battle(battle_message2(&p)))?,
                    0x028 => tx.send(NetEvent::Action(crate::net::action::parse(&p.data)))?,
                    0x038 => tx.send(NetEvent::Scheduler { actor: p.u32(4), target: p.u32(8), name: p.u32(12).to_le_bytes() })?,
                    0x051 => tx.send(NetEvent::SelfLook(look_at(&p, 4)))?,
                    0x00E => tx.send(NetEvent::Entity(entity(&p, EntityKind::Npc)))?,
                    0x057 => tx.send(NetEvent::Weather(p.u16(8), p.u16(0x0a)))?,
                    // s2c 0x009 (LAB_100fbd40): MesNo @0xa, its parameters as "Para0 %u Para1 %u ..." text in Data @0xd.
                    0x009 => {
                        let data = text(&p, 0xd, p.data.len().saturating_sub(0xd));
                        let d = std_data(&data);
                        tx.send(NetEvent::MessageStd { id: p.u16(0xa), cas: d.0, para: d.1, strings: d.2 })?
                    }
                    0x053 => tx.send(NetEvent::SystemMessage { id: p.u16(0xc), para: p.u32(4), para2: p.u32(8) })?,
                    0x0E0 => tx.send(NetEvent::LinkshellEquip { num: p.u8(4), index: p.u8(5), bag: p.u8(6) })?,
                    0x04D => tx.send(NetEvent::Fragments(p.data.clone()))?,
                    0x0D2 => tx.send(NetEvent::TrophyList { slot: p.u8(0x14), item: p.u16(0x10), target: p.u32(8), gold: p.u16(0xc), container: p.u8(0x16) != 0 })?,
                    0x0D3 => tx.send(NetEvent::TrophySolution {
                        slot: p.u8(0x14),
                        judge: p.u8(0x15),
                        loot: p.u32(4),
                        entry: p.u32(8),
                        entry_point: p.u16(0x12) as i16,
                        loot_name: text(&p, 0x16, 16),
                        entry_name: text(&p, 0x26, 16),
                    })?,
                    0x058 => tx.send(NetEvent::Assist { player: p.u32(4), target: p.u32(8) })?,
                    0x055 if p.data.len() >= 0x86 => tx.send(NetEvent::KeyItems { table: p.u16(0x84), bits: std::array::from_fn(|i| p.u32(4 + 4 * i)) })?,
                    0x05F => tx.send(NetEvent::Music { slot: p.u16(4), id: p.u16(6) })?,
                    0x017 => tx.send(NetEvent::Chat { kind: p.u8(4), attr: p.u8(5), data: p.u16(6), from: text(&p, 8, 15), text: text(&p, 23, 150) })?,
                    0x032..=0x034 => tx.send(NetEvent::Event(event_start(&p)))?,
                    0x05C => tx.send(NetEvent::EventParams(std::array::from_fn(|i| p.u32(4 + 4 * i))))?,
                    0x052 => tx.send(NetEvent::EventRelease { mode: p.u8(4), event: (p.u32(4) >> 8) as u16 })?,
                    0x05B | 0x065 => tx.send(NetEvent::Wpos { id: p.u32(0x10), pos: [p.f32(4), p.f32(8), p.f32(0xc)], dir: p.u8(0x17), mode: p.u8(0x16) })?,
                    0x036 => tx.send(NetEvent::ZoneText(ZoneText { packet: 0x036, actor: p.u32(4), index: p.u16(8), mes: p.u16(0xa), kind: p.u8(0xc), ..Default::default() }))?,
                    0x02A => tx.send(NetEvent::ZoneText(ZoneText {
                        packet: 0x02A,
                        actor: p.u32(4),
                        num: std::array::from_fn(|i| if i < 4 { p.u32(8 + 4 * i) as i32 } else { 0 }),
                        index: p.u16(0x18),
                        mes: p.u16(0x1a),
                        kind: p.u8(0x1c),
                        name: text(&p, 0x1e, 32),
                        ..Default::default()
                    }))?,
                    // 0x027 (handler 0x100a1dc0): Num1[4] @0x10 -> num 0..3, Num2[4..7] @0x60 -> num 4..7 (Num2[0..3] unread),
                    // String1 @0x20 -> string 0, String2 @0x40, Flags @0xe.
                    0x027 => tx.send(NetEvent::ZoneText(ZoneText {
                        packet: 0x027,
                        actor: p.u32(4),
                        index: p.u16(8),
                        mes: p.u16(0xa),
                        kind: p.u16(0xc).min(255) as u8,
                        flags: p.u8(0xe),
                        num: std::array::from_fn(|i| if i < 4 { p.u32(0x10 + 4 * i) as i32 } else { p.u32(0x50 + 4 * i) as i32 }),
                        strings: vec![text(&p, 0x20, 32)],
                        name2: text(&p, 0x40, 16),
                        ..Default::default()
                    }))?,
                    // 0x043 (handler 0x100a1070): fishing's monster / chest; sName @0x10 -> string 0.
                    0x043 => tx.send(NetEvent::ZoneText(ZoneText {
                        packet: 0x043,
                        actor: p.u32(4),
                        index: p.u16(8),
                        mes: p.u16(0xa),
                        kind: p.u8(0xc),
                        strings: vec![text(&p, 0x10, 16)],
                        ..Default::default()
                    }))?,
                    0x067 if p.u8(4) == 2 => tx.send(NetEvent::CharSync { id: p.u32(8), owner: p.u32(0x20), floors: p.u8(0x27) & 1 != 0, open: p.u8(0x24) != 0 })?,
                    0x02E => tx.send(NetEvent::MogMenu)?,
                    0x11A => tx.send(NetEvent::EmoteUnlocks { jobs: p.u32(4), chairs: p.u16(8) })?,
                    0x11E => tx.send(NetEvent::Jump(p.u16(4)))?,
                    0x030 => tx.send(NetEvent::SynthEffect { id: p.u32(4), index: p.u16(8), effect: p.u16(0xa) as i16, kind: p.u8(0xc) as i8, status: p.u8(0xd) as i8, timer: p.u16(0xe) })?,
                    0x06F | 0x070 => {
                        let a = super::synth::Answer {
                            result: p.u8(4),
                            grade: p.u8(5) as i8,
                            count: p.u8(6),
                            item: p.u16(8),
                            breaks: std::array::from_fn(|i| p.u16(0xa + 2 * i)),
                            name: if p.id == 0x070 { text(&p, 0x1e, 16) } else { String::new() },
                            crystal: if p.id == 0x06F { p.u16(0x22) } else { 0 },
                            materials: std::array::from_fn(|i| if p.id == 0x06F { p.u16(0x24 + 2 * i) } else { 0 }),
                        };
                        tx.send(if p.id == 0x06F { NetEvent::SynthResult(a) } else { NetEvent::SynthOthers(a) })?
                    }
                    0x04B => tx.send(NetEvent::Pbx(super::post::Pbx {
                        cmd: p.u8(4),
                        box_no: p.u8(5) as i8,
                        post: p.u8(6) as i8,
                        result: p.u8(0xc) as i8,
                        res: [p.u8(0xd) as i8, p.u8(0xe) as i8, p.u8(0xf) as i8],
                        slot: super::post::Slot { stat: p.u32(0x10), name: text(&p, 0x14, 16), work_no: p.u32(0x2c), item: p.u16(0x30), stack: p.u32(0x38), ext: std::array::from_fn(|i| p.u8(0x3c + i)) },
                    }))?,
                    0x01B => tx.send(NetEvent::JobInfo(super::moogle::JobInfo {
                        flags: p.u32(0xc),
                        levels: std::array::from_fn(|n| if n < 16 { p.u8(0x10 + n) } else { p.u8(0x48 + n) }),
                        sub_ok: p.u8(0x44) != 0,
                    }))?,
                    0x01C => tx.send(NetEvent::ItemMax(std::array::from_fn(|i| p.u8(4 + i)), std::array::from_fn(|i| p.u16(0x24 + 2 * i))))?,
                    0x01E => tx.send(NetEvent::ItemNum { container: p.u8(8), index: p.u8(9), count: p.u32(4) })?,
                    0x01F => tx.send(NetEvent::Item { container: p.u8(0xa), index: p.u8(0xb), id: p.u16(8), count: p.u32(4), ext: None, lock: p.u8(0xc) })?,
                    0x020 => {
                        let ext = std::array::from_fn(|k| p.u8(0x11 + k));
                        tx.send(NetEvent::Item { container: p.u8(0xe), index: p.u8(0xf), id: p.u16(0xc), count: p.u32(4), ext: Some(ext), lock: p.u8(0x10) })?
                    }
                    0x050 => tx.send(NetEvent::Equip { index: p.u8(4), slot: p.u8(5), container: p.u8(6) })?,
                    // CLISTATUS: hpmax @4, mpmax @8, jobs @0xc..0xf, bp_base @0x14, bp_adj @0x22, atk @0x30,
                    // def @0x32, def_elem @0x34, su_lv @0x52, ilvl @0x55.
                    0x061 => tx.send(NetEvent::CharStats(super::items::CharStats {
                        hp_max: p.u32(4),
                        mp_max: p.u32(8),
                        main: (p.u8(0xc), p.u8(0xd)),
                        sub: (p.u8(0xe), p.u8(0xf)),
                        base: std::array::from_fn(|i| p.u16(0x14 + 2 * i) as i32),
                        adj: std::array::from_fn(|i| p.u16(0x22 + 2 * i) as i16 as i32),
                        atk: p.u16(0x30) as i16 as i32,
                        def: p.u16(0x32) as i16 as i32,
                        elem: std::array::from_fn(|i| p.u16(0x34 + 2 * i) as i16 as i32),
                        su: p.u8(0x52),
                        ilvl: p.u8(0x55),
                    }))?,
                    _ => {}
                }
            }
        }
        if !answered && last_login.elapsed() > Duration::from_millis(500) {
            attempt += 1;
            if attempt > 20 {
                anyhow::bail!("the zone server did not answer the login");
            }
            map.send_login(login.char_id, &login.name, account, attempt)?;
            last_login = Instant::now();
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                // c2s 0x0B5 / 0x0B6 (FUN_100e3400 / the tell sender): the text in Shift-JIS, auto-translate blocks
                // kept, 150 bytes at most; 0x0B6 @4 = 3 as retail sends it.
                NetCommand::Chat { kind, text } => {
                    let mut body = vec![kind, 0];
                    body.extend_from_slice(&chat_bytes(&text));
                    body.push(0);
                    queue.push(build(0x0B5, &body));
                }
                NetCommand::Tell { name, text } => {
                    let mut body = vec![0u8; 17];
                    body[0] = 3;
                    body[2..2 + name.len().min(15)].copy_from_slice(&name.as_bytes()[..name.len().min(15)]);
                    body.extend_from_slice(&chat_bytes(&text));
                    body.push(0);
                    queue.push(build(0x0B6, &body));
                }
                NetCommand::Heal(mode) => queue.push(build(0x0E8, &mode.to_le_bytes())),
                NetCommand::Logout { mode, kind } => {
                    let mut b = mode.to_le_bytes().to_vec();
                    b.extend_from_slice(&kind.to_le_bytes());
                    queue.push(build(0x0E7, &b));
                }
                NetCommand::HomePoint => {
                    let mut body = vec![0u8; 24];
                    body[0..4].copy_from_slice(&login.char_id.to_le_bytes());
                    body[4..6].copy_from_slice(&my_index.to_le_bytes());
                    body[6] = 0x0B;
                    queue.push(build(0x01A, &body));
                }
                NetCommand::Act { target, index, action } => queue.push(build(0x01A, &action_body(target, index, action))),
                NetCommand::Packet { id, body } => queue.push(build(id, &body)),
                NetCommand::GroupListReq => queue.push(build(0x076, &[0u8; 4])),
                NetCommand::Check { target, index, kind } => {
                    let mut body = vec![0u8; 12];
                    body[0..4].copy_from_slice(&target.to_le_bytes());
                    body[4..8].copy_from_slice(&(index as u32).to_le_bytes());
                    body[8] = kind;
                    queue.push(build(0x0DD, &body));
                }
                NetCommand::ZoneLine { tag, pos } => {
                    let mut body = Vec::with_capacity(20);
                    body.extend_from_slice(&tag.to_le_bytes());
                    for v in pos {
                        body.extend_from_slice(&v.to_le_bytes());
                    }
                    body.extend_from_slice(&my_index.to_le_bytes());
                    body.extend_from_slice(&[0, 0]);
                    queue.push(build(0x05E, &body));
                }
                NetCommand::EventEnd { target, index, option, zone, event, mode } => queue.push(build(0x05B, &event_end_body(target, index, option, zone, event, mode))),
                NetCommand::EventPos { pos, dir, target, index, option, zone, event } => {
                    let mut b = Vec::with_capacity(28);
                    for v in pos {
                        b.extend_from_slice(&v.to_le_bytes());
                    }
                    b.extend_from_slice(&target.to_le_bytes());
                    b.extend_from_slice(&option.to_le_bytes());
                    b.extend_from_slice(&zone.to_le_bytes());
                    b.extend_from_slice(&event.to_le_bytes());
                    b.extend_from_slice(&index.to_le_bytes());
                    b.extend_from_slice(&[1, dir]);
                    queue.push(build(0x05C, &b));
                }
                NetCommand::Equip { index, slot, container } => queue.push(build(0x050, &[index, slot, container, 0])),
                NetCommand::StatusRequest(kind) => queue.push(build(0x061, &[kind, 0, 0, 0])),
                NetCommand::CharReq(index) => queue.push(build(0x016, &charreq_body(index))),
                NetCommand::MyRoomExit { bit, mode } => {
                    let mut body = super::myroom::ZMRQ.to_le_bytes().to_vec();
                    body.extend_from_slice(&[0; 14]);
                    body.extend_from_slice(&[bit, mode]);
                    queue.push(build(0x05E, &body));
                }
                NetCommand::SubMap { state, id } => {
                    let mut b = state.to_le_bytes().to_vec();
                    b.extend_from_slice(&id.to_le_bytes());
                    queue.push(build(0x0F2, &b));
                }
            }
        }
        // Retail sends one datagram every 400 ms (config +0xd0 = 400, net+0x417c), each with a fresh 0x015.
        if answered && last_send.elapsed() >= Duration::from_millis(400) {
            if zoned {
                let s = *me.lock().unwrap();
                let mut body = [0u8; 28];
                for (i, v) in s.pos.iter().enumerate() {
                    body[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
                body[14..16].copy_from_slice(&(s.moving as u16).to_le_bytes());
                body[16] = s.dir;
                queue.push(build(0x015, &body));
            }
            map.send(&queue)?;
            stats.sent.fetch_add(map.bytes_out - tx_seen, Relaxed);
            tx_seen = map.bytes_out;
            queue.clear();
            last_send = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// s2c 0x009 Data: LSB's /random ("string2 Lnxtest string3 523") and its Para keys.
    #[test]
    fn std_data_keys() {
        assert_eq!(std_data("string2 Lnxtest string3 523"), (0, [0; 4], ["Lnxtest".into(), "523".into()]));
        assert_eq!(std_data("CasUniqueNo 77 Para0 5 Para1 -3"), (77, [5, -3, 0, 0], [String::new(), String::new()]));
        assert_eq!(std_data("CasUniqueNo 1 TarUniqueNo 9"), (9, [0; 4], [String::new(), String::new()]));
    }

    fn sub(id: u16, len: usize, fill: &[(usize, &[u8])]) -> SubPacket {
        let mut data = vec![0u8; len];
        data[0] = id as u8;
        data[1] = ((len / 4) as u8) << 1 | (id >> 8) as u8;
        for (at, b) in fill {
            data[*at..*at + b.len()].copy_from_slice(b);
        }
        SubPacket { id, sync: 0, data }
    }

    /// GP_SERV_COMMAND_EVENT / EVENTNUM / EVENTSTR as LSB lays them out.
    #[test]
    fn event_starts() {
        let p = sub(0x032, 0x14, &[(4, &0x010e_607eu32.to_le_bytes()), (8, &0x7eu16.to_le_bytes()), (0xa, &230u16.to_le_bytes()), (0xc, &615u16.to_le_bytes()), (0xe, &8u16.to_le_bytes()), (0x10, &230u16.to_le_bytes())]);
        let s = event_start(&p);
        assert_eq!((s.target, s.index, s.zone, s.event, s.mode, s.zone2), (0x010e_607e, 0x7e, 230, 615, 8, 230));
        let p = sub(0x034, 0x34, &[(4, &0x010e_601bu32.to_le_bytes()), (8, &1u32.to_le_bytes()), (0x24, &7u32.to_le_bytes()), (0x28, &0x1bu16.to_le_bytes()), (0x2a, &230u16.to_le_bytes()), (0x2c, &0x24au16.to_le_bytes()), (0x2e, &8u16.to_le_bytes()), (0x30, &230u16.to_le_bytes())]);
        let s = event_start(&p);
        assert_eq!((s.index, s.event, s.params[0], s.params[7]), (0x1b, 0x24a, 1, 7));
        let p = sub(0x033, 0x70, &[(0xc, &5u16.to_le_bytes()), (0xe, &8u16.to_le_bytes()), (0x10, b"Bastok"), (0x50, &3u32.to_le_bytes())]);
        let s = event_start(&p);
        assert_eq!((s.event, s.strings[0].as_str(), s.params[0]), (5, "Bastok", 3));
    }

    /// GP_CLI_COMMAND_EVENTEND: UniqueNo, EndPara, ActIndex, Mode, EventNum, EventPara.
    #[test]
    fn event_end_layout() {
        let b = event_end_body(0x010e_607e, 0x7e, 0x4000_0000, 230, 615, 0);
        assert_eq!(b.len(), 16);
        assert_eq!(&b[4..8], &0x4000_0000u32.to_le_bytes());
        assert_eq!(&b[8..10], &0x7eu16.to_le_bytes());
        assert_eq!(&b[12..16], &[230, 0, 0x67, 2]);
    }
}
