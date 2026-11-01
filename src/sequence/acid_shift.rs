use super::{acid_commit, AcidLine, GenParams, Pattern, Rng};

const SHIFT_DEG: [usize; 8] = [0, 0, 4, 2, 0, 3, 6, 4];

/// Acid Shift Register: Turing-machine style ring of `Pulses` bits (4..16).
/// Every step the ring shifts one place and the feedback bit flips with probability Phase * 0.5.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 9);
    let len = p.pulses.clamp(4, 16);
    let mask = (1u32 << len) - 1;
    let mut reg = (rng.next_u64() as u32 & mask) | 1;
    let flip = p.phase.clamp(0.0, 1.0) * 0.5;
    let field = |reg: u32, lo: usize, bits: usize| -> u32 {
        (0..bits).fold(0u32, |a, b| a | (((reg >> ((lo + b) % len)) & 1) << b))
    };
    let (ta, ts, to) = (
        (p.accent * 16.0).round() as u32,
        (p.slide * 16.0).round() as u32,
        (p.octave * 8.0).round() as u32,
    );
    let mut line = AcidLine::new();
    for i in 0..n {
        let fb = (reg >> (len - 1)) & 1;
        let fb = if rng.unit() < flip { fb ^ 1 } else { fb };
        reg = ((reg << 1) | fb) & mask;
        out.lsym[i] = (reg & 0xFF) as u8;
        line.hit[i] = field(reg, 3, 2) != 0;
        line.slot[i] = SHIFT_DEG[field(reg, 0, 3) as usize];
        line.acc[i] = field(reg, 5, 4) < ta;
        line.sld[i] = field(reg, 9, 4) < ts;
        line.oct[i] = (field(reg, 13, 3) < to) as i8;
    }
    if !line.hit[..n].iter().any(|&h| h) {
        line.hit[0] = true;
    }
    acid_commit(&mut out, n, line);
    out
}
