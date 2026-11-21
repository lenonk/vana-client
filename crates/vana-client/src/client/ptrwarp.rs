//! The pointer put on a window's cursor child (FUN_10126000 on the mouse object DAT_104e2f04): the game's pointer
//! (+4 / +8) jumps there, the drawn sprite (+0xc / +0x10) glides after it, and for 18 ticks (+0x48) no child takes the
//! pointer. The next mouse motion puts the pointer back where the mouse really is.

/// The game pointer while it is not where the mouse is (UI pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PtrWarp {
    /// +4 / +8: where the pointer was put.
    pub at: (f32, f32),
    /// +0xc / +0x10: where the sprite is drawn.
    pub drawn: (f32, f32),
    /// +0x48: ticks left of the glide; while above 0 the children's hit test (FUN_1011a140) finds nothing.
    pub glide: f32,
    /// The stacked windows' pointer shift (World::ptr_dy) when it was put: a later change moves it with them, as
    /// FUN_101626e0 moves +4 / +8.
    pub dy: f32,
    /// Retail frames (30 a second) not yet stepped.
    acc: f32,
}

impl PtrWarp {
    /// FUN_10126000: +0x48 = 18.0, then FUN_10125b90 puts +4 / +8 on `at`; the sprite starts from `drawn`.
    pub fn new(at: (f32, f32), drawn: (f32, f32), dy: f32) -> Self {
        Self { at, drawn, glide: 18.0, dy, acc: 0.0 }
    }

    /// This warp with the stacked windows' shift now `dy`: the pointer and the sprite moved by its change.
    pub fn shifted(&self, dy: f32) -> Self {
        let d = dy - self.dy;
        Self { at: (self.at.0, self.at.1 + d), drawn: (self.drawn.0, self.drawn.1 + d), ..*self }
    }

    /// FUN_101267f0's head, once a retail frame (2 ticks of FUN_10014cf0): with no glide left the sprite is at the
    /// pointer; else the glide counts down (ending at 0 without a move) or the sprite goes half way, C division on
    /// ints.
    pub fn step(&mut self, dt: f32) {
        self.acc += dt * 30.0;
        while self.acc >= 1.0 {
            self.acc -= 1.0;
            if self.glide <= 0.0 {
                self.drawn = self.at;
                continue;
            }
            self.glide -= 2.0;
            if self.glide <= 0.0 {
                self.glide = 0.0;
            } else {
                let half = |a: f32, d: f32| (d as i32 + (a as i32 - d as i32) / 2) as f32;
                self.drawn = (half(self.at.0, self.drawn.0), half(self.at.1, self.drawn.1));
            }
        }
    }

    /// Where the windows see the pointer: nowhere during the glide (FUN_1011a140 returns -1 while +0x48 > 0), then
    /// at the pointer.
    pub fn pointer(&self) -> Option<(f32, f32)> {
        (self.glide <= 0.0).then_some(self.at)
    }

    /// WM_MOUSEMOVE (FUN_100016c0): a mouse at `p` (UI pixels) more than 2 pixels from the pointer on either axis
    /// moves it there (FUN_10125ad0, which also ends the glide).
    pub fn left_by(&self, p: (f32, f32)) -> bool {
        (p.0 - self.at.0).abs() > 2.0 || (p.1 - self.at.1).abs() > 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glides_half_way_for_eighteen_ticks_then_sits_on_the_pointer() {
        let mut w = PtrWarp::new((100.0, 50.0), (0.0, 0.0), 0.0);
        assert_eq!(w.pointer(), None);
        w.step(0.034);
        assert_eq!((w.drawn, w.glide), ((50.0, 25.0), 16.0));
        w.step(0.034);
        assert_eq!(w.drawn, (75.0, 37.0));
        // Six more frames to 2 ticks, the ninth ends the glide where it is, the tenth puts the sprite on the point.
        for _ in 0..6 {
            w.step(0.034);
        }
        assert_eq!(w.glide, 2.0);
        let before = w.drawn;
        w.step(0.034);
        assert_eq!((w.drawn, w.glide), (before, 0.0));
        assert_eq!(w.pointer(), Some((100.0, 50.0)));
        w.step(0.034);
        assert_eq!(w.drawn, (100.0, 50.0));
    }

    #[test]
    fn steps_at_thirty_a_second_whatever_the_frame_rate() {
        let mut w = PtrWarp::new((64.0, 0.0), (0.0, 0.0), 0.0);
        w.step(0.017);
        assert_eq!((w.drawn, w.glide), ((0.0, 0.0), 18.0));
        w.step(0.017);
        assert_eq!((w.drawn, w.glide), ((32.0, 0.0), 16.0));
    }

    #[test]
    fn negative_distances_round_toward_zero() {
        let mut w = PtrWarp::new((0.0, 0.0), (5.0, -5.0), 0.0);
        w.step(0.034);
        assert_eq!(w.drawn, (3.0, -3.0));
    }

    #[test]
    fn follows_the_stacked_windows() {
        let w = PtrWarp::new((100.0, 50.0), (90.0, 40.0), -4.0).shifted(-10.0);
        assert_eq!((w.at, w.drawn), ((100.0, 44.0), (90.0, 34.0)));
    }

    #[test]
    fn motion_past_two_pixels_ends_it() {
        let w = PtrWarp::new((100.0, 50.0), (0.0, 0.0), 0.0);
        assert!(!w.left_by((102.0, 48.0)));
        assert!(w.left_by((103.0, 50.0)));
        assert!(w.left_by((100.0, 47.5)));
    }
}
