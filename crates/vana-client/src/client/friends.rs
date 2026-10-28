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
        }
    }
    let events: Vec<Irc> = w.friends.irc.as_ref().map(|l| l.events.try_iter().collect()).unwrap_or_default();
    for e in events {
        match e {
            Irc::SignedOn => {
                log::info!("friends: signed on to xi_profile's IRC");
                w.friends.signed_on = true;
            }
            Irc::Status { account, list_index, status, char_id, age } => notice(w, account, list_index, status, char_id, age),
            Irc::Mail(h) => log::info!("friends: new message notice ({} bytes)", h.len()),
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
