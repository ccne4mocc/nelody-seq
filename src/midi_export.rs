//! Standard MIDI File export for the current melodic loop.
//!
//! Editor thread only; `process()` never builds or writes a file. One format-0 track with note
//! events and a tempo meta event is small enough to encode directly, so no MIDI crate is needed.

use crate::sequence::{Pattern, MAX_STEPS};
use crate::MelodyParams;

const PPQ: u16 = 480;
const CHANNEL: u8 = 0;

pub(crate) struct ExportSettings {
    /// Host tempo. Invalid or missing values fall back to 120 BPM.
    pub bpm: f64,
    /// Clock rate in steps per beat (4 = 1/16).
    pub steps_per_beat: f64,
    /// Exact loop length in steps. Fractional lengths wrap mid-cell.
    pub loop_steps: f64,
    pub gate: f64,
    pub swing: f64,
}

struct Event {
    tick: u32,
    /// Same-tick write order: 0 = note off, 1..=6 = the RPN messages in sequence, 7 = pitch bend,
    /// 8 = note on. A note off therefore cannot cut the incoming note, and the bend sensitivity is
    /// established before the first bend or note.
    order: u8,
    data: [u8; 3],
}

/// Channel-1 pitch-bend sensitivity (RPN 0), paired with the message that closes the RPN.
const RPN_ZERO: [(u8, u8); 6] = [
    (101, 0),
    (100, 0),
    (6, 0),
    (38, 0),
    (101, 127),
    (100, 127),
];

/// Encode one generated loop as a format-0 Standard MIDI File.
pub(crate) fn smf(pattern: &Pattern, p: &MelodyParams, settings: &ExportSettings) -> Vec<u8> {
    let steps = pattern.steps.clamp(1, MAX_STEPS);
    let spb = if settings.steps_per_beat.is_finite() && settings.steps_per_beat > 0.0 {
        settings.steps_per_beat
    } else {
        4.0
    };
    let step_len = 1.0 / spb;
    let loop_steps = if settings.loop_steps.is_finite() && settings.loop_steps > 0.0 {
        settings.loop_steps
    } else {
        steps as f64
    };
    let gate = settings.gate.clamp(0.02, 1.0);
    let swing = settings.swing.clamp(0.0, 0.5) * step_len;
    let to_tick = |beat: f64| (beat * PPQ as f64).round().clamp(0.0, u32::MAX as f64) as u32;
    let nts = p.nts.value();
    let bend_range = p.bend_range.value().clamp(1, 24) as f32 * 100.0;

    let mut events = Vec::new();
    if nts {
        // The file carries the same sensitivity the plugin announces over MIDI, so the notes do
        // not play back at a different microtonal offset than they sounded. The RPN needs its
        // four messages in order, so each one gets its own sort key.
        for (k, (cc, raw)) in RPN_ZERO.iter().copied().enumerate() {
            let raw = if cc == 6 { p.bend_range.value().clamp(1, 24) as u8 } else { raw };
            events.push(Event { tick: 0, order: 1 + k as u8, data: [0xB0 | CHANNEL, cc, raw] });
        }
    }
    for i in 0..steps {
        if !pattern.hit[i] { continue; }
        let onset = i as f64 * step_len + if i % 2 == 1 { swing } else { 0.0 };
        // A slide is held until the next onset; otherwise the gate knob decides.
        let hold = if pattern.slide[i] {
            crate::next_hit_after(&pattern.hit, i, steps)
                .map(|j| ((j + steps - i) % steps) as f64 + 0.5)
                .unwrap_or(gate)
        } else {
            gate
        };
        let velocity = (p.velocity.value() * pattern.acc[i]).clamp(0.0, 1.0);
        let velocity = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
        let (note, cents) = p.pitch_for(pattern, i);
        if nts {
            let bend = (crate::bend_value(cents, bend_range) * 16_383.0).round() as u16;
            events.push(Event {
                tick: to_tick(onset),
                order: 7,
                data: [0xE0 | CHANNEL, (bend & 0x7F) as u8, (bend >> 7) as u8],
            });
        }
        events.push(Event { tick: to_tick(onset), order: 8, data: [0x90 | CHANNEL, note, velocity] });
        events.push(Event { tick: to_tick(onset + hold * step_len), order: 0, data: [0x80 | CHANNEL, note, 0] });
    }
    let loop_end = to_tick(loop_steps * step_len);
    if nts {
        // The loop must not end carrying the last note's bend into the next iteration.
        events.push(Event { tick: loop_end, order: 7, data: [0xE0 | CHANNEL, 0x00, 0x40] });
    }
    events.sort_unstable_by(|a, b| {
        a.tick.cmp(&b.tick).then(a.order.cmp(&b.order)).then(a.data.cmp(&b.data))
    });

    let mut track = Vec::new();
    let bpm = if settings.bpm.is_finite() && settings.bpm > 0.0 { settings.bpm } else { 120.0 };
    let us_per_beat = (60_000_000.0 / bpm).round().clamp(1.0, 16_777_215.0) as u32;
    track.extend_from_slice(&[0, 0xFF, 0x51, 0x03]);
    track.extend_from_slice(&us_per_beat.to_be_bytes()[1..]);
    track.extend_from_slice(&[0, 0xFF, 0x58, 0x04, 4, 2, 24, 8]);

    let mut last_tick = 0;
    for event in &events {
        push_vlq(&mut track, event.tick - last_tick);
        track.extend_from_slice(&event.data);
        last_tick = event.tick;
    }
    let end = loop_end.max(last_tick);
    push_vlq(&mut track, end - last_tick);
    track.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    let mut out = Vec::with_capacity(14 + 8 + track.len());
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&PPQ.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

fn push_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut bytes = [0u8; 5];
    let mut at = 4;
    bytes[at] = (value & 0x7F) as u8;
    value >>= 7;
    while value > 0 {
        at -= 1;
        bytes[at] = ((value & 0x7F) as u8) | 0x80;
        value >>= 7;
    }
    out.extend_from_slice(&bytes[at..]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::prelude::*;

    fn setup(hits: &[usize]) -> (Pattern, MelodyParams, ExportSettings) {
        let mut pattern = Pattern { steps: 4, ..Pattern::default() };
        for &i in hits {
            pattern.hit[i] = true;
            pattern.acc[i] = 1.0;
            pattern.u[i] = 0.0;
        }
        let p = MelodyParams {
            nts: BoolParam::new("Microtonal Pitch Bend (NTS)", false),
            ..MelodyParams::default()
        };
        let settings = ExportSettings { bpm: 120.0, steps_per_beat: 4.0, loop_steps: 4.0, gate: 0.5, swing: 0.0 };
        (pattern, p, settings)
    }

    #[test]
    fn variable_length_quantities_match_the_midi_encoding() {
        for (value, expected) in [
            (0u32, &[0x00][..]),
            (0x7F, &[0x7F][..]),
            (0x80, &[0x81, 0x00][..]),
            (0x2000, &[0xC0, 0x00][..]),
            (0x0FFF, &[0x9F, 0x7F][..]),
        ] {
            let mut out = Vec::new();
            push_vlq(&mut out, value);
            assert_eq!(out, expected, "vlq({value})");
        }
    }

    #[test]
    fn one_loop_writes_note_pairs_on_channel_one() {
        let (pattern, p, settings) = setup(&[0, 1]);
        let bytes = smf(&pattern, &p, &settings);
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(&bytes[14..18], b"MTrk");
        let track = &bytes[22..];
        let events = channel_events(track);
        assert_eq!(events.len(), 4);
        assert_eq!((events[0].0, events[0].1, events[0].2), (0, 0x90, 48));
        assert_eq!((events[1].0, events[1].1), (60, 0x80));
        assert_eq!((events[2].0, events[2].1, events[2].2), (120, 0x90, 48));
        assert!(track.ends_with(&[0xFF, 0x2F, 0x00]));
    }

    #[test]
    fn fractional_loop_uses_the_exact_loop_length() {
        let (pattern, p, mut settings) = setup(&[]);
        settings.loop_steps = 4.5;
        let bytes = smf(&pattern, &p, &settings);
        assert!(bytes[22..].ends_with(&[0x84, 0x1C, 0xFF, 0x2F, 0x00]));
    }

    fn channel_events(track: &[u8]) -> Vec<(u32, u8, u8)> {
        let mut out = Vec::new();
        let mut at = 0;
        let mut tick = 0u32;
        while at < track.len() {
            let (delta, used) = read_vlq(&track[at..]);
            at += used;
            tick += delta;
            match track[at] {
                0xFF => {
                    at += 2;
                    let (len, used) = read_vlq(&track[at..]);
                    at += used + len as usize;
                }
                0x80..=0x9F => {
                    out.push((tick, track[at], track[at + 1]));
                    at += 3;
                }
                other => panic!("unexpected event byte {other:#04x} at {at}"),
            }
        }
        out
    }

    /// Every channel event with both data bytes, for the bend/RPN tests.
    fn all_events(track: &[u8]) -> Vec<(u32, [u8; 3])> {
        let mut out = Vec::new();
        let mut at = 0;
        let mut tick = 0u32;
        while at < track.len() {
            let (delta, used) = read_vlq(&track[at..]);
            at += used;
            tick += delta;
            match track[at] {
                0xFF => {
                    at += 2;
                    let (len, used) = read_vlq(&track[at..]);
                    at += used + len as usize;
                }
                0x80..=0xEF => {
                    out.push((tick, [track[at], track[at + 1], track[at + 2]]));
                    at += 3;
                }
                other => panic!("unexpected event byte {other:#04x} at {at}"),
            }
        }
        out
    }

    fn detuned_setup(range: i32, root_cents: f32) -> (Pattern, MelodyParams, ExportSettings) {
        let (mut pattern, _, settings) = setup(&[0, 1]);
        pattern.u[0] = 0.0;
        pattern.u[1] = 0.0;
        let p = MelodyParams {
            nts: BoolParam::new("Microtonal Pitch Bend (NTS)", true),
            bend_range: IntParam::new("Pitch Bend Range", range, IntRange::Linear { min: 1, max: 24 }),
            root_cents: FloatParam::new("Root Cents", root_cents, FloatRange::Linear { min: -50.0, max: 50.0 }),
            ..MelodyParams::default()
        };
        (pattern, p, settings)
    }

    #[test]
    fn nts_announces_its_bend_range_before_any_bend_or_note() {
        let (pattern, p, settings) = detuned_setup(2, 40.0);
        let bytes = smf(&pattern, &p, &settings);
        let events = all_events(&bytes[22..]);
        // RPN 0: select, semitones = range, cents = 0, then deselect.
        let rpn: Vec<(u32, [u8; 3])> = events.iter().copied().take(6).collect();
        assert_eq!(rpn.iter().map(|(_, e)| (e[1], e[2])).collect::<Vec<_>>(), vec![
            (101, 0), (100, 0), (6, 2), (38, 0), (101, 127), (100, 127)
        ]);
        assert!(rpn.iter().all(|(tick, e)| *tick == 0 && e[0] == 0xB0));
        // The first detuned note is preceded by its own bend at the same tick.
        let bend = events.iter().find(|(_, e)| e[0] == 0xE0).expect("no pitch bend written");
        assert_eq!(bend.0, 0);
        // +40 c over a ±2 st range is 0.6 of the 14-bit span.
        let value = (u16::from(bend.1[1]) | (u16::from(bend.1[2]) << 7)) as f32 / 16_383.0;
        assert!((value - 0.6).abs() < 1e-3, "bend value {value}");
        // The loop ends on a centred bend, after the last note off.
        let last = events.last().copied().unwrap();
        assert_eq!(last.1, [0xE0, 0x00, 0x40]);
        assert_eq!(last.0, 480, "loop end in ticks");
        let note_off_at = events.iter().rev().find(|(_, e)| e[0] == 0x80).unwrap().0;
        assert!(note_off_at <= last.0);
    }

    #[test]
    fn bend_range_changes_the_exported_bend_depth() {
        let (pattern, wide, settings) = detuned_setup(2, 40.0);
        let (_, narrow, _) = detuned_setup(1, 40.0);
        let value = |p: &MelodyParams| {
            let bytes = smf(&pattern, p, &settings);
            let bend = all_events(&bytes[22..]).into_iter().find(|(_, e)| e[0] == 0xE0).unwrap();
            (u16::from(bend.1[1]) | (u16::from(bend.1[2]) << 7)) as f32 / 16_383.0
        };
        let (a, b) = (value(&wide), value(&narrow));
        assert!((a - 0.6).abs() < 1e-3, "±2 st: {a}");
        assert!((b - 0.7).abs() < 1e-3, "±1 st: {b}");
    }

    #[test]
    fn nts_off_writes_no_bend_or_rpn() {
        let (pattern, p, settings) = setup(&[0, 1]);
        let bytes = smf(&pattern, &p, &settings);
        let events = all_events(&bytes[22..]);
        assert!(events.iter().all(|(_, e)| e[0] != 0xE0 && e[0] != 0xB0), "unexpected bend/CC");
    }

    fn read_vlq(data: &[u8]) -> (u32, usize) {
        let mut value = 0u32;
        for (i, &byte) in data.iter().enumerate() {
            value = (value << 7) | (byte & 0x7F) as u32;
            if byte & 0x80 == 0 {
                return (value, i + 1);
            }
        }
        panic!("unterminated vlq");
    }
}
