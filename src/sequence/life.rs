use super::{GenParams, Pattern, Rng};

/// Fixed 16 × 8 torus. Keeping the grid small bounds the worst case to `steps × 128` updates and
/// gives the panel a legible population wave instead of a wall of cells.
pub(crate) const WIDTH: usize = 16;
pub(crate) const HEIGHT: usize = 8;

const LIFE_BIRTH: u16 = 1 << 3;
const LIFE_SURVIVE: u16 = (1 << 2) | (1 << 3);

fn rule_masks(rule: u8) -> (u16, u16) {
    match rule % 5 {
        0 => (LIFE_BIRTH, LIFE_SURVIVE),
        // HighLife: B36/S23.
        1 => ((1 << 3) | (1 << 6), LIFE_SURVIVE),
        // Seeds: B2/S.
        2 => (1 << 2, 0),
        // Day & Night: B3678/S34678.
        3 => (
            (1 << 3) | (1 << 6) | (1 << 7) | (1 << 8),
            (1 << 3) | (1 << 4) | (1 << 6) | (1 << 7) | (1 << 8),
        ),
        // A dense, high-survival rule that keeps the grid active.
        _ => (1 << 3, (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5)),
    }
}

type Grid = [[bool; WIDTH]; HEIGHT];

fn seed(p: &GenParams, rng: &mut Rng) -> Grid {
    let mut grid = [[false; WIDTH]; HEIGHT];
    match p.symmetry {
        1 => {
            // Centre block.
            for row in 2..6 {
                for col in 6..10 {
                    grid[row][col] = true;
                }
            }
        }
        2 => {
            // Vertical stripes.
            for row in 0..HEIGHT {
                for col in (0..WIDTH).step_by(2) {
                    grid[row][col] = true;
                }
            }
        }
        3 => {
            // Sparse random with a guaranteed live cell in every row.
            for row in 0..HEIGHT {
                for col in 0..WIDTH {
                    grid[row][col] = rng.unit() < p.density.clamp(0.0, 1.0) * 0.35;
                }
            }
        }
        _ => {
            for row in 0..HEIGHT {
                for col in 0..WIDTH {
                    grid[row][col] = rng.unit() < p.density.clamp(0.0, 1.0) * 0.6;
                }
            }
        }
    }
    // A diagonal seed guarantees every row has something to read on the first generation.
    for row in 0..HEIGHT {
        grid[row][row % WIDTH] = true;
    }
    grid
}

fn step(grid: &Grid, birth: u16, survive: u16) -> Grid {
    let mut next = [[false; WIDTH]; HEIGHT];
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let mut neighbours = 0u16;
            for dr in [-1i32, 0, 1] {
                for dc in [-1i32, 0, 1] {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let r = (row as i32 + dr).rem_euclid(HEIGHT as i32) as usize;
                    let c = (col as i32 + dc).rem_euclid(WIDTH as i32) as usize;
                    neighbours += grid[r][c] as u16;
                }
            }
            let mask = 1u16 << neighbours;
            next[row][col] = if grid[row][col] {
                survive & mask != 0
            } else {
                birth & mask != 0
            };
        }
    }
    // A dead torus is a dead line; inject one cell so the next generation can recover.
    if !next.iter().any(|row| row.iter().any(|cell| *cell)) {
        next[0][0] = true;
    }
    next
}

/// A Life-like torus read as one melodic line: `pulses` is the warm-up generation count,
/// `ca_rule` selects the rule family and `symmetry` the seed shape. Each step reads one column;
/// the live rows become a scale-degree slot and the population drives onset probability/accent.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let mut rng = Rng::new(p.seed, p.variation, 0x11FE_0000);
    let (birth, survive) = rule_masks(p.ca_rule);
    let mut grid = seed(p, &mut rng);
    let warmup = p.pulses.min(64);
    for _ in 0..warmup {
        grid = step(&grid, birth, survive);
    }

    let mut any = false;
    for i in 0..p.steps {
        let col = i % WIDTH;
        let live_rows: u8 = (0..HEIGHT).fold(0u8, |mask, row| {
            mask | ((grid[row][col] as u8) << row)
        });
        let population = live_rows.count_ones() as f32 / HEIGHT as f32;
        // Rotate the 4-bit window by the step so adjacent steps do not read the same rows.
        let turn = (i % 4) as u32;
        let code = ((live_rows >> turn) | (live_rows << (4 - turn))) & 0x0F;
        out.slotted = true;
        out.slots[i] = code % 8;
        out.u[i] = (out.slots[i] as f32 + 0.5) / 8.0;
        out.orbit[i] = population;
        let gate = (0.3 + 0.7 * p.density.clamp(0.0, 1.0)) * (0.45 + 0.55 * population);
        let local = Rng::new(p.seed, p.variation, 0x11FE_1000 + i as u64).unit();
        let hit = p.steps == 1
            || (population > 0.0 && (p.density >= 0.99 || local < gate));
        out.hit[i] = hit;
        if hit {
            any = true;
            out.acc[i] = 0.65 + 0.35 * population;
        }
        grid = step(&grid, birth, survive);
    }
    if !any {
        out.hit[0] = true;
        out.slots[0] = 0;
        out.u[0] = 0.5;
        out.acc[0] = 0.9;
    }

    // Publish the final torus for the panel. Only the first HEIGHT rows and WIDTH columns are live.
    for row in 0..HEIGHT {
        out.ca[row][..WIDTH].copy_from_slice(&grid[row]);
    }
    out.ca_play = 0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::Mode;
    use crate::sequence::generate;

    #[test]
    fn every_rule_family_produces_hits_and_a_bounded_grid() {
        for rule in 0..5u8 {
            for symmetry in 1..=4usize {
                let p = GenParams {
                    kind: Mode::Life,
                    steps: 64,
                    pulses: 4,
                    density: 0.5,
                    ca_rule: rule,
                    symmetry,
                    probability: 1.0,
                    ..GenParams::default()
                };
                let out = generate(&p);
                assert!(out.hit[..64].iter().any(|hit| *hit), "rule {rule} symmetry {symmetry}");
                assert!(
                    out.ca[..HEIGHT].iter().any(|row| row[..WIDTH].iter().any(|cell| *cell)),
                    "published grid is empty for rule {rule} symmetry {symmetry}"
                );
                assert!(out.slotted, "Life must publish scale-degree slots");
            }
        }
    }

    #[test]
    fn one_step_patterns_make_a_sound() {
        let out = generate(&GenParams {
            kind: Mode::Life,
            steps: 1,
            pulses: 0,
            density: 0.25,
            probability: 1.0,
            ..GenParams::default()
        });
        assert!(out.hit[0]);
    }
}
