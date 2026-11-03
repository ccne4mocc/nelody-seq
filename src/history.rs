//! Piano-roll history of emitted MIDI notes (flowing right -> left). Pure, unit-tested geometry.

use crate::sequence::MAX_STEPS;
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
    /// Alternating bar shading (fill).
    pub bg_bars: String,
    /// C lines (stroke).
    pub grid: String,
    /// Step, beat and bar lines (stroke paths with different weights in the UI).
    pub grid_steps: String,
    pub grid_beats: String,
    pub grid_bars: String,
    /// Note rectangles (fill, gradient applied in the UI).
    pub notes: String,
    /// Brightness layers: `notes` is the base, these add mid and high velocity notes.
    pub notes_mid: String,
    pub notes_hard: String,
    /// Ghost of the current pattern's loop, phase-locked to the transport (stroke in the UI).
    pub pattern: String,
    /// Slide connectors for the current-loop ghost (stroke in the UI).
    pub pattern_slides: String,
    pub lo: i32,
    pub hi: i32,
}

/// One note of the current loop, already resolved through `pitch_for`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub step: usize,
    pub note: u8,
    pub cents: f32,
    /// Hold length in steps.
    pub hold: f32,
    pub slide: bool,
}

/// Grid spacing used by the roll's time lines.
#[derive(Clone, Copy, Debug)]
pub struct GridSpec {
    pub step_ms: f32,
    pub steps_per_beat: f32,
    pub beats_per_bar: f32,
}

/// One pattern loop projected onto the history time axis.
pub struct Overlay<'a> {
    pub notes: &'a [Note],
    /// One grid step in milliseconds.
    pub step_ms: f32,
    /// Continuous position inside the loop at the right edge of the roll.
    pub phase_steps: f32,
    /// Loop length in steps; fractional lengths wrap mid-cell.
    pub loop_steps: f32,
    /// Swing delay applied to odd steps, in steps.
    pub swing_steps: f32,
}

#[derive(Clone, Copy, Debug)]
struct Seg {
    start: i64,
    end: i64,
    note: u8,
    vel: u8,
    cents: f32,
}

fn rect(s: &mut String, x0: f32, y0: f32, x1: f32, y1: f32) {
    let _ = write!(s, "M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} Z ", x0, y0, x1, y0, x1, y1, x0, y1);
}

/// `events` may be unsorted / contain stale entries; `now_ms` is the right edge of the view.
#[allow(dead_code)]
pub fn build(events: &mut Vec<Ev>, now_ms: u64, window_ms: u64, w: f32, h: f32, overlay: Option<&Overlay>) -> Roll {
    build_with_grid(events, now_ms, window_ms, w, h, overlay, None)
}

/// Build the roll with an optional musical time grid.
pub fn build_with_grid(
    events: &mut Vec<Ev>,
    now_ms: u64,
    window_ms: u64,
    w: f32,
    h: f32,
    overlay: Option<&Overlay>,
    grid: Option<&GridSpec>,
) -> Roll {
    let t_min = now_ms as i64 - window_ms as i64;
    // Stable sort by time only: at the same millisecond an On must stay before its Off.
    events.sort_by_key(|e| e.t_ms);
    let mut segs: Vec<Seg> = Vec::new();
    let mut open: [Option<(i64, f32, u8)>; 128] = [None; 128];
    for e in events.iter() {
        let t = e.t_ms as i64;
        let n = (e.note & 0x7F) as usize;
        if e.on {
            if let Some((s, c, v)) = open[n].take() {
                segs.push(Seg { start: s, end: t, note: n as u8, vel: v, cents: c });
            }
            open[n] = Some((t, e.cents, e.vel));
        } else if let Some((s, c, v)) = open[n].take() {
            segs.push(Seg { start: s, end: t, note: n as u8, vel: v, cents: c });
        }
        // An Off with no matching On is a ring-wrap/stale event; skip it.
    }
    for (n, o) in open.iter().enumerate() {
        if let Some((s, c, v)) = o {
            segs.push(Seg { start: *s, end: now_ms as i64, note: n as u8, vel: *v, cents: *c });
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
        let x1 = x1.max(x0 + 2.0);
        rect(&mut roll.notes, x0, y + 0.5, x1, y + rh - 0.5);
        // Velocity is packed with the event; split it into two brighter overlay layers so the UI
        // can shade notes without a per-note model.
        if s.vel >= 64 {
            rect(&mut roll.notes_mid, x0, y + 0.5, x1, y + rh - 0.5);
        }
        if s.vel >= 100 {
            rect(&mut roll.notes_hard, x0, y + 0.5, x1, y + rh - 0.5);
        }
    }

    if let Some(o) = overlay {
        pattern_overlay(&mut roll, o, window_ms, w, lo, hi, rh);
    }
    if let Some(g) = grid {
        time_grid(&mut roll, now_ms, window_ms, w, h, g);
    }

    for s in [
        &mut roll.bg,
        &mut roll.bg_bars,
        &mut roll.grid,
        &mut roll.grid_steps,
        &mut roll.grid_beats,
        &mut roll.grid_bars,
        &mut roll.notes,
        &mut roll.notes_mid,
        &mut roll.notes_hard,
        &mut roll.pattern,
        &mut roll.pattern_slides,
    ] {
        if s.is_empty() {
            s.push_str("M0 0 ");
        }
    }
    roll
}

/// Project the current loop backwards from `now_ms` so the ghost can be compared with what the
/// scheduler actually played. Bounded by the window and by `MAX_STEPS` drawn notes.
fn pattern_overlay(roll: &mut Roll, o: &Overlay, window_ms: u64, w: f32, lo: i32, hi: i32, rh: f32) {
    if o.notes.is_empty() || !o.step_ms.is_finite() || o.step_ms <= 0.0 { return; }
    let loop_len = if o.loop_steps.is_finite() && o.loop_steps >= 1.0 {
        o.loop_steps
    } else {
        o.notes.iter().map(|n| n.step + 1).max().unwrap_or(1) as f32
    };
    if !(loop_len > 0.0) { return; }
    let phase = o.phase_steps.rem_euclid(loop_len);
    let step_px = (o.step_ms * w / window_ms as f32).max(0.01);
    let swing = if o.swing_steps.is_finite() { o.swing_steps.clamp(0.0, 0.5) } else { 0.0 };
    let cycles = ((window_ms as f32 / (loop_len * o.step_ms)).ceil() as usize + 1).min(MAX_STEPS);
    let mut drawn = 0usize;
    'cycles: for cycle in 0..cycles {
        for (idx, note) in o.notes.iter().enumerate() {
            let onset = |step: usize, wrapped: bool| {
                step as f32
                    + if step % 2 == 1 { swing } else { 0.0 }
                    + if wrapped { loop_len } else { 0.0 }
            };
            let age = phase - onset(note.step, false) + cycle as f32 * loop_len;
            if age < 0.0 { continue; }
            let x0 = w - age * step_px;
            let bw = (note.hold.clamp(0.02, 8.0) * step_px).max(1.5);
            if x0 >= w { continue; }
            if x0 + bw < 0.0 { break 'cycles; }
            let n = note.note as i32;
            if !(lo..hi).contains(&n) { continue; }
            let y = (hi - 1 - n) as f32 * rh - note.cents / 100.0 * rh;
            rect(&mut roll.pattern, x0, y + 0.5, x0 + bw, y + rh - 0.5);
            drawn += 1;
            if drawn >= MAX_STEPS { break 'cycles; }
            // A slide holds until the next onset; draw the legato link so it reads as a slide
            // rather than just a very long note.
            if note.slide {
                let next = &o.notes[(idx + 1) % o.notes.len()];
                let wrapped = next.step <= note.step;
                let next_age = phase - onset(next.step, wrapped) + cycle as f32 * loop_len;
                if next_age >= 0.0 {
                    let x1 = w - next_age * step_px;
                    if x1 > x0 + bw {
                        let ya = y + rh * 0.5;
                        let _ = write!(roll.pattern_slides, "M{:.1} {:.1} L{:.1} {:.1} ", x0 + bw, ya, x1, ya);
                    }
                }
            }
        }
    }
}

/// Draw the bar shading and the step/beat/bar lines. Line density is bounded by pixel spacing so
/// a 1024-step loop cannot blow up the path string.
fn time_grid(roll: &mut Roll, now_ms: u64, window_ms: u64, w: f32, h: f32, g: &GridSpec) {
    let step_ms = g.step_ms as f64;
    if !step_ms.is_finite() || step_ms <= 0.0 || window_ms == 0 { return; }
    let spb = if g.steps_per_beat.is_finite() && g.steps_per_beat > 0.0 {
        g.steps_per_beat as f64
    } else {
        4.0
    };
    let bar_steps = spb * if g.beats_per_bar.is_finite() && g.beats_per_bar > 0.0 {
        g.beats_per_bar as f64
    } else {
        4.0
    };
    let window = window_ms as f64;
    let now = now_ms as f64;
    let step_px = step_ms * w as f64 / window;
    let x_of = |t: f64| (w as f64 - (now - t) * w as f64 / window) as f32;
    let first = ((now - window) / step_ms).floor() as i64;
    let last = (now / step_ms).ceil() as i64;
    let count = (last - first + 1).clamp(0, 8192);
    for n in 0..count {
        let k = first + n;
        let x = x_of(k as f64 * step_ms);
        if !(0.0..=w).contains(&x) { continue; }
        let beat_pos = (k as f64).rem_euclid(spb);
        let bar_pos = (k as f64).rem_euclid(bar_steps);
        // Drop finer lines as they approach pixel spacing: at 1px they turn the roll into a
        // solid block. Bars always stay; beats need ~2.5px and steps ~4px to read cleanly.
        if bar_pos.abs() < 1e-9 {
            let _ = write!(roll.grid_bars, "M{x:.1} 0 L{x:.1} {h:.1} ");
        } else if beat_pos.abs() < 1e-9 {
            if step_px * spb >= 2.5 {
                let _ = write!(roll.grid_beats, "M{x:.1} 0 L{x:.1} {h:.1} ");
            }
        } else if step_px >= 4.0 {
            let _ = write!(roll.grid_steps, "M{x:.1} 0 L{x:.1} {h:.1} ");
        }
    }
    // Alternate bar shading. Bars are bounded by the visible window.
    let bar_ms = bar_steps * step_ms;
    if bar_ms > 0.0 {
        let first_bar = ((now - window) / bar_ms).floor() as i64;
        let last_bar = (now / bar_ms).ceil() as i64;
        let count = (last_bar - first_bar + 1).clamp(0, 4096);
        for n in 0..count {
            let b = first_bar + n;
            if b.rem_euclid(2) == 0 { continue; }
            let x0 = x_of(b as f64 * bar_ms).max(0.0);
            let x1 = x_of((b + 1) as f64 * bar_ms).min(w);
            if x1 > x0 {
                rect(&mut roll.bg_bars, x0, 0.0, x1, h);
            }
        }
    }
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
            build(&mut evs, 1_000, 6_000, 600.0, 96.0, None).notes
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
        let a = build(&mut evs.clone(), 5_000, 6_000, 600.0, 96.0, None);
        let b = build(&mut evs, 5_500, 6_000, 600.0, 96.0, None);
        assert_ne!(a.notes, b.notes);
        assert!(a.notes.starts_with('M') && !a.notes.contains("NaN"));
        assert_eq!((a.lo, a.hi), (60, 84));
    }

    #[test]
    fn open_note_extends_to_now_and_old_notes_drop_out() {
        let mut open = vec![Ev { t_ms: 1_000, note: 64, vel: 90, on: true, cents: 0.0 }];
        assert!(build(&mut open, 2_000, 6_000, 600.0, 96.0, None).notes.len() > 4);
        let mut old = vec![
            Ev { t_ms: 100, note: 64, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 200, note: 64, vel: 0, on: false, cents: 0.0 },
        ];
        assert_eq!(build(&mut old, 20_000, 6_000, 600.0, 96.0, None).notes.trim(), "M0 0");
    }

    #[test]
    fn stray_note_off_does_not_stretch_back_to_the_window_edge() {
        // A wrapped ring can leave an Off with no matching On.
        let mut evs = vec![Ev { t_ms: 3_000, note: 60, vel: 0, on: false, cents: 0.0 }];
        let r = build(&mut evs, 5_000, 6_000, 600.0, 96.0, None);
        assert_eq!(r.notes.trim(), "M0 0");
    }

    #[test]
    fn same_millisecond_on_and_off_pair_as_a_short_note() {
        // Short notes can land On and Off in the same millisecond; keep On first.
        let mut evs = vec![
            Ev { t_ms: 4_000, note: 60, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 4_000, note: 60, vel: 0, on: false, cents: 0.0 },
        ];
        let r = build(&mut evs, 5_000, 6_000, 600.0, 96.0, None);
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
        let r = build(&mut evs, 1_000, 6_000, 600.0, 96.0, None);
        assert_eq!((r.lo, r.hi), (36, 84));
    }

    fn note(step: usize, note: u8, cents: f32, hold: f32) -> Note {
        Note { step, note, cents, hold, slide: false }
    }

    fn loop_notes() -> Vec<Note> {
        vec![note(0, 60, 0.0, 0.25), note(1, 64, 0.0, 0.25), note(2, 67, 0.0, 0.25), note(3, 71, 0.0, 0.25)]
    }

    #[test]
    fn overlay_projects_the_loop_backwards_and_wraps() {
        let notes = loop_notes();
        let o = Overlay { notes: &notes, step_ms: 125.0, phase_steps: 1.0, loop_steps: 4.0, swing_steps: 0.0 };
        let mut evs = vec![];
        let r = build(&mut evs, 10_000, 2_000, 600.0, 96.0, Some(&o));
        assert!(r.pattern.starts_with("M562.5 "), "step 0 should sit one step (37.5 px) left of now: {}", r.pattern);
        // Step 3 is still ahead of the playhead, so it only appears from the previous cycle.
        assert!(r.pattern.contains("M525.0 "), "the wrapped note is missing: {}", r.pattern);
        // Older cycles keep filling the window instead of leaving the left half empty.
        assert!(r.pattern.matches('M').count() > notes.len(), "{}", r.pattern);
        assert!(!r.pattern.contains("NaN"));
        // A silent history still gives the overlay the usual two-octave range.
        assert_eq!((r.lo, r.hi), (48, 72));
    }

    #[test]
    fn overlay_uses_the_rolls_pitch_range_and_cents() {
        let detuned = vec![note(0, 48, 0.0, 0.5), note(1, 84, -30.0, 0.5)];
        let plain = vec![note(0, 48, 0.0, 0.5), note(1, 84, 0.0, 0.5)];
        let mut evs = vec![
            Ev { t_ms: 9_000, note: 48, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 9_100, note: 48, vel: 0, on: false, cents: 0.0 },
            Ev { t_ms: 9_200, note: 84, vel: 90, on: true, cents: 0.0 },
            Ev { t_ms: 9_300, note: 84, vel: 0, on: false, cents: 0.0 },
        ];
        let a = build(
            &mut evs.clone(),
            10_000,
            6_000,
            600.0,
            96.0,
            Some(&Overlay { notes: &detuned, step_ms: 125.0, phase_steps: 0.0, loop_steps: 2.0, swing_steps: 0.0 }),
        );
        let b = build(
            &mut evs,
            10_000,
            6_000,
            600.0,
            96.0,
            Some(&Overlay { notes: &plain, step_ms: 125.0, phase_steps: 0.0, loop_steps: 2.0, swing_steps: 0.0 }),
        );
        assert!(a.hi - a.lo >= 24);
        // The cent offset shifts the note inside its row, so the geometry must differ.
        assert_ne!(a.pattern, b.pattern);
        assert!(!a.pattern.contains("NaN"));
    }

    #[test]
    fn time_grid_draws_beat_and_bar_lines() {
        let mut evs = vec![];
        let grid = GridSpec { step_ms: 125.0, steps_per_beat: 4.0, beats_per_bar: 4.0 };
        let r = build_with_grid(&mut evs, 10_000, 2_000, 800.0, 200.0, None, Some(&grid));
        assert!(r.grid_bars.matches('M').count() >= 2);
        assert!(r.grid_beats.matches('M').count() >= 2);
        assert!(!r.grid_steps.contains("NaN"));
        assert!(!r.bg_bars.contains("NaN"));
    }

    #[test]
    fn dense_time_grid_drops_step_and_beat_lines() {
        let grid = GridSpec { step_ms: 1.0, steps_per_beat: 4.0, beats_per_bar: 4.0 };
        let r = build_with_grid(&mut vec![], 10_000, 10_000, 800.0, 200.0, None, Some(&grid));
        assert_eq!(r.grid_steps, "M0 0 ");
        assert_eq!(r.grid_beats, "M0 0 ");
        assert!(r.grid_bars.matches('M').count() > 1);
    }

    #[test]
    fn slide_connectors_follow_the_pattern_ghost() {
        let mut notes = loop_notes();
        notes[0].slide = true;
        let o = Overlay { notes: &notes, step_ms: 125.0, phase_steps: 4.0, loop_steps: 4.0, swing_steps: 0.0 };
        let mut evs = vec![];
        let r = build(&mut evs, 10_000, 2_000, 600.0, 96.0, Some(&o));
        assert!(r.pattern_slides.starts_with('M'), "missing slide connector: {}", r.pattern_slides);
    }

    #[test]
    fn overlay_is_absent_without_a_clock_and_stays_bounded() {
        let mut evs = vec![];
        let none = build(&mut evs, 1_000, 6_000, 600.0, 96.0, None);
        assert_eq!(none.pattern.trim(), "M0 0");
        let notes = loop_notes();
        // A one-step loop over a long window would otherwise repeat forever.
        let o = Overlay { notes: &notes[..1], step_ms: 1.0, phase_steps: 0.0, loop_steps: 1.0, swing_steps: 0.0 };
        let r = build(&mut evs, 1_000, 60_000, 600.0, 96.0, Some(&o));
        assert!(r.pattern.matches('M').count() <= crate::sequence::MAX_STEPS);
        // A non-finite clock produces no overlay instead of a broken path.
        let bad = Overlay { notes: &notes, step_ms: f32::NAN, phase_steps: 0.0, loop_steps: 4.0, swing_steps: 0.0 };
        assert_eq!(build(&mut evs, 1_000, 6_000, 600.0, 96.0, Some(&bad)).pattern.trim(), "M0 0");
    }
}
