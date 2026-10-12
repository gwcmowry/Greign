// Camera RGB to scene-linear working-space matrix.

struct Params { row0: vec4<f32>, row1: vec4<f32>, row2: vec4<f32>, misc: vec4<f32> };
@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> params: Params;
@compute @workgroup_size(16,16,1)
fn process(@builtin(global_invocation_id) invocation: vec3<u32>) {
    if invocation.x>=u32(params.misc.y) || invocation.y>=u32(params.misc.z) { return; }
    let camera=textureLoad(source_texture,vec2<i32>(invocation.xy),0).rgb;
    let pixel=vec3<f32>(dot(params.row0.xyz,camera),dot(params.row1.xyz,camera),dot(params.row2.xyz,camera));
    textureStore(output_texture,vec2<i32>(invocation.xy),vec4<f32>(pixel,1.0));
}
