//! The Friend List's data (research friend_list_spec.md 2, 4.2): what polcore and FFXiMain's friend manager held, filled
//! from LSB's xi_profile (net/profile.rs): our open status, handle and characters, the friend and block lists, the
//! friends' presence from the IRC notices.

use super::World;
use crate::net::profile::{self, Cred, FriendInfo, Irc, IrcLink};
use crate::net::search::Record;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// One friend or block-list entry (sqPolFriend and FFXiMain's part of it, 2.2).
#[derive(Clone, Debug, Default)]
pub struct Friend {
    pub black: bool,
    /// The list index (0..199 friends, 0..99 block list) and display order.
    pub index: u8,
    pub order: u8,
    pub account: u64,
    /// What we call them (the handle name of the entry).
    pub name: String,
    pub pending: bool,
    /// From the notices: 0 offline, 1 online, 2 away; the character they play; the newest notice's age.
    pub status: u8,
    pub char_id: Option<u32>,
    pub age: u64,
    /// The playing character's search record (+0xb0) and whether it is current (+0xfc bit 0).
    pub record: Option<Record>,
    pub fresh: bool,
    /// An unread message from them (+0xfc bit 1).
    pub mail: bool,
}

impl Friend {
    fn from_info(f: &FriendInfo) -> Self {
        Self { black: f.black, index: f.num, order: f.order, account: f.pol, name: f.name.clone(), pending: f.pending, ..Default::default() }
    }
}

struct Loaded {
    status: u8,
    list: Vec<FriendInfo>,
    handle: (u64, String),
    chars: Vec<(u8, u32, String)>,
}

enum Done {
    Loaded(anyhow::Result<Loaded>),
    /// ChangeMyStatus answered: the open status it set (None: only the in-game flag).
    Status(anyhow::Result<()>, Option<u8>),
    /// Anything else: applied on the main thread.
    Then(Box<dyn FnOnce(&mut World) + Send>),
}

/// A message in the local folders (retail's 7 received unread / 8 read, 2.4): downloaded, then deleted on the server.
#[derive(Clone, Debug)]
pub struct Mail {
    pub name: String,
    pub header: profile::MsgHeader,
    pub data: Vec<u8>,
    pub unread: bool,
}

#[derive(Default)]
pub struct Friends {
    cred: Option<Cred>,
    irc: Option<IrcLink>,
    jobs: Vec<mpsc::Receiver<Done>>,
    pub list: Vec<Friend>,
    /// Our open status (DAT_104df90c): 0 Online, 1 Away, 4 Invisible.
    pub my_status: u8,
    /// Our handle (LoadHandleNameList: LSB's account's first character) and the account's characters.
    pub handle: Option<(u64, String)>,
    pub chars: Vec<(u8, u32, String)>,
    pub signed_on: bool,
    pub loaded: bool,
    started: bool,
    /// The in-game ChangeMyStatus went (after the map session exists, so friends see the character).
    announced: bool,
    retry_at: Option<Instant>,
    /// The friends' characters asked of xi_search (2.3): the queries out, the chunks left of a round, the next round.
    lookups: Vec<(mpsc::Receiver<anyhow::Result<crate::net::search::Reply>>, Vec<u32>)>,
    round: Vec<Vec<u32>>,
    next_round: Option<Instant>,
    /// The categories the list hides (+0xa4..+0xa8, kept for the session).
    pub hidden: [bool; 5],
    /// One PlayOnline message operation at a time (DAT_10663ac8).
    pub busy: bool,
    /// The local messages, oldest first; messages that came since the count was last seen (netstat).
    pub mail: Vec<Mail>,
    pub mail_new: u32,
    fetching: bool,
}

impl Friends {
    pub fn new(cred: Option<Cred>) -> Self {
        Self { cred, ..Default::default() }
    }

    /// A friend by account, on the friend list.
    pub fn by_account(&self, account: u64) -> Option<&Friend> {
        self.list.iter().find(|f| !f.black && f.account == account)
    }
}

fn spawn(w: &mut World, job: impl FnOnce(&Cred) -> Done + Send + 'static) {
    let Some(c) = w.friends.cred.clone() else { return };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job(&c));
    });
    w.friends.jobs.push(rx);
}

/// In game: the IRC sign-on and the loads (as xiloader's login: our status first, then the lists).
pub fn start(w: &mut World) {
    let f = &mut w.friends;
    if f.started || f.cred.is_none() {
        return;
    }
    f.started = true;
    f.irc = f.cred.clone().map(profile::irc);
    spawn(w, |c| {
        Done::Loaded((|| {
            let status = profile::load_my_status(c)?;
            let list = profile::load_friend_list(c)?;
            let handle = profile::load_handle(c)?;
            let chars = profile::load_characters(c)?;
            Ok(Loaded { status, list, handle, chars })
        })())
    });
}

/// Our character's slot in LoadCharacterList's order.
fn slot(w: &World) -> Option<u8> {
    w.friends.chars.iter().find(|c| c.1 == w.my_id).map(|c| c.0)
}

/// FUN_100f7a90 (/online 0, /away 1, /hide 4, olstat): ChangeMyStatus when it differs; it is ours once answered.
pub fn set_status(w: &mut World, open: u8) {
    if open == w.friends.my_status || !w.friends.signed_on {
        return;
    }
    let s = slot(w);
    spawn(w, move |c| Done::Status(profile::change_my_status(c, s, Some(open)), Some(open)));
}

/// Each frame: the jobs' answers and the IRC session's notices.
pub fn poll(w: &mut World) {
    let mut done = Vec::new();
    w.friends.jobs.retain(|rx| match rx.try_recv() {
        Ok(d) => {
            done.push(d);
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(mpsc::TryRecvError::Disconnected) => false,
    });
    for d in done {
        match d {
            Done::Loaded(Ok(l)) => {
                let f = &mut w.friends;
                log::info!("friends: status {}, {} entries, handle {:?}, {} characters", l.status, l.list.len(), l.handle, l.chars.len());
                f.my_status = l.status;
                f.list = l.list.iter().map(Friend::from_info).collect();
                (f.handle, f.chars, f.loaded) = (Some(l.handle), l.chars, true);
            }
            Done::Loaded(Err(e)) => log::warn!("friends: loading: {e:#}"),
            Done::Status(Ok(()), open) => {
                if let Some(o) = open {
                    w.friends.my_status = o;
                }
                log::info!("friends: ChangeMyStatus {open:?} applied");
            }
            Done::Status(Err(e), _) => log::warn!("friends: ChangeMyStatus: {e:#}"),
            Done::Then(f) => f(w),
        }
    }
    let events: Vec<Irc> = w.friends.irc.as_ref().map(|l| l.events.try_iter().collect()).unwrap_or_default();
    for e in events {
        match e {
            Irc::SignedOn => {
                log::info!("friends: signed on to xi_profile's IRC");
                w.friends.signed_on = true;
                load_mail(w);
                fetch(w);
            }
            Irc::Status { account, list_index, status, char_id, age } => notice(w, account, list_index, status, char_id, age),
            Irc::Mail(_) => fetch(w),
            Irc::Closed(why) => {
                log::warn!("friends: IRC closed: {why}");
                let f = &mut w.friends;
                (f.irc, f.signed_on, f.announced) = (None, false, false);
                f.retry_at = Some(Instant::now() + Duration::from_secs(30));
            }
        }
    }
    // A dropped session comes back and loads again.
    if w.friends.retry_at.is_some_and(|t| Instant::now() >= t) {
        (w.friends.retry_at, w.friends.started) = (None, false);
        start(w);
    }
    refresh(w);
    // This character is now the active one, once signed on and in a zone.
    let f = &w.friends;
    if f.signed_on && f.loaded && !f.announced && w.zone.is_some() {
        w.friends.announced = true;
        let s = slot(w);
        spawn(w, move |c| Done::Status(profile::change_my_status(c, s, None), None));
    }
}

/// A friend's status notice (3.4.3): applied to the entry at its list index when newer than the last.
fn notice(w: &mut World, account: u64, index: u8, status: u8, char_id: Option<u32>, age: u64) {
    let Some(f) = w.friends.list.iter_mut().find(|f| !f.black && f.index == index && f.account == account) else {
        log::info!("friends: status of account {account} (index {index}) for no entry");
        return;
    };
    if age < f.age {
        return;
    }
    if f.char_id != char_id {
        (f.record, f.fresh) = (None, false);
    }
    (f.status, f.char_id, f.age) = (status, char_id, age);
    log::info!("friends: {} is {status} ({char_id:?})", f.name);
}

/// FUN_100e63e0 / FUN_100e6650: the playing friends' characters from xi_search, 20 ids a query (wire type 1); a round
/// at login runs its chunks back to back, then one chunk every 60 s. A record sets the friend's (fresh); an id without
/// one loses it.
fn refresh(w: &mut World) {
    let mut answers = Vec::new();
    w.friends.lookups.retain(|(rx, ids)| match rx.try_recv() {
        Ok(r) => {
            answers.push((r, ids.clone()));
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(mpsc::TryRecvError::Disconnected) => false,
    });
    for (r, ids) in answers {
        let records = r.map(|r| r.records).unwrap_or_default();
        for f in w.friends.list.iter_mut().filter(|f| f.char_id.is_some_and(|c| ids.contains(&c))) {
            f.record = records.iter().find(|r| Some(r.id) == f.char_id).cloned();
            f.fresh = f.record.is_some();
        }
    }
    if !w.friends.loaded || w.zone.is_none() || !w.friends.lookups.is_empty() {
        return;
    }
    let f = &mut w.friends;
    let now = Instant::now();
    if f.round.is_empty() && f.next_round.is_none_or(|t| now >= t) {
        let ids: Vec<u32> = f.list.iter().filter(|f| !f.black && !f.pending && f.status != 0).filter_map(|f| f.char_id).collect();
        f.round = ids.chunks(20).map(<[u32]>::to_vec).collect();
        f.next_round = Some(now + Duration::from_secs(60));
    }
    if let Some(ids) = f.round.pop() {
        let rx = crate::net::search::spawn(w.search_addr, crate::net::search::Cond::ids(ids.clone()));
        w.friends.lookups.push((rx, ids));
    }
}

fn now_unix() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32)
}

/// The local folders: <data>/mail/<account>/unread and read.
fn mail_dir(w: &World, unread: bool) -> Option<std::path::PathBuf> {
    let c = w.friends.cred.as_ref()?;
    Some(crate::config::data_dir().join("mail").join(c.account.to_string()).join(if unread { "unread" } else { "read" }))
}

/// The local folders read at sign-on (FUN_100f1f00: the counts), every friend's unread bit set from them.
fn load_mail(w: &mut World) {
    let mut mail = Vec::new();
    for unread in [true, false] {
        let Some(dir) = mail_dir(w, unread) else { return };
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let (Some(header), Ok(data)) = (profile::MsgHeader::from_name(&name), std::fs::read(e.path())) {
                mail.push(Mail { name, header, data, unread });
            }
        }
    }
    mail.sort_by_key(|m| (m.header.date, m.header.seq));
    w.friends.mail = mail;
    mark_mail(w);
}

/// +0xfc bit 1: an unread message from that friend.
fn mark_mail(w: &mut World) {
    let senders: Vec<u64> = w.friends.mail.iter().filter(|m| m.unread).map(|m| m.header.from).collect();
    for f in &mut w.friends.list {
        f.mail = senders.contains(&f.account);
    }
}

/// The mail engine (2.4): the server's mailbox listed, each message downloaded into the unread folder and deleted on the
/// server; new ones counted (netstat's SE 0039).
pub fn fetch(w: &mut World) {
    if w.friends.fetching {
        return;
    }
    w.friends.fetching = true;
    spawn(w, |c| {
        let got: Vec<(String, Vec<u8>)> = profile::mail_list(c)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_, _, name)| {
                let path = format!("{}{name}", profile::MAILBOX);
                let data = profile::read_file(c, &path).ok()?;
                let _ = profile::remove_file(c, &path);
                Some((name, data))
            })
            .collect();
        Done::Then(Box::new(move |w: &mut World| {
            w.friends.fetching = false;
            let dir = mail_dir(w, true);
            for (name, data) in got {
                let Some(header) = profile::MsgHeader::from_name(&name) else { continue };
                if let Some(d) = &dir {
                    let _ = std::fs::create_dir_all(d);
                    let _ = std::fs::write(d.join(&name), &data);
                }
                log::info!("friends: message type {} from {} ({})", header.kind, header.sender, header.from);
                w.friends.mail.push(Mail { name, header, data, unread: true });
                w.friends.mail_new += 1;
            }
            // Oldest first by Date (FUN_102006b0).
            w.friends.mail.sort_by_key(|m| (m.header.date, m.header.seq));
            mark_mail(w);
        }))
    });
}

/// Mark a message read (FUN_100f7590): the file moves to the read folder.
pub fn mark_read(w: &mut World, name: &str) {
    if let (Some(from), Some(to)) = (mail_dir(w, true), mail_dir(w, false)) {
        let _ = std::fs::create_dir_all(&to);
        let _ = std::fs::rename(from.join(name), to.join(name));
    }
    if let Some(m) = w.friends.mail.iter_mut().find(|m| m.name == name) {
        m.unread = false;
    }
    mark_mail(w);
}

/// FUN_100e68a0: -3 ourselves, -2 already in that list, -1 full, 0 room.
pub fn room(w: &World, black: bool, account: u64) -> i8 {
    let f = &w.friends;
    match () {
        _ if f.cred.as_ref().is_some_and(|c| c.account as u64 == account) => -3,
        _ if f.list.iter().any(|e| e.black == black && e.account == account) => -2,
        _ if f.list.iter().filter(|e| e.black == black).count() >= if black { 100 } else { 200 } => -1,
        _ => 0,
    }
}

/// StoreFriendList with every index's order and `entry`; the list loaded again after.
fn store(c: &Cred, list: &[Friend], entry: FriendInfo) -> anyhow::Result<Vec<FriendInfo>> {
    let mut fo = [0u8; 200];
    let mut bo = [0u8; 100];
    for f in list {
        if f.black {
            bo[f.index as usize % 100] = f.order;
        } else {
            fo[f.index as usize % 200] = f.order;
        }
    }
    let i = entry.num as usize;
    if entry.black { bo[i % 100] = entry.order } else { fo[i % 200] = entry.order }
    let ans = profile::store_friend_list(c, &fo, &bo, &[entry])?;
    if ans.first().is_some_and(|a| a.result != 0) {
        anyhow::bail!("the store was refused");
    }
    profile::load_friend_list(c)
}

/// A set entry at the first free index of a list.
fn new_entry(w: &World, black: bool, account: u64, name: &str) -> FriendInfo {
    let used: Vec<u8> = w.friends.list.iter().filter(|f| f.black == black).map(|f| f.index).collect();
    let num = (0..if black { 100 } else { 200 }).find(|n| !used.contains(n)).unwrap_or(0);
    FriendInfo { op: 1, black, handle_id: account, num, order: num, pol: account, name: name.chars().take(15).collect(), ..Default::default() }
}

/// The store's answer applied: the entries as loaded, their presence kept.
fn apply_list(w: &mut World, list: Vec<FriendInfo>) {
    let old = std::mem::take(&mut w.friends.list);
    w.friends.list = list
        .iter()
        .map(|i| {
            let mut f = Friend::from_info(i);
            if let Some(o) = old.iter().find(|o| o.black == f.black && o.account == f.account) {
                (f.status, f.char_id, f.age, f.record, f.fresh) = (o.status, o.char_id, o.age, o.record.clone(), o.fresh);
            }
            f
        })
        .collect();
    mark_mail(w);
}

/// What a message operation is (msgline's modes, 2.6).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// 0 / 1: a plain message.
    Plain { to: u64 },
    /// 2: a friend request: the account, our list name for them, their character's name (the text).
    Request { to: u64, list_name: String, char_name: String },
    /// 5: a reply.
    Reply { to: u64 },
    /// 7: an accept, listing them under `list_name`.
    Accept { to: u64, list_name: String },
    /// 8: a decline.
    Decline { to: u64 },
}

/// polcore's built-in titles (xiloader's English) and FFXiMain's own (group 9).
fn title(w: &World, kind: u8, reply: bool) -> String {
    match kind {
        profile::FRIEND_REQUEST => "Let's be friends!".into(),
        profile::FRIEND_ACCEPTED => "Friend registration accepted".into(),
        profile::FRIEND_DECLINED => "Friend registration declined".into(),
        _ => w.battle.as_ref().and_then(|b| b.group9(if reply { 0x1f } else { 0x1c })).unwrap_or_default(),
    }
}

/// msgline's commit (FUN_101ff3e0): the job for the mode, busy until it answers; the success line (group 9) or the
/// failure (10 0x2d, reply / accept 10 0x6f).
pub fn send(w: &mut World, op: Op, text: Vec<u8>) {
    let Some(me) = w.friends.cred.as_ref().map(|c| c.account as u64) else { return };
    w.friends.busy = true;
    let (to, kind) = match &op {
        Op::Plain { to } | Op::Reply { to } => (*to, profile::PLAIN),
        Op::Request { to, .. } => (*to, profile::FRIEND_REQUEST),
        Op::Accept { to, .. } => (*to, profile::FRIEND_ACCEPTED),
        Op::Decline { to } => (*to, profile::FRIEND_DECLINED),
    };
    let t = title(w, kind, matches!(op, Op::Reply { .. }));
    let body = match &op {
        Op::Request { char_name, .. } => profile::encode_body(t.as_bytes(), char_name.as_bytes()),
        _ => profile::encode_body(t.as_bytes(), &text),
    };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let header = profile::MsgHeader { from: me, to, sender: w.name.clone(), title: t.chars().take(15).collect(), seq: nanos, date: now_unix(), length: body.len() as u32, kind, no_reply: false };
    let entry = match &op {
        Op::Request { list_name, .. } | Op::Accept { list_name, .. } => Some(new_entry(w, false, to, list_name)),
        _ => None,
    };
    let list = w.friends.list.clone();
    let (ok_line, fail_line) = match op {
        Op::Plain { .. } => (0x11, 0x2d),
        Op::Request { .. } => (0x14, 0x2d),
        Op::Reply { .. } => (0x12, 0x6f),
        Op::Accept { .. } => (0x15, 0x6f),
        Op::Decline { .. } => (0x16, 0x2d),
    };
    spawn(w, move |c| {
        let r = (|| {
            let stored = match entry {
                Some(e) => Some(store(c, &list, e)?),
                None => None,
            };
            profile::send_message(c, &header, &body)?;
            Ok::<_, anyhow::Error>(stored)
        })();
        Done::Then(Box::new(move |w: &mut World| {
            w.friends.busy = false;
            match r {
                Ok(stored) => {
                    if let Some(l) = stored {
                        apply_list(w, l);
                    }
                    super::textcmd::log9(w, ok_line);
                }
                Err(e) => {
                    log::warn!("friends: send: {e:#}");
                    line10(w, fail_line, 1);
                }
            }
        }))
    });
}

/// A group 10 line with its %d.
fn line10(w: &mut World, id: usize, code: i32) {
    if let Some(bt) = w.battle.clone() {
        let s = super::select::printf(&bt.group10(id).unwrap_or_default(), &[&code.to_string()]);
        let colour = bt.colour_in(0x79, &w.game);
        w.chat.push_colour(s, colour);
    }
}

/// Ignore (FUN_10200280): a friend's entry copied to the block list, else a new one; 9 0x18, then `then`.
pub fn ignore(w: &mut World, account: u64, name: &str, then: impl FnOnce(&mut World) + Send + 'static) {
    let e = new_entry(w, true, account, name);
    let list = w.friends.list.clone();
    w.friends.busy = true;
    spawn(w, move |c| {
        let r = store(c, &list, e);
        Done::Then(Box::new(move |w: &mut World| {
            w.friends.busy = false;
            match r {
                Ok(l) => {
                    apply_list(w, l);
                    super::textcmd::log9(w, 0x18);
                }
                Err(e) => log::warn!("friends: ignore: {e:#}"),
            }
            then(w);
        }))
    });
}

/// FUN_101ffa20 (/befriend, the search results' Befriend): SearchPolId for the character, then our lists: not listed ->
/// the composer for a request (its name box filled with theirs); listed -> 7 0xa1 from /befriend, a plain message from
/// Befriend; ourselves or not found -> 7 0xa2 *(inf)*. Busy: 7 0x7a.
pub fn befriend(w: &mut World, char_id: u32, char_name: String, from_command: bool) {
    if w.friends.busy {
        super::textcmd::refuse(w, 0x7a);
        return;
    }
    w.friends.busy = true;
    spawn(w, move |c| {
        let r = profile::search_pol_id(c, char_id);
        Done::Then(Box::new(move |w: &mut World| {
            w.friends.busy = false;
            let pol = match r {
                Ok(Some((pol, _))) => pol,
                other => {
                    log::info!("friends: SearchPolId {char_id}: {other:?}");
                    super::textcmd::refuse(w, 0xa2);
                    return;
                }
            };
            match room(w, false, pol) {
                0 => {
                    super::polmail::compose(w, super::polmail::Mode::Request { to: pol, char_name: char_name.clone() }, char_name);
                }
                -2 if !from_command => {
                    super::polmail::compose(w, super::polmail::Mode::Plain { to: pol }, String::new());
                }
                -2 => super::textcmd::refuse(w, 0xa1),
                _ => super::textcmd::refuse(w, 0xa2),
            }
        }))
    });
}
