// Bilinear Bayer demosaic correctness kernel.

struct Params {
    width: u32,
    height: u32,
    _padding: vec2<u32>,
    cfa: vec4<u32>,
};

@group(0) @binding(0)
var<storage, read> source: array<f32>;

@group(0) @binding(1)
var<storage, read_write> destination: array<vec4<f32>>;

@group(0) @binding(2)
var<uniform> params: Params;

fn color_at(x: u32, y: u32) -> u32 {
    return params.cfa[(y % 2u) * 2u + (x % 2u)];
}

fn interpolate(x: u32, y: u32, color: u32) -> f32 {
    let center = y * params.width + x;
    if color_at(x, y) == color {
        return source[center];
    }
    var sum = 0.0;
    var count = 0u;
    for (var dy = -1i; dy <= 1i; dy += 1i) {
        for (var dx = -1i; dx <= 1i; dx += 1i) {
            let nx = i32(x) + dx;
            let ny = i32(y) + dy;
            if nx < 0 || ny < 0 || nx >= i32(params.width) || ny >= i32(params.height) {
                continue;
            }
            if color_at(u32(nx), u32(ny)) == color {
                sum += source[u32(ny) * params.width + u32(nx)];
                count += 1u;
            }
        }
    }
    if count == 0u {
        return 0.0;
    }
    return sum / f32(count);
}

@compute @workgroup_size(16, 16, 1)
fn process(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if invocation.x >= params.width || invocation.y >= params.height {
        return;
    }
    let index = invocation.y * params.width + invocation.x;
    destination[index] = vec4<f32>(
        interpolate(invocation.x, invocation.y, 0u),
        interpolate(invocation.x, invocation.y, 1u),
        interpolate(invocation.x, invocation.y, 2u),
        1.0,
    );
}
