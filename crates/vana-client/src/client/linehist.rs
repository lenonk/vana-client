//! The chat line's history (FSTEXTINPUT DAT_10662d40 +0x7ed8; spec ~/ffxi/lab/research/input_history_spec.md): slot
//! 0 is the line being typed, then the 9 last sent lines newest first. Up older, Down newer, no wrap, no sound.

/// Sent lines kept (+0x614 at most 9).
const KEEP: usize = 9;

#[derive(Default)]
pub struct LineHistory {
    /// Sent lines, newest first.
    sent: Vec<String>,
    /// Slot 0: the line being typed, saved by the first Up.
    working: String,
    /// +0x618: 0 = the working line, n = sent[n - 1].
    pos: usize,
    /// The shown line was edited since it was recalled: the next Up / Down writes it back.
    dirty: bool,
    /// The chat line was open last frame (an open resets the position).
    open: bool,
}

impl LineHistory {
    /// Every open of the line and every send go back to the working line.
    pub fn reset(&mut self) {
        (self.pos, self.dirty) = (0, false);
    }

    /// Called once a frame with the chat line's state: an open resets the position.
    pub fn track(&mut self, typing: bool) {
        if typing && !self.open {
            self.reset();
        }
        self.open = typing;
    }

    /// FUN_101ac580: a non-empty sent line goes on top (no duplicate check); the oldest drops off.
    pub fn record(&mut self, line: &str) {
        if !line.is_empty() {
            self.sent.insert(0, line.to_string());
            self.sent.truncate(KEEP);
        }
        self.reset();
    }

    pub fn edited(&mut self) {
        self.dirty = true;
    }

    /// FUN_101ac2c0: the next older line, or None past the oldest.
    pub fn up(&mut self, current: &str) -> Option<String> {
        if self.pos >= self.sent.len() {
            return None;
        }
        self.store(current);
        self.pos += 1;
        Some(self.sent[self.pos - 1].clone())
    }

    /// FUN_101ac3d0: the next newer line, the working line last; None on the working line.
    pub fn down(&mut self, current: &str) -> Option<String> {
        if self.pos == 0 {
            return None;
        }
        self.store(current);
        self.pos -= 1;
        Some(if self.pos == 0 { self.working.clone() } else { self.sent[self.pos - 1].clone() })
    }

    /// The working line is always kept; a recalled line only when it was edited.
    fn store(&mut self, current: &str) {
        if self.pos == 0 {
            self.working = current.to_string();
        } else if self.dirty {
            self.sent[self.pos - 1] = current.to_string();
        }
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_down() {
        let mut h = LineHistory::default();
        for l in ["/sea", "", "hello", "/wave"] {
            h.record(l);
        }
        assert_eq!(h.up("half"), Some("/wave".into()));
        assert_eq!(h.up("/wave"), Some("hello".into()));
        assert_eq!(h.up("hello"), Some("/sea".into()));
        assert_eq!(h.up("/sea"), None);
        // An edit is written back by the next move.
        h.edited();
        assert_eq!(h.down("/sea all"), Some("hello".into()));
        assert_eq!(h.down("hello"), Some("/wave".into()));
        assert_eq!(h.down("/wave"), Some("half".into()));
        assert_eq!(h.down("half"), None);
        assert_eq!(h.sent, ["/wave", "hello", "/sea all"]);
        for k in 0..12 {
            h.record(&k.to_string());
        }
        assert_eq!(h.sent.len(), 9);
        assert_eq!(h.sent[0], "11");
    }
}
