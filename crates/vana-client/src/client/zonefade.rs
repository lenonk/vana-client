//! A zone change fades to black, then the new zone fades in once it is ready. While the screen darkens, the old zone's
//! last frame is held: the camera has already moved to the new zone.

#[derive(Default, Clone, Copy, Debug)]
pub struct Hold {
    /// The frame is held: the last one drawn, copied from the scene texture before the next frame draws.
    pub captured: bool,
}

/// A zone change starts (a zone line touched, or the server's 0x00B): hold the frame once a world frame was drawn and
/// the screen is not black yet.
pub fn arm(h: &mut Option<Hold>, drawn: bool, fade: f32) {
    if drawn && fade < 1.0 && h.is_none() {
        log::info!("zone fade: holding the frame");
        *h = Some(Hold::default());
    }
}

/// Each frame after the fade: arms while a zone line waits; the hold ends once the screen is black.
pub fn step(h: &mut Option<Hold>, zoning: bool, drawn: bool, fade: f32) {
    if zoning {
        arm(h, drawn, fade);
    }
    if h.is_some_and(|x| x.captured) && fade >= 1.0 {
        log::info!("zone fade: black");
        *h = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_until_black() {
        let mut h = None;
        // Not zoning, or no world drawn yet (the login): nothing.
        step(&mut h, false, true, 0.0);
        step(&mut h, true, false, 0.0);
        assert!(h.is_none());
        step(&mut h, true, true, 0.0);
        assert!(h.is_some());
        // Not captured yet: kept even at black.
        step(&mut h, true, true, 1.0);
        assert!(h.is_some());
        h.as_mut().unwrap().captured = true;
        step(&mut h, true, true, 0.6);
        assert!(h.is_some());
        step(&mut h, true, true, 1.0);
        assert!(h.is_none());
        // Still waiting on the zone line, already black: no new hold.
        step(&mut h, true, true, 1.0);
        assert!(h.is_none());
    }
}
