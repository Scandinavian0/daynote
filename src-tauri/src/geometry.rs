//! Window placement maths. Everything here is in physical pixels and has no
//! access to real windows or monitors, so it can be unit tested.

/// A rectangle in physical pixels: top-left corner plus size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// Width and height of the overlap with `o` (0 when they do not touch).
    pub fn overlap(&self, o: &Rect) -> (i32, i32) {
        let w = self.right().min(o.right()) - self.x.max(o.x);
        let h = self.bottom().min(o.bottom()) - self.y.max(o.y);
        (w.max(0), h.max(0))
    }
    /// Stored form: `x,y,w,h`.
    pub fn to_setting(self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.w, self.h)
    }
    pub fn parse(s: &str) -> Option<Rect> {
        let v: Vec<i32> = s
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<Vec<_>>>()?;
        match v.as_slice() {
            [x, y, w, h] if *w > 0 && *h > 0 => Some(Rect::new(*x, *y, *w, *h)),
            _ => None,
        }
    }
}

/// Logical sizes of the two forms of the main window and their minimums.
pub const COMPACT: (f64, f64) = (380.0, 640.0);
pub const COMPACT_MIN: (f64, f64) = (340.0, 420.0);
pub const EXPANDED: (f64, f64) = (980.0, 680.0);
pub const EXPANDED_MIN: (f64, f64) = (860.0, 520.0);
/// First-run placement of the compact panel: gap to the right edge and to the top.
pub const EDGE_MARGIN: f64 = 16.0;
pub const TOP_OFFSET: f64 = 80.0;
/// Reminder window width (logical) and quick-add box size (logical).
pub const REMINDER_WIDTH: f64 = 384.0;
pub const QUICKADD: (f64, f64) = (600.0, 92.0);

/// A logical size in physical pixels at `scale`.
pub fn scaled((w, h): (f64, f64), scale: f64) -> (i32, i32) {
    ((w * scale).round() as i32, (h * scale).round() as i32)
}

/// Shrink `r` to fit `work` if needed, then shift it so it lies fully inside.
pub fn clamp_into(r: Rect, work: Rect) -> Rect {
    let w = r.w.min(work.w);
    let h = r.h.min(work.h);
    let x = r.x.clamp(work.x, work.right() - w);
    let y = r.y.clamp(work.y, work.bottom() - h);
    Rect::new(x, y, w, h)
}

/// Default compact panel: right side of the work area with a margin, near the top.
pub fn default_compact(work: Rect, scale: f64) -> Rect {
    let (w, h) = scaled(COMPACT, scale);
    let margin = (EDGE_MARGIN * scale).round() as i32;
    let top = (TOP_OFFSET * scale).round() as i32;
    clamp_into(Rect::new(work.right() - margin - w, work.y + top, w, h), work)
}

/// Switch between the compact and expanded forms: the new size keeps the current
/// right edge and top, then the whole window is kept inside the work area.
pub fn switch_form(current: Rect, new_size: (i32, i32), work: Rect) -> Rect {
    let (w, h) = new_size;
    clamp_into(Rect::new(current.right() - w, current.y, w, h), work)
}

/// Index of the monitor (work area) that overlaps `r` the most.
pub fn best_monitor(r: Rect, monitors: &[Rect]) -> Option<usize> {
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (w, h) = r.overlap(m);
            (i, i64::from(w) * i64::from(h))
        })
        .filter(|(_, area)| *area > 0)
        .max_by_key(|(_, area)| *area)
        .map(|(i, _)| i)
}

/// How much of a restored window must be on some monitor to count as "on screen".
const VISIBLE_W: i32 = 120;
const VISIBLE_H: i32 = 48;

/// The saved rect (grown to `min` if smaller) when enough of it is on a monitor,
/// otherwise `fallback`.
pub fn restore(saved: Option<Rect>, monitors: &[Rect], min: (i32, i32), fallback: Rect) -> Rect {
    let Some(r) = saved else { return fallback };
    let r = Rect::new(r.x, r.y, r.w.max(min.0), r.h.max(min.1));
    let visible = monitors.iter().any(|m| {
        let (w, h) = r.overlap(m);
        w >= VISIBLE_W.min(r.w) && h >= VISIBLE_H.min(r.h)
    });
    if visible {
        r
    } else {
        fallback
    }
}

/// The reminder stack flush with the bottom-right corner of the work area.
pub fn bottom_right(work: Rect, (w, h): (i32, i32)) -> Rect {
    let w = w.min(work.w);
    let h = h.clamp(1, work.h);
    Rect::new(work.right() - w, work.bottom() - h, w, h)
}

/// The quick-add box: horizontally centred, its top about 28% down the work area.
pub fn quickadd_rect(work: Rect, (w, h): (i32, i32)) -> Rect {
    let x = work.x + (work.w - w) / 2;
    let y = work.y + (f64::from(work.h) * 0.28).round() as i32;
    clamp_into(Rect::new(x, y, w, h), work)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);

    #[test]
    fn setting_round_trip() {
        let r = Rect::new(-1200, 40, 380, 640);
        assert_eq!(Rect::parse(&r.to_setting()), Some(r));
        assert_eq!(Rect::parse("1,2,3"), None);
        assert_eq!(Rect::parse("1,2,0,5"), None);
        assert_eq!(Rect::parse("a,b,c,d"), None);
    }

    #[test]
    fn default_compact_hugs_the_right_edge() {
        let r = default_compact(WORK, 1.0);
        assert_eq!(r, Rect::new(1920 - 16 - 380, 80, 380, 640));
        let r = default_compact(Rect::new(0, 0, 3840, 2080), 2.0);
        assert_eq!(r, Rect::new(3840 - 32 - 760, 160, 760, 1280));
    }

    #[test]
    fn expanding_keeps_the_right_edge() {
        let compact = Rect::new(1500, 100, 380, 640);
        let r = switch_form(compact, (980, 680), WORK);
        assert_eq!(r.right(), compact.right());
        assert_eq!(r, Rect::new(1880 - 980, 100, 980, 680));
        // and back again
        let back = switch_form(r, (380, 640), WORK);
        assert_eq!(back, compact);
    }

    #[test]
    fn expanding_near_the_left_edge_is_pushed_inside() {
        let compact = Rect::new(20, 100, 380, 640);
        let r = switch_form(compact, (980, 680), WORK);
        assert_eq!(r, Rect::new(0, 100, 980, 680));
        assert!(r.x >= WORK.x && r.right() <= WORK.right());
    }

    #[test]
    fn expanding_near_the_bottom_is_pulled_up() {
        let compact = Rect::new(1500, 600, 380, 400);
        let r = switch_form(compact, (980, 680), WORK);
        assert_eq!(r.right(), compact.right());
        assert_eq!(r.bottom(), WORK.bottom());
        assert_eq!(r.y, 1040 - 680);
    }

    #[test]
    fn expanding_on_a_secondary_monitor_stays_on_it() {
        let left = Rect::new(-1280, 0, 1280, 984);
        let compact = Rect::new(-400, 50, 380, 640);
        let r = switch_form(compact, (980, 680), left);
        assert_eq!(r.right(), -20);
        assert!(r.x >= left.x && r.right() <= left.right());
    }

    #[test]
    fn window_bigger_than_the_work_area_is_shrunk() {
        let small = Rect::new(0, 0, 800, 600);
        let r = switch_form(Rect::new(400, 0, 380, 500), (980, 680), small);
        assert_eq!(r, Rect::new(0, 0, 800, 600));
    }

    #[test]
    fn restore_keeps_visible_rect_and_rejects_offscreen() {
        let fallback = default_compact(WORK, 1.0);
        let mons = [WORK, Rect::new(1920, 0, 1920, 1040)];
        let saved = Rect::new(2000, 100, 380, 640);
        assert_eq!(restore(Some(saved), &mons, (340, 420), fallback), saved);
        // the second monitor has been unplugged
        assert_eq!(restore(Some(saved), &mons[..1], (340, 420), fallback), fallback);
        // only a 10px sliver is on screen
        let sliver = Rect::new(1910, 100, 380, 640);
        assert_eq!(restore(Some(sliver), &mons[..1], (340, 420), fallback), fallback);
        assert_eq!(restore(None, &mons, (340, 420), fallback), fallback);
        // too small saved size grows to the minimum
        let tiny = Rect::new(100, 100, 100, 100);
        assert_eq!(restore(Some(tiny), &mons, (340, 420), fallback), Rect::new(100, 100, 340, 420));
    }

    #[test]
    fn best_monitor_by_overlap() {
        let mons = [WORK, Rect::new(1920, 0, 1920, 1040)];
        assert_eq!(best_monitor(Rect::new(1800, 0, 400, 400), &mons), Some(1));
        assert_eq!(best_monitor(Rect::new(100, 0, 400, 400), &mons), Some(0));
        assert_eq!(best_monitor(Rect::new(9000, 0, 400, 400), &mons), None);
    }

    #[test]
    fn reminder_sits_in_the_bottom_right_corner() {
        let r = bottom_right(WORK, (384, 200));
        assert_eq!(r, Rect::new(1920 - 384, 1040 - 200, 384, 200));
        let tall = bottom_right(WORK, (384, 5000));
        assert_eq!(tall.y, 0);
        assert_eq!(tall.h, 1040);
    }

    #[test]
    fn quickadd_is_centred_high() {
        let r = quickadd_rect(Rect::new(1920, 0, 1920, 1040), (600, 92));
        assert_eq!(r.x, 1920 + 660);
        assert_eq!(r.y, 291);
    }
}
