//! Piano-roll history of emitted MIDI notes (flowing right -> left). Pure, unit-tested geometry.

use std::fmt::Write;

/// Bit layout of a packed word, lowest field first:
/// `note (7) | velocity (8) | kind (1) | valid (1) | detune (8) | ts_ms (38)`.
const NOTE_MASK: u64 = 0x7F;
const VALID: u64 = 1 << 16;
const DETUNE_BIAS: f32 = 128.0;
const TS_SHIFT: u32 = 25;

#[cfg(test)]
pub fn pack(ts_ms: u64, note: u8, vel: u8, on: bool) -> u64 {
    pack_detuned(ts_ms, note, vel, on, 0.0)
}

/// Packs one event into a single atomic word. `cents` is stored as a signed byte biased by 128.
pub fn pack_detuned(ts_ms: u64, note: u8, vel: u8, on: bool, cents: f32) -> u64 {
    let detune = ((cents.round() as i64 + DETUNE_BIAS as i64).clamp(0, 255)) as u64;
    (ts_ms << TS_SHIFT) | VALID | (detune << 17) | ((on as u64) << 15) | ((vel as u64) << 7) | (note as u64 & NOTE_MASK)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ev {
    pub t_ms: u64,
    pub note: u8,
    pub vel: u8,
    pub on: bool,
    /// Microtonal offset in cents, -64..=63.
    pub cents: f32,
}

pub fn unpack(w: u64) -> Option<Ev> {
    (w & VALID != 0).then(|| Ev {
        t_ms: w >> TS_SHIFT,
        note: (w & NOTE_MASK) as u8,
        vel: ((w >> 7) & 0xFF) as u8,
        on: (w >> 15) & 1 == 1,
        cents: ((w >> 17) & 0xFF) as f32 - DETUNE_BIAS,
    })
}

#[derive(Clone, Debug, Default)]
pub struct Roll {
    /// Shading of black-key rows (fill).
    pub bg: String,
    /// C lines (stroke).
    pub grid: String,
    /// Note rectangles (fill, gradient applied in the UI).
    pub notes: String,
    pub lo: i32,
    pub hi: i32,
}

#[derive(Clone, Copy, Debug)]
struct Seg {
    start: i64,
    end: i64,
    note: u8,
    cents: f32,
}

fn rect(s: &mut String, x0: f32, y0: f32, x1: f32, y1: f32) {
    let _ = write!(s, "M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} Z ", x0, y0, x1, y0, x1, y1, x0, y1);
}

/// `events` may be unsorted / contain stale entries; `now_ms` is the right edge of the view.
pub fn build(events: &mut Vec<Ev>, now_ms: u64, window_ms: u64, w: f32, h: f32) -> Roll {
    let t_min = now_ms as i64 - window_ms as i64;
    // Stable sort by time only: at the same millisecond an On must stay before its Off.
    events.sort_by_key(|e| e.t_ms);
    let mut segs: Vec<Seg> = Vec::new();
    let mut open: [Option<(i64, f32)>; 128] = [None; 128];
    for e in events.iter() {
        let t = e.t_ms as i64;
        let n = (e.note & 0x7F) as usize;
        if e.on {
            if let Some((s, c)) = open[n].take() {
                segs.push(Seg { start: s, end: t, note: n as u8, cents: c });
            }
            open[n] = Some((t, e.cents));
        } else if let Some((s, c)) = open[n].take() {
            segs.push(Seg { start: s, end: t, note: n as u8, cents: c });
        }
        // An Off with no matching On is a ring-wrap/stale event; skip it.
    }
    for (n, o) in open.iter().enumerate() {
        if let Some((s, c)) = o {
            segs.push(Seg { start: *s, end: now_ms as i64, note: n as u8, cents: *c });
        }
    }
    segs.retain(|s| s.end >= t_min && s.start <= now_ms as i64 + 50);

    // octave-aligned pitch range
    let (mut lo, mut hi) = (48, 72);
    if !segs.is_empty() {
        let mn = segs.iter().map(|s| s.note as i32).min().unwrap();
        let mx = segs.iter().map(|s| s.note as i32).max().unwrap();
        lo = mn.div_euclid(12) * 12;
        hi = (mx.div_euclid(12) + 1) * 12;
        if hi - lo < 24 {
            hi = lo + 24;
        }
    }
    let rows = (hi - lo) as f32;
    let rh = h / rows;
    let x_of = |t: i64| -> f32 { (w - (now_ms as i64 - t) as f32 * w / window_ms as f32).clamp(0.0, w) };

    let mut roll = Roll { lo, hi, ..Default::default() };
    for n in lo..hi {
        let y = (hi - 1 - n) as f32 * rh;
        if matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10) {
            rect(&mut roll.bg, 0.0, y, w, y + rh);
        }
        if n.rem_euclid(12) == 0 {
            let yb = y + rh;
            let _ = write!(roll.grid, "M0 {:.1} L{:.1} {:.1} ", yb, w, yb);
        }
    }
    for s in &segs {
        let n = s.note as i32;
        if !(lo..hi).contains(&n) {
            continue;
        }
        // A cent offset shifts the note inside its own row, so NTS pitches read as microtonal.
        let y = (hi - 1 - n) as f32 * rh - s.cents / 100.0 * rh;
        let (x0, x1) = (x_of(s.start), x_of(s.end));
        rect(&mut roll.notes, x0, y + 0.5, x1.max(x0 + 2.0), y + rh - 0.5);
    }
    for s in [&mut roll.bg, &mut roll.grid, &mut roll.notes] {
        if s.is_empty() {
            s.push_str("M0 0 ");
        }
    }
    roll
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip_and_empty() {
        let e = Ev { t_ms: 123_456_789, note: 60, vel: 100, on: true, cents: 0.0 };
        assert_eq!(unpack(pack(e.t_ms, e.note, e.vel, e.on)), Some(Ev { cents: 0.0, ..e }));
        assert_eq!(unpack(pack(5, 0, 0, false)).unwrap().on, false);
        assert_eq!(unpack(0), None);
    }

    #[test]
    fn detune_survives_the_round_trip() {
        for cents in [-63.0, -13.7, -0.5, 0.0, 7.8, 36.8, 63.0, 100.0] {
            let ev = unpack(pack_detuned(1_000, 62, 100, true, cents)).unwrap();
            assert!((ev.cents - cents).abs() <= 0.5, "{cents} -> {}", ev.cents);
        }
        let hi = unpack(pack_detuned(1_234_567, 62, 100, true, 100.0)).unwrap();
        assert_eq!((hi.t_ms, hi.note, hi.vel, hi.on, hi.cents), (1_234_567, 62, 100, true, 100.0));
        let over = unpack(pack_detuned(1_000, 62, 100, true, 400.0)).unwrap();
        assert_eq!((over.t_ms, over.note, over.vel, over.on), (1_000, 62, 100, true));
        let at = |cents: f32| {
            let mut evs = vec![
                Ev { t_ms: 900, note: 64, vel: 90, on: true, cents },
                Ev { t_ms: 980, note: 64, vel: 0, on: false, cents },
            ];
            build(&mut evs, 1_000, 6_000, 600.0, 96.0).notes
        };
        assert_ne!(at(0.0), at(30.0));
        assert!(!at(30.0).contains("NaN"));
    }

    #[test]
    fn pairs_notes_and_scrolls_right_to_left() {
        let mut evs = vec![
            Ev { t_ms: 4_000, note: 60, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 4_500, note: 60, vel: 0, on: false, cents: 0.0 },
        ];
        let a = build(&mut evs.clone(), 5_000, 6_000, 600.0, 96.0);
        let b = build(&mut evs, 5_500, 6_000, 600.0, 96.0);
        assert_ne!(a.notes, b.notes);
        assert!(a.notes.starts_with('M') && !a.notes.contains("NaN"));
        assert_eq!((a.lo, a.hi), (60, 84));
    }

    #[test]
    fn open_note_extends_to_now_and_old_notes_drop_out() {
        let mut open = vec![Ev { t_ms: 1_000, note: 64, vel: 90, on: true, cents: 0.0 }];
        assert!(build(&mut open, 2_000, 6_000, 600.0, 96.0).notes.len() > 4);
        let mut old = vec![
            Ev { t_ms: 100, note: 64, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 200, note: 64, vel: 0, on: false, cents: 0.0 },
        ];
        assert_eq!(build(&mut old, 20_000, 6_000, 600.0, 96.0).notes.trim(), "M0 0");
    }

    #[test]
    fn stray_note_off_does_not_stretch_back_to_the_window_edge() {
        // A wrapped ring can leave an Off with no matching On.
        let mut evs = vec![Ev { t_ms: 3_000, note: 60, vel: 0, on: false, cents: 0.0 }];
        let r = build(&mut evs, 5_000, 6_000, 600.0, 96.0);
        assert_eq!(r.notes.trim(), "M0 0");
    }

    #[test]
    fn same_millisecond_on_and_off_pair_as_a_short_note() {
        // Short notes can land On and Off in the same millisecond; keep On first.
        let mut evs = vec![
            Ev { t_ms: 4_000, note: 60, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 4_000, note: 60, vel: 0, on: false, cents: 0.0 },
        ];
        let r = build(&mut evs, 5_000, 6_000, 600.0, 96.0);
        assert!(r.notes.starts_with("M500.0"), "unexpected path: {}", r.notes);
        assert!(!r.notes.starts_with("M0.0"), "note was drawn from the window edge");
    }

    #[test]
    fn range_follows_notes_in_whole_octaves() {
        let mut evs = vec![
            Ev { t_ms: 900, note: 38, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 950, note: 38, vel: 0, on: false, cents: 0.0 },
            Ev { t_ms: 960, note: 77, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 990, note: 77, vel: 0, on: false, cents: 0.0 },
        ];
        let r = build(&mut evs, 1_000, 6_000, 600.0, 96.0);
        assert_eq!((r.lo, r.hi), (36, 84));
    }
}
