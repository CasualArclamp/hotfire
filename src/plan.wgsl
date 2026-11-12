// Which parts of the play space the gas solver visits. The 8x8-cell tiles are grouped into blocks of 8x8
// tiles (64x64 cells); every few steps this lists the blocks with anything going on in or next to them, and
// the solver is only dispatched over that list. Still air far from the engine then costs nothing at all,
// rather than a workgroup per tile that finds it has nothing to do.
//
// A tile is quiet when both copies of its bookkeeping (the two spec buffers ping-pong) say it didn't change
// and was asleep (a sleeping tile's "last ran" is negative). A block is left out only if every tile in it and
// within a block's width around it is quiet: gas can't move a cell a step, so nothing reaches a block that is
// left out before the list is made again.

struct P { w: u32, h: u32 }
struct Args { x: u32, count: atomic<u32>, z: u32 }

@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> spec_a: array<f32>;
@group(0) @binding(2) var<storage, read> spec_b: array<f32>;
// the cell materials, with the list of blocks after the last cell
@group(0) @binding(3) var<storage, read_write> mat: array<u32>;
// the solver's indirect dispatch: 64 tiles a block, by the number of blocks listed
@group(0) @binding(4) var<storage, read_write> args: Args;

@compute @workgroup_size(1)
fn reset() {
    args.x = 64u;
    atomicStore(&args.count, 0u);
    args.z = 1u;
}

@compute @workgroup_size(64)
fn plan(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tw = (p.w + 7u) / 8u;
    let th = (p.h + 7u) / 8u;
    let sw = (tw + 7u) / 8u;
    let sh = (th + 7u) / 8u;
    let s = gid.x;
    if (s >= sw * sh) { return; }
    let base = p.w * p.h;
    let nt = tw * th;
    let tx0 = i32(s % sw) * 8;
    let ty0 = i32(s / sw) * 8;
    // the first block always runs: its first tile keeps the step counter
    var hot = s == 0u;
    for (var ty = max(ty0 - 8, 0); ty < min(ty0 + 16, i32(th)) && !hot; ty = ty + 1) {
        for (var tx = max(tx0 - 8, 0); tx < min(tx0 + 16, i32(tw)); tx = tx + 1) {
            let t = u32(ty) * tw + u32(tx);
            if (spec_a[base + t] != 0.0 || spec_b[base + t] != 0.0 || spec_a[base + 2u * nt + t] >= 0.0 || spec_b[base + 2u * nt + t] >= 0.0) {
                hot = true;
                break;
            }
        }
    }
    if (hot) {
        let k = atomicAdd(&args.count, 1u);
        mat[base + k] = s;
    }
}
