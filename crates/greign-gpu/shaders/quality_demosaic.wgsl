// Fused pattern-pixel-grouping (PPG) Bayer demosaic.
// Derived from Darktable's GPLv3 PPG implementation. The second pass is
// fused by recomputing the small green neighborhood from the sensor texture.

struct Params { dimensions: vec4<u32>, cfa: vec4<u32> };
@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> params: Params;

fn color_at(x: u32, y: u32) -> u32 { return params.cfa[(y % 2u) * 2u + (x % 2u)]; }
fn sample_at(x: u32, y: u32) -> f32 { return textureLoad(source_texture, vec2<i32>(i32(x), i32(y)), 0).x; }
// CPU's first PPG pass clamps known RGB samples before color differences.
// Keep signed RAW for the directional green estimator, but match that clamp
// when consuming the known red/blue neighbors in the second pass.
fn known_at(x: u32, y: u32) -> f32 { return max(0.0, sample_at(x,y)); }
fn border(x: u32, y: u32) -> bool {
    return x < 3u || y < 3u || x + 3u >= params.dimensions.x || y + 3u >= params.dimensions.y;
}
fn outer_border(x: u32, y: u32) -> bool {
    return x == 0u || y == 0u || x + 1u >= params.dimensions.x || y + 1u >= params.dimensions.y;
}
fn interpolate(x: u32, y: u32, color: u32) -> f32 {
    if color_at(x, y) == color { return sample_at(x, y); }
    var sum = 0.0; var count = 0u;
    for (var dy = -1i; dy <= 1i; dy += 1i) {
        for (var dx = -1i; dx <= 1i; dx += 1i) {
            let nx = i32(x) + dx; let ny = i32(y) + dy;
            if nx < 0i || ny < 0i || nx >= i32(params.dimensions.x) || ny >= i32(params.dimensions.y) { continue; }
            if color_at(u32(nx), u32(ny)) == color { sum += sample_at(u32(nx), u32(ny)); count += 1u; }
        }
    }
    if count == 0u { return 0.0; }
    return sum / f32(count);
}
fn green_at(x: u32, y: u32) -> f32 {
    if color_at(x, y) == 1u { return max(0.0, sample_at(x, y)); }
    if border(x, y) { return max(0.0, interpolate(x, y, 1u)); }
    let l3=sample_at(x-3u,y); let l2=sample_at(x-2u,y); let l=sample_at(x-1u,y);
    let r=sample_at(x+1u,y); let r2=sample_at(x+2u,y); let r3=sample_at(x+3u,y);
    let u3=sample_at(x,y-3u); let u2=sample_at(x,y-2u); let u=sample_at(x,y-1u);
    let d=sample_at(x,y+1u); let d2=sample_at(x,y+2u); let d3=sample_at(x,y+3u);
    let center=sample_at(x,y);
    let guess_x=(l+center+r)*2.0-r2-l2;
    let diff_x=(abs(l2-center)+abs(r2-center)+abs(l-r))*3.0+(abs(r3-r)+abs(l3-l))*2.0;
    let guess_y=(u+center+d)*2.0-d2-u2;
    let diff_y=(abs(u2-center)+abs(d2-center)+abs(u-d))*3.0+(abs(d3-d)+abs(u3-u))*2.0;
    let horizontal=clamp(guess_x*0.25,min(l,r),max(l,r));
    let vertical=clamp(guess_y*0.25,min(u,d),max(u,d));
    let tolerance=1.0e-6*max(1.0,max(diff_x,diff_y));
    if abs(diff_x-diff_y)<=tolerance {return max(0.0,(horizontal+vertical)*0.5);}
    if diff_x > diff_y { return max(0.0,vertical); }
    return max(0.0,horizontal);
}
fn diagonal_color(x: u32, y: u32, green: f32) -> f32 {
    let tl=vec2<u32>(x-1u,y-1u); let tr=vec2<u32>(x+1u,y-1u);
    let bl=vec2<u32>(x-1u,y+1u); let br=vec2<u32>(x+1u,y+1u);
    let value_tl=known_at(tl.x,tl.y); let value_tr=known_at(tr.x,tr.y);
    let value_bl=known_at(bl.x,bl.y); let value_br=known_at(br.x,br.y);
    let diff_one=abs(value_tl-value_br)+abs(green_at(tl.x,tl.y)-green)+abs(green_at(br.x,br.y)-green);
    let guess_one=value_tl+value_br+2.0*green-green_at(tl.x,tl.y)-green_at(br.x,br.y);
    let diff_two=abs(value_tr-value_bl)+abs(green_at(tr.x,tr.y)-green)+abs(green_at(bl.x,bl.y)-green);
    let guess_two=value_tr+value_bl+2.0*green-green_at(tr.x,tr.y)-green_at(bl.x,bl.y);
    let tolerance=1.0e-6*max(1.0,max(diff_one,diff_two));
    if abs(diff_one-diff_two)<=tolerance {return max(0.0,(guess_one+guess_two)*0.25);}
    if diff_one > diff_two { return max(0.0,guess_two*0.5); }
    if diff_one < diff_two { return max(0.0,guess_one*0.5); }
    return max(0.0,(guess_one+guess_two)*0.25);
}
@compute @workgroup_size(16,16,1)
fn process(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let x=invocation.x; let y=invocation.y;
    if x>=params.dimensions.x || y>=params.dimensions.y { return; }
    if outer_border(x,y) {
        textureStore(output_texture,vec2<i32>(invocation.xy),vec4<f32>(max(vec3<f32>(0.0),vec3<f32>(interpolate(x,y,0u),interpolate(x,y,1u),interpolate(x,y,2u))),1.0)); return;
    }
    let green=green_at(x,y);
    let site=color_at(x,y);
    var red=0.0; var blue=0.0;
    if site == 1u {
        let top=vec2<u32>(x,y-1u); let bottom=vec2<u32>(x,y+1u);
        let left=vec2<u32>(x-1u,y); let right=vec2<u32>(x+1u,y);
        if color_at(x+1u,y) == 0u {
            blue=(known_at(top.x,top.y)+known_at(bottom.x,bottom.y)+2.0*green-green_at(top.x,top.y)-green_at(bottom.x,bottom.y))*0.5;
            red=(known_at(left.x,left.y)+known_at(right.x,right.y)+2.0*green-green_at(left.x,left.y)-green_at(right.x,right.y))*0.5;
        } else {
            red=(known_at(top.x,top.y)+known_at(bottom.x,bottom.y)+2.0*green-green_at(top.x,top.y)-green_at(bottom.x,bottom.y))*0.5;
            blue=(known_at(left.x,left.y)+known_at(right.x,right.y)+2.0*green-green_at(left.x,left.y)-green_at(right.x,right.y))*0.5;
        }
    } else if site == 0u {
        red=max(0.0,sample_at(x,y)); blue=diagonal_color(x,y,green);
    } else {
        blue=max(0.0,sample_at(x,y)); red=diagonal_color(x,y,green);
    }
    textureStore(output_texture,vec2<i32>(invocation.xy),vec4<f32>(max(0.0,red),green,max(0.0,blue),1.0));
}
