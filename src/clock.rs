//! Timing math derived from the sequencer mode.
//!
//! The host gives us a position in quarter notes. The plug-in never asks the
//! user for a meter: each mode declares its own step unit, and the generated
//! pattern length implies the meter. Everything here is rational so unusual
//! divisions stay exact.

/// Small reduced fraction used by tuplet/ratio timing. The denominator is always positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frac {
    pub num: u32,
    pub den: u32,
}

impl Frac {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    pub fn reduced(num: u32, den: u32) -> Self {
        let den = den.max(1);
        let g = gcd_u32(num.max(1), den);
        Self { num: num.max(1) / g, den: den / g }
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn inverse(self) -> Self {
        Self::reduced(self.den, self.num)
    }
}

pub fn gcd_u32(a: u32, b: u32) -> u32 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

pub fn gcd_u64(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

pub fn lcm_capped(lane_steps: &[u16], cap: u64) -> (u64, bool) {
    let mut out = 1u64;
    for &n in lane_steps {
        let n = n.max(1) as u64;
        let g = gcd_u64(out, n);
        out = (out / g).saturating_mul(n);
        if out > cap {
            return (out, true);
        }
    }
    (out, false)
}

/// A step subdivision owned by a mode. `steps_per_quarter` is exact: 1/16 is
/// 4, 1/8T is 3, 1/16T is 6. `note_den` is the note value used by a MIDI time
/// signature when the unit is a plain power-of-two note (1/16 -> 16); triplets
/// and dotted units have no exact signature denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepUnit {
    Quarter,
    Eighth,
    EighthTriplet,
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
}

impl StepUnit {
    pub fn steps_per_quarter(self) -> f64 {
        match self {
            Self::Quarter => 1.0,
            Self::Eighth => 2.0,
            Self::EighthTriplet => 3.0,
            Self::Sixteenth => 4.0,
            Self::SixteenthTriplet => 6.0,
            Self::ThirtySecond => 8.0,
        }
    }

    pub fn note_den(self) -> Option<u32> {
        match self {
            Self::Quarter => Some(4),
            Self::Eighth => Some(8),
            Self::Sixteenth => Some(16),
            Self::ThirtySecond => Some(32),
            Self::EighthTriplet | Self::SixteenthTriplet => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::EighthTriplet => "1/8T",
            Self::Sixteenth => "1/16",
            Self::SixteenthTriplet => "1/16T",
            Self::ThirtySecond => "1/32",
        }
    }
}

/// A bar grouping implied by a pattern. There is no host or user meter here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    pub num: u32,
    pub den: u32,
}

impl Meter {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

}

/// Implied meter for a repeating pattern of `steps` cells at `unit`.
///
/// Halve long even numerators until the signature is compact (16/16 -> 8/8 ->
/// 4/4) but keep useful odd bars such as 13/16, 12/8 and 15/8. Multi-bar or
/// unsupported units return `None`; callers show the loop length instead of
/// inventing a signature.
pub fn meter_from_steps(steps: u32, unit: StepUnit) -> Option<Meter> {
    let den = unit.note_den()?;
    if steps == 0 {
        return None;
    }
    let mut n = steps;
    let mut d = den;
    while n > 16 && d > 4 && n % 2 == 0 {
        n /= 2;
        d /= 2;
    }
    if n == d && d > 4 {
        n = 4;
        d = 4;
    }
    if (2..=16).contains(&n) {
        Some(Meter::new(n, d))
    } else {
        None
    }
}

/// Short legend label: `4/4 · step 1/16` or `loop 81 · step 1/16`.
pub fn meter_label(meter: Option<Meter>, unit: StepUnit, loop_steps: u32) -> String {
    match meter {
        Some(m) => format!("{}/{} · step {}", m.num, m.den, unit.label()),
        None => format!("loop {loop_steps} · step {}", unit.label()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_and_lcm_are_exact() {
        assert_eq!(Frac::reduced(14, 8), Frac::new(7, 4));
        assert!((Frac::new(3, 2).as_f64() - 1.5).abs() < 1e-12);
        assert_eq!(Frac::new(7, 4).inverse(), Frac::new(4, 7));
        assert_eq!(gcd_u32(13, 7), 1);
        assert_eq!(lcm_capped(&[13, 7], 512), (91, false));
        assert_eq!(lcm_capped(&[511, 509], 512).1, true);
    }

    #[test]
    fn implied_meter_uses_the_mode_unit_and_loop() {
        assert_eq!(meter_from_steps(16, StepUnit::Sixteenth), Some(Meter::new(4, 4)));
        assert_eq!(meter_from_steps(13, StepUnit::Sixteenth), Some(Meter::new(13, 16)));
        assert_eq!(meter_from_steps(12, StepUnit::Eighth), Some(Meter::new(12, 8)));
        assert_eq!(meter_from_steps(15, StepUnit::Eighth), Some(Meter::new(15, 8)));
        assert_eq!(meter_from_steps(6, StepUnit::Eighth), Some(Meter::new(6, 8)));
        assert_eq!(meter_from_steps(81, StepUnit::Sixteenth), None);
        assert_eq!(meter_from_steps(16, StepUnit::SixteenthTriplet), None);
    }

    #[test]
    fn labels_name_the_derived_timing() {
        assert_eq!(
            meter_label(Some(Meter::new(13, 16)), StepUnit::Sixteenth, 13),
            "13/16 · step 1/16"
        );
        assert_eq!(meter_label(None, StepUnit::Sixteenth, 81), "loop 81 · step 1/16");
    }
}
