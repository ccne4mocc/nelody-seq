use super::{frac, top_k, GenParams, Pattern, Rng, MAX_STEPS};

pub(crate) const MODULI: [usize; 6] = [2, 3, 5, 7, 11, 13];

/// The three classes the sieve is built from: `[union, union, subtract]`, each `(modulus, residue)`.
pub(crate) fn classes(p: &GenParams) -> [(usize, usize); 3] {
    let rule = p.ca_rule as usize;
    let m1 = MODULI[rule % MODULI.len()];
    let m2 = MODULI[(rule / MODULI.len()) % MODULI.len()];
    let m3 = MODULI[(rule / (MODULI.len() * MODULI.len()) + 3) % MODULI.len()];
    let shift = (p.phase.clamp(0.0, 1.0) * MODULI.len() as f32).round() as usize + p.variation as usize;
    let r1 = (1 + shift) % m1;
    let r2 = (2 + shift) % m2;
    let r3 = (3 + shift + (p.seed.clamp(0.0, 1.0) * 97.0) as usize) % m3;
    [(m1, r1), (m2, r2), (m3, r3)]
}

pub(crate) fn class_contains(step: usize, class: (usize, usize)) -> bool {
    class.0 > 0 && step % class.0 == class.1
}

fn union_terms(p: &GenParams) -> usize {
    if p.pulses >= 2 { 2 } else { 1 }
}

fn is_member(step: usize, cls: &[(usize, usize); 3], terms: usize) -> bool {
    (0..terms).any(|k| class_contains(step, cls[k])) && !class_contains(step, cls[2])
}

/// Xenakis sieve: `M = (m1,r1) U (m2,r2) \ (m3,r3)`.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, slotted: true, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let cls = classes(p);
    let terms = union_terms(p);
    out.sieve_cls = cls;
    let d = p.density.clamp(0.0, 1.0);
    let mut score = [0.0f32; MAX_STEPS];
    let mut members = 0usize;
    let phase = frac(p.seed as f64 + p.phase as f64);
    let shift = (phase * 7.0).round() as usize;

    for i in 0..n {
        let c0 = class_contains(i, cls[0]);
        let c1 = terms >= 2 && class_contains(i, cls[1]);
        let deg = ((i / cls[0].0.max(1)) + if c1 { 3 } else { 0 }) % 7;
        // Explicit slot keeps slot 0 on the tonic in any tuning.
        let slot = ((deg + shift) % 7) as u8;
        out.slots[i] = slot;
        out.u[i] = (slot as f32 + 0.5) / 7.0;
        if !is_member(i, &cls, terms) {
            continue;
        }
        members += 1;
        let depth = usize::from(c0) + usize::from(c1);
        let jitter = Rng::new(p.seed, p.variation, 0x51E7_0000 + i as u64).unit() * 0.15;
        score[i] = 1.0 + 0.25 * depth as f32 + jitter;
    }
    let candidates = if members > 0 { members } else { n };
    if members == 0 {
        for i in 0..n {
            score[i] = 1.0 - i as f32 / n as f32;
        }
    }
    let wanted = ((candidates as f32 * d).round() as usize).max(1).min(candidates);
    top_k(&score, n, wanted, &mut out.hit);
    for i in 0..n {
        if out.hit[i] {
            out.acc[i] = (0.6 + 0.25 * score[i]).clamp(0.05, 1.0);
        }
    }
    out
}
