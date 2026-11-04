//! Characters' costumes (FUN_10095e90, actor +0x120 bits 4 / 5): the id from s2c 0x037 @0x44 (ours) or 0x00D @0x30
//! (others) marks a change pending; once a frame the pending costume goes on (or off, for id 0) unless the actor is
//! bound to the running event. Some statuses take a costume that is on back off, which leaves it pending again. In
//! costume FUN_101cc0d0's mask 0x40 refuses our trades and commands (group 7 line 0xb1), and the actor is drawn as the
//! costume's NPC model (FUN_100d2f60 case 2: FUN_100c51c0(0, +0x166), the whole body); the look it would have waits.

use super::{Source, World};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Costume {
    /// +0x166: the NPC model id, 0 none.
    pub id: u16,
    /// +0x120 bit 4: changed, not applied yet.
    pub pending: bool,
    /// +0x120 bit 5: in costume.
    pub on: bool,
    /// The model drawn now (0 none): a new id stays pending, and the old model on, while the step refuses it.
    pub shown: u16,
}

impl Costume {
    /// FUN_1008cce0: a different id is stored and marked pending.
    pub fn set_id(&mut self, id: u16) {
        if self.id != id {
            self.id = id;
            self.pending = true;
        }
    }

    /// FUN_10095e90, once a frame: `status` is +0x170 (the event status while bound), `bound` +0x120 bit 7.
    pub fn step(&mut self, status: u8, bound: bool) {
        // Table 0x10095fe0 (status - 2), then FUN_10095680 (5), FUN_100956a0 (0x55), FUN_100956c0 (fishing, digging)
        // and FUN_10095790 (chairs): only checked while the costume is on.
        let blocked = self.on && matches!(status, 2 | 3 | 5 | 6 | 7 | 0x21 | 0x26..=0x2c | 0x2f | 0x30 | 0x32..=0x35 | 0x38..=0x53 | 0x55);
        let allowed = !blocked && !bound;
        if self.pending {
            if self.id == 0 {
                (self.pending, self.on, self.shown) = (false, false, 0);
            } else if allowed {
                (self.pending, self.on, self.shown) = (false, true, self.id);
            }
        } else if self.on && !allowed {
            // Off and pending again (inf: so in one of those statuses it goes back on the next frame).
            (self.pending, self.on, self.shown) = (true, false, 0);
        }
    }
}

/// The costume model an actor wears now, if any.
pub(super) fn worn(w: &World, id: u32) -> Option<u16> {
    let c = if id == w.my_id { Some(&w.costume) } else { w.costumes.get(&id) };
    c.filter(|c| c.on && c.shown != 0).map(|c| c.shown)
}

/// `source` is the costume's own model.
pub(super) fn is_model(source: &Source, id: u16) -> bool {
    matches!(source, Source::Npc(crate::dat::npc::NpcLook::Model(m)) if *m == id)
}

/// FUN_10095e90 on every actor with a costume, ours from our status, the others' from theirs. A costume going on keeps
/// the actor's look aside and loads the NPC model; coming off brings the look back.
pub(super) fn step_all(w: &mut World) {
    let bound = |w: &World, id: u32| w.event.as_ref().is_some_and(|l| l.st.bound.contains(&id));
    let mut changes = Vec::new();
    let me = w.my_id;
    // The applied model: a new id while on (FUN_1008cce0 marks it pending) reloads it once the step takes it.
    let applied = |c: &Costume| (c.on && c.shown != 0).then_some(c.shown);
    let was = applied(&w.costume);
    let b = bound(w, me);
    w.costume.step(w.my_motion.status, b);
    if applied(&w.costume) != was {
        changes.push((me, applied(&w.costume)));
    }
    w.costumes.retain(|id, _| w.entities.contains_key(id));
    let ids: Vec<(u32, u8, bool)> = w.costumes.keys().map(|&id| (id, w.entities.get(&id).map_or(0, |e| e.motion.status), bound(w, id))).collect();
    for (id, status, b) in ids {
        let Some(c) = w.costumes.get_mut(&id) else { continue };
        let was = applied(c);
        c.step(status, b);
        if applied(c) != was {
            changes.push((id, applied(c)));
        }
    }
    for (id, on) in changes {
        match on {
            Some(model) => {
                // The look underneath is kept once; a costume swapped for another keeps the same one.
                if !w.costume_base.contains_key(&id)
                    && let Some(s) = w.actors.get(&id).and_then(|a| a.source).filter(|s| !is_model(s, model))
                {
                    w.costume_base.insert(id, s);
                }
                w.event_looks.push((id, Source::Npc(crate::dat::npc::NpcLook::Model(model))));
            }
            None => {
                // With the held looks, ahead of this frame's server looks: a newer look from the server wins.
                if let Some(s) = w.costume_base.remove(&id) {
                    w.held_looks.push((id, s));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costume_states() {
        let mut c = Costume::default();
        c.set_id(0x123);
        c.step(0, true);
        assert!(c.pending && !c.on, "bound to the event: stays pending");
        c.step(0x2f, false);
        assert!(!c.pending && c.on, "the status lists never stop it going on");
        c.step(0x2f, false);
        assert!(c.pending && !c.on, "sitting takes it off");
        c.step(0, false);
        assert!(c.on);
        c.set_id(0x124);
        c.step(0, true);
        assert!(c.on && c.pending && c.shown == 0x123, "bound: the new id waits, the old model stays");
        c.step(0, false);
        assert!(c.on && c.shown == 0x124 && !c.pending, "a new id while on swaps the model");
        c.set_id(0);
        c.step(0, true);
        assert_eq!(c, Costume::default(), "id 0 takes it off even while bound");
    }
}
