// Direct GPU-resident texture output for interactive presentation.

struct Params {
    width: u32,
    height: u32,
    operation: u32,
    _padding: u32,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
};

@group(0) @binding(0)
var<storage, read> source: array<vec4<f32>>;

@group(0) @binding(1)
var destination: texture_storage_2d<rgba32float, write>;

@group(0) @binding(2)
var<uniform> params: Params;

fn signed_reinhard(value: vec3<f32>) -> vec3<f32> {
    return value / (vec3<f32>(1.0) + abs(value));
}

@compute @workgroup_size(16, 16, 1)
fn process(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if invocation.x >= params.width || invocation.y >= params.height {
        return;
    }
    let index = invocation.y * params.width + invocation.x;
    let input = source[index];
    var output = input;

    switch params.operation {
        case 0u: {
            output = vec4<f32>((input.xyz - params.a.xyz) / (params.b.xyz - params.a.xyz), input.w);
        }
        case 1u: {
            output = vec4<f32>(input.xyz * params.a.xyz, input.w);
        }
        case 2u: {
            output = vec4<f32>(input.xyz * exp2(params.a.x), input.w);
        }
        case 3u: {
            output = vec4<f32>(signed_reinhard(input.xyz), input.w);
        }
        case 4u: {
            output = vec4<f32>(
                dot(params.a.xyz, input.xyz),
                dot(params.b.xyz, input.xyz),
                dot(params.c.xyz, input.xyz),
                input.w,
            );
        }
        default: {}
    }
    textureStore(destination, vec2<i32>(invocation.xy), output);
}
