//! Pure tuning math: scale tables in cents, degree -> (MIDI note, detune cents). No allocation,
//! no global state. The `Tuning` enum is a host parameter, so it carries nice-plug's `Enum` derive.

use nice_plug::prelude::Enum;

/// Highest equal-division count offered by the `edo` parameter.
pub const MAX_EDO: u32 = 72;

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Tuning {
    #[id = "major"]
    #[name = "Major"]
    Major,
    #[id = "minor"]
    #[name = "Minor"]
    Minor,
    #[id = "dorian"]
    #[name = "Dorian"]
    Dorian,
    #[id = "pent_min"]
    #[name = "Pent Minor"]
    PentMinor,
    #[id = "pent_maj"]
    #[name = "Pent Major"]
    PentMajor,
    #[id = "12tet"]
    #[name = "12-TET Chromatic"]
    Chromatic,
    #[id = "ji5"]
    #[name = "Just 5-limit"]
    JustFiveLimit,
    #[id = "pyth"]
    #[name = "Pythagorean"]
    Pythagorean,
    #[id = "meantone"]
    #[name = "Meantone 1/4"]
    Meantone,
    #[id = "harmonic"]
    #[name = "Harmonic 8:16"]
    Harmonic,
    #[id = "slendro"]
    #[name = "Slendro"]
    Slendro,
    /// No table: `edo` equal divisions per octave.
    #[id = "edo"]
    #[name = "Equal (EDO)"]
    Edo,
}

impl Tuning {
    /// Pitch classes of one octave, in cents above the root, ascending from 0.
    /// `Edo` has no table; its step size comes from the `edo` parameter instead.
    pub fn cents(self) -> &'static [f32] {
        match self {
            Tuning::Major => &[0.0, 200.0, 400.0, 500.0, 700.0, 900.0, 1100.0],
            Tuning::Minor => &[0.0, 200.0, 300.0, 500.0, 700.0, 800.0, 1000.0],
            Tuning::Dorian => &[0.0, 200.0, 300.0, 500.0, 700.0, 900.0, 1000.0],
            Tuning::PentMinor => &[0.0, 300.0, 500.0, 700.0, 1000.0],
            Tuning::PentMajor => &[0.0, 200.0, 400.0, 700.0, 900.0],
            Tuning::Chromatic => &[0.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1100.0],
            // 1/1 9/8 5/4 4/3 3/2 5/3 15/8.
            Tuning::JustFiveLimit => &[0.0, 203.91, 386.31, 498.04, 701.96, 884.36, 1088.27],
            // 3/2 stacked six fifths either side of the tonic.
            Tuning::Pythagorean => &[0.0, 90.22, 203.91, 294.13, 407.82, 498.04, 588.27, 701.96, 792.18, 905.87, 996.09, 1109.78],
            // 3/2 narrowed by a quarter syntonic comma (81/80), which gives a 5/4 major third.
            Tuning::Meantone => &[0.0, 117.11, 193.16, 310.26, 386.31, 503.42, 620.53, 696.58, 813.69, 889.74, 1006.84, 1082.89],
            // Harmonics 8..15 over the fundamental 8: 8/8 9/8 10/8 11/8 12/8 13/8 14/8 15/8. The table
            // used to start at 9/8 and end on 1200c, so the tonic itself was missing from every octave
            // (and acid slot 0, the root-biased one, sounded the 9/8 above it).
            Tuning::Harmonic => &[0.0, 203.91, 386.31, 551.32, 701.96, 840.53, 968.83, 1088.27],
            // Near-even pentatonic, ~240c per tone.
            Tuning::Slendro => &[0.0, 240.0, 480.0, 720.0, 960.0],
            Tuning::Edo => &[],
        }
    }

    /// Degrees reachable within `octaves` octaves.
    pub fn degrees(self, octaves: u32, edo: u32) -> usize {
        let octaves = octaves.max(1) as usize;
        match self {
            Tuning::Edo => edo.clamp(1, MAX_EDO) as usize * octaves,
            _ => self.cents().len() * octaves,
        }
    }

    /// Cents above the root for degree `deg` (already clamped to `degrees()`).
    pub fn degree_cents(self, deg: usize, edo: u32) -> f32 {
        match self {
            Tuning::Edo => {
                let e = edo.clamp(1, MAX_EDO) as usize;
                // The octave term was missing: with `octaves` > 1 every upper octave replayed the first.
                (deg % e) as f32 * 1200.0 / e as f32 + 1200.0 * (deg / e) as f32
            }
            _ => {
                let n = self.cents().len();
                self.cents()[deg % n] + 1200.0 * (deg / n) as f32
            }
        }
    }

    /// Quantise the pitch contour `u` (0..1) to a semitone offset from the root note plus the
    /// detune in cents that did not fit on that note. Detune is in -50..=50.
    pub fn pitch(self, u: f32, octaves: u32, edo: u32, root_cents: f32) -> (i32, f32) {
        let span = self.degrees(octaves, edo).max(1);
        let deg = ((u.clamp(0.0, 1.0) * span as f32) as usize).min(span - 1);
        let cents = self.degree_cents(deg, edo) + root_cents;
        let semis = (cents / 100.0).round();
        (semis as i32, cents - semis * 100.0)
    }

    /// Like `pitch`, but `slot` names a scale degree inside one octave: slot 0 is always the tonic
    /// and `oct` shifts the whole degree by octaves. The slot-based generators use this so their
    /// contour cannot fall between degree bins at two or more octaves.
    pub fn pitch_slot(self, slot: usize, oct: i32, edo: u32, root_cents: f32) -> (i32, f32) {
        let span = self.degrees(1, edo).max(1);
        let cents = self.degree_cents(slot % span, edo) + 1200.0 * oct as f32 + root_cents;
        let semis = (cents / 100.0).round();
        (semis as i32, cents - semis * 100.0)
    }

    /// Like `pitch`, but `u` (0..1) picks a degree inside ONE octave and `oct` then shifts it by whole
    /// octaves. The acid generators use this so a line keeps its shape in any tuning.
    pub fn pitch_in_octave(self, u: f32, oct: i32, edo: u32, root_cents: f32) -> (i32, f32) {
        let span = self.degrees(1, edo).max(1);
        let slot = ((u.clamp(0.0, 1.0) * span as f32) as usize).min(span - 1);
        self.pitch_slot(slot, oct, edo, root_cents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tonal_scales_still_land_on_plain_12tet_notes() {
        let semitones: &[(Tuning, &[i32])] = &[
            (Tuning::Major, &[0, 2, 4, 5, 7, 9, 11]),
            (Tuning::Minor, &[0, 2, 3, 5, 7, 8, 10]),
            (Tuning::Dorian, &[0, 2, 3, 5, 7, 9, 10]),
            (Tuning::PentMinor, &[0, 3, 5, 7, 10]),
            (Tuning::PentMajor, &[0, 2, 4, 7, 9]),
            (Tuning::Chromatic, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
        ];
        for (t, semis) in semitones {
            assert_eq!(t.degrees(1, 12), semis.len());
            for (deg, &s) in semis.iter().enumerate() {
                assert_eq!(t.degree_cents(deg, 12), s as f32 * 100.0, "{t:?} degree {deg}");
            }
        }
        let n = Tuning::Major.degrees(2, 12);
        assert_eq!(n, 14);
        assert_eq!(Tuning::Major.pitch(0.0, 1, 12, 0.0), (0, 0.0));
        assert_eq!(Tuning::Major.pitch(3.0 / 7.0, 1, 12, 0.0), (5, 0.0));
        assert_eq!(Tuning::Major.pitch(0.999, 1, 12, 0.0), (11, 0.0));
        assert_eq!(Tuning::Major.pitch(1.0, 2, 12, 0.0), (23, 0.0));
    }

    #[test]
    fn xenoharmonic_tunings_detune_within_half_a_semitone() {
        let near = |t: Tuning, u: f32, semis: i32, cents: f32| {
            let (got_semis, got_cents) = t.pitch(u, 1, 12, 0.0);
            assert_eq!(got_semis, semis, "{t:?} semitones at u={u}");
            assert!((got_cents - cents).abs() < 0.01, "{t:?} cents at u={u}: {got_cents} vs {cents}");
        };
        near(Tuning::Pythagorean, 4.0 / 12.0, 4, 7.82);
        near(Tuning::Pythagorean, 7.0 / 12.0, 7, 1.96);
        near(Tuning::Meantone, 3.0 / 12.0, 3, 10.26);
        near(Tuning::JustFiveLimit, 2.0 / 7.0, 4, -13.69);
        near(Tuning::JustFiveLimit, 4.0 / 7.0, 7, 1.96);
        near(Tuning::Harmonic, 3.0 / 8.0, 6, -48.68);
        for t in [Tuning::Pythagorean, Tuning::Meantone, Tuning::JustFiveLimit, Tuning::Harmonic, Tuning::Slendro] {
            let cents = t.cents();
            assert!(cents.windows(2).all(|w| w[0] < w[1]), "{t:?} table is not ascending");
            assert!(cents.iter().all(|&c| (0.0..=1200.0).contains(&c)), "{t:?} table leaves the octave");
        }
    }

    #[test]
    fn edo_provides_its_own_steps() {
        assert_eq!(Tuning::Edo.cents().len(), 0);
        assert_eq!(Tuning::Edo.degrees(1, 12), 12);
        assert_eq!(Tuning::Edo.degrees(2, 19), 38);
        assert_eq!(Tuning::Edo.pitch(1.0 / 19.0, 1, 19, 0.0), (1, -36.842106));
        assert_eq!(Tuning::Edo.pitch(0.0, 1, 19, 0.0), (0, 0.0));
        assert_eq!(Tuning::Edo.degrees(1, 0), 1);
        assert!(Tuning::Edo.degrees(1, 999) <= MAX_EDO as usize);
    }

    #[test]
    fn pitch_in_octave_stays_inside_one_octave_until_shifted() {
        for k in 0..7 {
            let u = (k as f32 + 0.5) / 7.0;
            let (s, c) = Tuning::Minor.pitch_in_octave(u, 0, 12, 0.0);
            assert_eq!((s, c), ([0, 2, 3, 5, 7, 8, 10][k], 0.0), "slot {k}");
            assert_eq!(Tuning::Minor.pitch_in_octave(u, 1, 12, 0.0).0, s + 12);
            assert_eq!(Tuning::Minor.pitch_in_octave(u, -1, 12, 0.0).0, s - 12);
        }
        for t in [Tuning::PentMinor, Tuning::Chromatic, Tuning::Slendro, Tuning::Edo] {
            let notes: Vec<f32> = (0..7)
                .map(|k| {
                    let (s, c) = t.pitch_in_octave((k as f32 + 0.5) / 7.0, 0, 19, 0.0);
                    s as f32 * 100.0 + c
                })
                .collect();
            assert!(notes.windows(2).all(|w| w[0] <= w[1]), "{t:?} {notes:?}");
            assert!(notes.iter().all(|&c| (-60.0..1260.0).contains(&c)), "{t:?} {notes:?}");
        }
        assert_eq!(Tuning::Minor.pitch_in_octave(0.01, 0, 12, 25.0), (0, 25.0));
        let (_, c) = Tuning::Pythagorean.pitch_in_octave(0.4, 1, 12, 0.0);
        assert!(c.abs() <= 50.0);
    }

    #[test]
    fn pitch_slot_keeps_slot_zero_on_the_tonic() {
        for t in [Tuning::Major, Tuning::Minor, Tuning::Dorian, Tuning::JustFiveLimit,
                  Tuning::Pythagorean, Tuning::Meantone, Tuning::Harmonic] {
            for k in 0..7 {
                let (s, c) = t.pitch_slot(k, 0, 12, 0.0);
                let want = t.degree_cents(k, 12);
                assert!((s as f32 * 100.0 + c - want).abs() < 1e-3, "{t:?} slot {k}");
            }
        }
        assert_eq!(Tuning::Major.pitch_slot(0, 2, 12, 0.0), (24, 0.0));
        assert_eq!(Tuning::Major.pitch_slot(7, 0, 12, 0.0), (0, 0.0), "slots wrap inside the octave");
        for edo in [5u32, 12, 19, 24, 31, 53, 72] {
            assert_eq!(Tuning::Edo.pitch_slot(0, 0, edo, 0.0), (0, 0.0), "edo {edo}");
            assert_eq!(Tuning::Edo.pitch_slot(1, 0, edo, 0.0).0, (1200.0 / edo as f32 / 100.0).round() as i32);
        }
    }

    #[test]
    fn edo_upper_octaves_climb() {
        // 19-EDO over two octaves: degree 19 is the octave, not a replay of degree 0.
        assert_eq!(Tuning::Edo.degree_cents(19, 19), 1200.0);
        assert!((Tuning::Edo.degree_cents(28, 19) - (9.0 * 1200.0 / 19.0 + 1200.0)).abs() < 1e-3);
        let (lo, _) = Tuning::Edo.pitch(0.01, 2, 19, 0.0);
        let (hi, _) = Tuning::Edo.pitch(0.99, 2, 19, 0.0);
        assert!(hi - lo >= 22, "two octaves must span ~24 semitones, got {}", hi - lo);
        for edo in [5u32, 12, 19, 24, 31, 53, 72] {
            let n = Tuning::Edo.degrees(3, edo);
            assert!(Tuning::Edo.degree_cents(n - 1, edo) > 2400.0, "edo {edo}");
        }
    }

    #[test]
    fn every_table_starts_on_the_tonic() {
        for t in [Tuning::Major, Tuning::Minor, Tuning::Dorian, Tuning::PentMinor, Tuning::PentMajor,
                  Tuning::Chromatic, Tuning::JustFiveLimit, Tuning::Pythagorean, Tuning::Meantone,
                  Tuning::Harmonic, Tuning::Slendro] {
            assert_eq!(t.cents()[0], 0.0, "{t:?} has no tonic");
            assert!(*t.cents().last().unwrap() < 1200.0, "{t:?} repeats the octave inside the table");
        }
    }

    #[test]
    fn detune_is_always_inside_half_a_semitone_and_rebuilds_the_target() {
        // 24-EDO quarter tones and +-50 root cents sit exactly on the rounding boundary.
        for (t, edo) in [(Tuning::Edo, 24u32), (Tuning::Edo, 48), (Tuning::Edo, 72), (Tuning::Major, 12)] {
            for rc in [-50.0f32, -49.0, 0.0, 49.0, 50.0] {
                let span = t.degrees(2, edo);
                for deg in 0..span {
                    let (s, c) = t.pitch((deg as f32 + 0.5) / span as f32, 2, edo, rc);
                    assert!(c.abs() <= 50.0 + 1e-3, "{t:?} {edo} deg {deg} rc {rc}: detune {c}");
                    let want = t.degree_cents(deg, edo) + rc;
                    assert!((s as f32 * 100.0 + c - want).abs() < 1e-2, "{t:?} {edo} deg {deg} rc {rc}");
                }
            }
        }
    }

    #[test]
    fn root_cents_shifts_every_degree() {
        assert_eq!(Tuning::Major.pitch(0.0, 1, 12, 25.0), (0, 25.0));
        assert_eq!(Tuning::Major.pitch(0.0, 1, 12, -25.0), (0, -25.0));
        assert_eq!(Tuning::Major.pitch(3.0 / 7.0, 1, 12, 60.0), (6, -40.0));
        assert_eq!(Tuning::Edo.pitch(0.0, 1, 12, 0.0), (0, 0.0));
    }
}
