// Packed RAW -> cropped, normalized, reconstructed, white-balanced R32F.

struct Params {
    source: vec4<u32>,
    output: vec4<u32>,
    cfa: vec4<u32>,
    black: vec4<f32>,
    white: vec4<f32>,
    wb_threshold: vec4<f32>,
    level_counts: vec4<u32>,
};

@group(0) @binding(0) var<storage, read> raw: array<u32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<r32float, write>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<storage, read> highlight_context: array<vec4<f32>>;

fn cfa_index(x: u32, y: u32) -> u32 {
    return (y % 2u) * 2u + (x % 2u);
}

fn color_at(x: u32, y: u32) -> u32 {
    return params.cfa[cfa_index(x, y)];
}

fn raw_sample(index: u32) -> f32 {
    let packed = raw[index / 2u];
    let shift = (index % 2u) * 16u;
    return f32((packed >> shift) & 65535u);
}

fn normalized_at(x: u32, y: u32) -> f32 {
    let plane = cfa_index(x, y);
    let black_index = select(plane, 0u, params.level_counts.x == 1u);
    let white_index = select(plane, 0u, params.level_counts.y == 1u);
    let black = params.black[black_index];
    let white = params.white[white_index];
    return (raw_sample(y * params.source.x + x) - black) / (white - black);
}

fn white_balance(color: u32) -> f32 {
    if color == 0u { return params.wb_threshold.x; }
    if color == 1u { return params.wb_threshold.y; }
    return params.wb_threshold.z;
}

fn balanced_at(x: u32, y: u32) -> f32 {
    let color = color_at(x, y);
    return normalized_at(x, y) * white_balance(color);
}

fn clip_level(color: u32) -> f32 {
    return params.wb_threshold.w * white_balance(color);
}

fn opposed_reference(x: u32, y: u32, target_color: u32) -> f32 {
    var sums = vec3<f32>(0.0);
    var counts = vec3<u32>(0u);
    for (var dy = -1i; dy <= 1i; dy += 1i) {
        for (var dx = -1i; dx <= 1i; dx += 1i) {
            let nx = i32(x) + dx;
            let ny = i32(y) + dy;
            if nx < i32(params.source.z) || ny < i32(params.source.w) || nx >= i32(params.source.z+params.output.x) || ny >= i32(params.source.w+params.output.y) { continue; }
            let neighbor_color = color_at(u32(nx), u32(ny));
            sums[neighbor_color] += max(0.0, balanced_at(u32(nx), u32(ny)));
            counts[neighbor_color] += 1u;
        }
    }
    var roots = vec3<f32>(0.0);
    for (var channel = 0u; channel < 3u; channel += 1u) {
        if counts[channel] > 0u { roots[channel] = pow(sums[channel] / f32(counts[channel]), 1.0 / 3.0); }
    }
    var opposing = (roots.y + roots.z) * 0.5;
    if target_color == 1u { opposing = (roots.x + roots.z) * 0.5; }
    if target_color == 2u { opposing = (roots.x + roots.y) * 0.5; }
    return opposing * opposing * opposing;
}

fn direction(index: u32) -> vec2<i32> {
    if index == 0u { return vec2<i32>(-1i, -1i); }
    if index == 1u { return vec2<i32>( 0i, -1i); }
    if index == 2u { return vec2<i32>( 1i, -1i); }
    if index == 3u { return vec2<i32>(-1i,  0i); }
    if index == 4u { return vec2<i32>( 1i,  0i); }
    if index == 5u { return vec2<i32>(-1i,  1i); }
    if index == 6u { return vec2<i32>( 0i,  1i); }
    return vec2<i32>(1i, 1i);
}

@compute @workgroup_size(16, 16, 1)
fn process(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if invocation.x >= params.output.x || invocation.y >= params.output.y { return; }
    let x = invocation.x + params.source.z;
    let y = invocation.y + params.source.w;
    let color = color_at(x, y);
    var value = balanced_at(x, y);
    if value >= clip_level(color) {
        let reference = opposed_reference(x, y, color);
        let tile=(invocation.y / params.output.w)*params.output.z+invocation.x / params.output.w;
        var chrominance=highlight_context[tile][color];
        if highlight_context[tile].w==0.0 {
            var residual_sum=0.0;var residual_count=0u;
            for(var i=0u;i<8u;i+=1u){let offset=direction(i);
                for(var d=1i;d<=4i;d+=1i){
                    let nx=i32(x)+offset.x*d*2i;let ny=i32(y)+offset.y*d*2i;
                    if nx<i32(params.source.z)||ny<i32(params.source.w)||nx>=i32(params.source.z+params.output.x)||ny>=i32(params.source.w+params.output.y){break;}
                    let candidate=balanced_at(u32(nx),u32(ny));
                    if candidate>clip_level(color)*0.2 && candidate<clip_level(color){
                        residual_sum+=candidate-opposed_reference(u32(nx),u32(ny),color);residual_count+=1u;break;
                    }
                }
            }
            if residual_count>=3u {chrominance=residual_sum/f32(residual_count);}
        }
        value = max(value, reference + chrominance);
    }
    textureStore(output_texture, vec2<i32>(invocation.xy), vec4<f32>(value, 0.0, 0.0, 1.0));
}
