// Fused capture sharpening, tone, Rec.2020->sRGB, OETF, and orientation.

struct Params { dimensions: vec4<u32>, settings: vec4<f32> };
@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: Params;
fn oriented_source(x:u32,y:u32)->vec2<u32>{
    let w=params.dimensions.x; let h=params.dimensions.y; let o=params.dimensions.z;
    if o==2u{return vec2<u32>(w-1u-x,y);} if o==3u{return vec2<u32>(w-1u-x,h-1u-y);}
    if o==4u{return vec2<u32>(x,h-1u-y);} if o==5u{return vec2<u32>(y,x);}
    if o==6u{return vec2<u32>(y,h-1u-x);} if o==7u{return vec2<u32>(w-1u-y,h-1u-x);}
    if o==8u{return vec2<u32>(w-1u-y,x);} return vec2<u32>(x,y);
}
fn sharpen(x:u32,y:u32)->vec3<f32>{
    let center=textureLoad(source_texture,vec2<i32>(i32(x),i32(y)),0).rgb;
    let weights=vec3<f32>(0.2627002,0.67799807,0.059301715);
    let luma=dot(center,weights); let chroma=center-vec3<f32>(luma);
    let variance=0.000025+0.0004*max(luma,0.0);
    var blur=0.0; var count=0u; var total=0.0; var sum=vec3<f32>(0.0);
    let radius=select(1i,2i,params.settings.w>0.0);
    for(var dy=-radius;dy<=radius;dy+=1i){for(var dx=-radius;dx<=radius;dx+=1i){
        let nx=i32(x)+dx;let ny=i32(y)+dy;
        if(nx<0i||ny<0i||nx>=i32(params.dimensions.x)||ny>=i32(params.dimensions.y)){continue;}
        let neighbor=textureLoad(source_texture,vec2<i32>(nx,ny),0).rgb;
        let nl=dot(neighbor,weights);
        if abs(dx)<=1i && abs(dy)<=1i {blur+=nl;count+=1u;}
        if params.settings.w>0.0 {
            let nc=neighbor-vec3<f32>(nl);let dl=nl-luma;let dc=nc-chroma;
            let weight=1.0/(1.0+dl*dl/variance+(dc.x*dc.x+dc.z*dc.z)/(4.0*variance));
            sum+=weight*nc;total+=weight;
        }
    }}
    var filtered=center;
    if params.settings.w>0.0 {filtered+=params.settings.w*(sum/total-chroma);}
    return filtered+vec3<f32>(params.settings.x*(luma-blur/f32(count)));
}
fn oetf(v:f32)->f32{let c=clamp(v,0.0,1.0);if c<=0.0031308{return 12.92*c;}return 1.055*pow(c,1.0/2.4)-0.055;}
@compute @workgroup_size(16,16,1)
fn process(@builtin(global_invocation_id) invocation:vec3<u32>){
    if invocation.x>=params.dimensions.w || invocation.y>=u32(params.settings.y){return;}
    let source=oriented_source(invocation.x,invocation.y);
    var p=sharpen(source.x,source.y)*params.settings.z;
    let peak=max(abs(p.x),max(abs(p.y),abs(p.z)));p=p*((1.0+peak*0.25)/(1.0+peak));
    let s=vec3<f32>(1.660491*p.x-0.5876411*p.y-0.07284986*p.z,-0.12455047*p.x+1.1328999*p.y-0.00834942*p.z,-0.01815076*p.x-0.1005789*p.y+1.1187297*p.z);
    textureStore(output_texture,vec2<i32>(invocation.xy),vec4<f32>(oetf(s.x),oetf(s.y),oetf(s.z),1.0));
}
