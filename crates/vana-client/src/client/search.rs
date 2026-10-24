//! The search server's users: FUN_1007c740's name lookups (all areas, name only; the callbacks take an exact match).
//! The search windows ("menu    scresult" and the rest) come later.

use super::World;
use super::net::NetCommand;
use crate::net::search::{self, Cond, Reply};

/// What a finished query is for.
pub enum Then {
    /// LAB_1007d5b0: /pcmd or /acmd add for someone not in the zone.
    PartyAdd(String),
}

/// FUN_1007c740: the name, first letter upper and the rest lower (FUN_101c8ef0), at most 15 chars, searched in every
/// area by name only.
pub fn lookup(w: &mut World, name: &str, then: impl FnOnce(String) -> Then) {
    let mut chars = name.chars().take(15);
    let name: String = chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars.map(|c| c.to_ascii_lowercase())).collect();
    log::info!("search: name lookup {name}");
    let rx = search::spawn(w.search_addr, Cond::Name(name.clone()));
    w.searches.push((rx, then(name)));
}

/// FUN_100e0d50's delivery for finished queries (a failure or the 30 s timeout delivers nothing).
pub fn poll(w: &mut World) {
    let mut i = 0;
    while i < w.searches.len() {
        match w.searches[i].0.try_recv() {
            Ok(r) => {
                let (_, then) = w.searches.remove(i);
                finish(w, r.ok(), then);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
            Err(_) => {
                w.searches.remove(i);
            }
        }
    }
}

fn finish(w: &mut World, reply: Option<Reply>, then: Then) {
    let Some(reply) = reply else { return };
    match then {
        Then::PartyAdd(name) => {
            // An exact match: c2s 0x06E with its id, ActIndex 0, kind 5 to a party leader (rec+0x30 bit 3), else 0.
            let Some(r) = reply.records.iter().find(|r| r.name == name) else {
                log::info!("search: {name} not found ({} of {})", reply.records.len(), reply.total);
                return;
            };
            let mut b = vec![0u8; 8];
            b[0..4].copy_from_slice(&r.id.to_le_bytes());
            b[6] = if r.flags & 8 != 0 { 5 } else { 0 };
            log::info!("search: {name} is {:#x} in zone {}, inviting (kind {})", r.id, r.zone, b[6]);
            let _ = w.net.commands.send(NetCommand::Packet { id: 0x06E, body: b });
        }
    }
}
