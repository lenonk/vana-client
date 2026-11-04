//! Our own character's costume state (FUN_10095e90, actor +0x120 bits 4 / 5): the id from s2c 0x037 @0x44 marks a
//! change pending; once a frame the pending costume goes on (or off, for id 0) unless the actor is bound to the
//! running event. Some statuses take a costume that is on back off, which leaves it pending again. Only the "in
//! costume" bit is kept here: FUN_101cc0d0's mask 0x40 refuses trades and commands with it (group 7 line 0xb1). The
//! costume model itself (an NPC model id, drawn as the whole body) is not built.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Costume {
    /// +0x166: the NPC model id, 0 none.
    pub id: u16,
    /// +0x120 bit 4: changed, not applied yet.
    pub pending: bool,
    /// +0x120 bit 5: in costume.
    pub on: bool,
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
                (self.pending, self.on) = (false, false);
            } else if allowed {
                (self.pending, self.on) = (false, true);
            }
        } else if self.on && !allowed {
            // Off and pending again (inf: so in one of those statuses it goes back on the next frame).
            (self.pending, self.on) = (true, false);
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
        c.set_id(0);
        c.step(0, true);
        assert_eq!(c, Costume::default(), "id 0 takes it off even while bound");
    }
}
