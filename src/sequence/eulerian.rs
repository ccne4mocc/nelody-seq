use super::{shuffle, GenParams, Pattern, Rng};

/// Eulerian circuit (Hierholzer, shuffled adjacency) over a random balanced digraph.
/// Vertex 0 is a rest (except at steps=1), vertices 1..nv are pitches.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 1);
    let nv = p.pulses.clamp(3, 8);
    let mut edges = [(0u8, 0u8); 32];
    let mut ne = 0usize;
    let mut perm = [0u8, 1, 2, 3, 4, 5, 6, 7];
    shuffle(&mut perm[..nv], &mut rng);
    for k in 0..nv {
        edges[ne] = (perm[k], perm[(k + 1) % nv]);
        ne += 1;
    }
    for _ in 0..1 + rng.below(2) {
        let len = 1 + rng.below(4);
        let mut pv = [0u8, 1, 2, 3, 4, 5, 6, 7];
        shuffle(&mut pv[..nv], &mut rng);
        for k in 0..len {
            edges[ne] = (pv[k], pv[(k + 1) % len]);
            ne += 1;
        }
    }
    let mut adj = [[0u8; 4]; 8];
    let mut alen = [0usize; 8];
    for &(a, b) in &edges[..ne] {
        adj[a as usize][alen[a as usize]] = b;
        alen[a as usize] += 1;
    }
    for v in 0..nv {
        let l = alen[v];
        shuffle(&mut adj[v][..l], &mut rng);
    }
    let mut stack = [0u8; 40];
    let mut sp = 1usize;
    stack[0] = perm[0];
    let mut walk = [0u8; 40];
    let mut on = 0usize;
    while sp > 0 {
        let v = stack[sp - 1] as usize;
        if alen[v] > 0 {
            alen[v] -= 1;
            stack[sp] = adj[v][alen[v]];
            sp += 1;
        } else {
            walk[on] = v as u8;
            on += 1;
            sp -= 1;
        }
    }
    walk[..on].reverse();
    for k in 0..ne.min(on.saturating_sub(1)) {
        out.circuit[k] = (walk[k], walk[k + 1]);
    }
    out.ne = ne;
    out.nv = nv;
    let mut any = false;
    for i in 0..n {
        let k = i % ne;
        let dest = out.circuit[k].1;
        out.step_edge[i] = k as u8;
        out.state[i] = dest;
        out.hit[i] = dest != 0;
        if out.hit[i] {
            any = true;
        }
        out.u[i] = (dest.saturating_sub(1)) as f32 / (nv - 2).max(1) as f32;
        out.acc[i] = if dest == 1 { 0.95 } else { 0.76 };
    }
    if !any {
        out.hit[0] = true;
        out.u[0] = 0.5;
        out.acc[0] = 0.85;
    }
    out
}
