#version 330 core
in vec2 uv;
out vec4 color;
uniform sampler2D source;
uniform vec4 lighting; // exposure, contrast, shadows, highlights
uniform vec4 balance; // warmth, tint, saturation, material (0 negative, 1 bw, 2 positive)
uniform vec3 bands[8];
uniform vec4 crop;
uniform int rotation;
uniform vec2 tilt;
uniform vec2 sample_grid[3];
uniform vec2 sample_size;
uniform int mirrored;
uniform vec4 view_window;
uniform int export_linear; // 0 preview; 1 linear float; 2 JPEG RGB8; 3 JPEG sRGB float
float luminance(vec3 v) {return dot(v, vec3(0.2627002,0.67799807,0.059301715));}
float smooth01(float x) {x=clamp(x,0.,1.);return x*x*(3.-2.*x);}
vec3 mix_colors(vec3 p) {
    float y=luminance(p), hi=max(p.r,max(p.g,p.b)), lo=min(p.r,min(p.g,p.b)), c=hi-lo;
    if(hi<=0. || c<=hi*0.0001 || y<=0.) return p;
    float hue=60.*(hi==p.r ? (p.g-p.b)/c : hi==p.g ? (p.b-p.r)/c+2. : (p.r-p.g)/c+4.);
    hue=mod(hue,360.);
    float centers[9]=float[9](0.,30.,60.,120.,180.,240.,270.,300.,360.);
    int index=7;
    for(int i=0;i<8;i++) {if(hue<centers[i+1]){index=i;break;}}
    float t=smooth01((hue-centers[index])/(centers[index+1]-centers[index]));
    vec3 band=mix(bands[index],bands[(index+1)%8],t);
    float strength=smooth01(c/hi/0.08);
    float shift=clamp(band.x,-60.,60.)*strength;
    float saturation=max(0.,1.+clamp(band.y,-100.,200.)*0.01*strength);
    float gain=exp2(clamp(band.z,-100.,100.)*0.01*strength);
    if(shift==0. && saturation==1. && gain==1.) return p;
    float h=mod(hue+shift,360.)/60., x=c*(1.-abs(mod(h,2.)-1.));
    vec3 r=h<1.?vec3(c,x,0.):h<2.?vec3(x,c,0.):h<3.?vec3(0.,c,x):h<4.?vec3(0.,x,c):h<5.?vec3(x,0.,c):vec3(c,0.,x);
    r+=lo;
    return (vec3(y)+(r-vec3(luminance(r)))*saturation)*gain;
}
float gamma_encode(float v) {v=clamp(v,0.,1.);return v<=0.0031308 ? 12.92*v : 1.055*pow(v,1./2.4)-0.055;}
vec3 finish_pixel(vec3 v) {
    v*=exp2(vec3(balance.x,-balance.y,-balance.x)*0.008);
    float y=max(luminance(v),0.000001);
    float contrast=pow(y/0.18,clamp(lighting.y/100.,-0.9,0.9));
    v=(vec3(y)+(v-vec3(y))*max(0.,1.+balance.z/100.))*contrast*exp2(clamp(lighting.x,-6.,6.));
    if(lighting.z!=0. || lighting.w!=0.) {
        float stops=log2(max(luminance(v),0.000001)/0.18);
        v*=exp2(2.*(clamp(lighting.z,-100.,100.)*0.01*smooth01(-stops/5.)+clamp(lighting.w,-100.,100.)*0.01*smooth01(stops/4.)));
    }
    if(balance.w==1.) v=vec3(luminance(v));
    else v=mix_colors(v);
    if(balance.w==2.) {float peak=max(abs(v.r),max(abs(v.g),abs(v.b)));v*=(1.+peak*0.25)/(1.+peak);}
    return v;
}
vec3 finished_texel(ivec2 p) {
    return finish_pixel(texelFetch(source,clamp(p,ivec2(0),textureSize(source,0)-1),0).rgb);
}
void main() {
    vec2 q=vec2(uv.x,1.-uv.y);
    q=view_window.xy+q*view_window.zw;
    if(mirrored!=0) q.x=1.-q.x;
    if(rotation==1) q=vec2(q.y,1.-q.x);
    else if(rotation==2) q=vec2(1.)-q;
    else if(rotation==3) q=vec2(1.-q.y,q.x);
    vec2 size=vec2(textureSize(source,0));
    vec2 offset=(q-0.5)*crop.zw*size;
    vec2 position=crop.xy+crop.zw*0.5+vec2(tilt.y*offset.x-tilt.x*offset.y,tilt.x*offset.x+tilt.y*offset.y)/size;
    vec3 v;
    float footprint=max(length(dFdx(position*size)),length(dFdy(position*size)));
    if(export_linear==0 && footprint>1.2) {
        v=finish_pixel(texture(source,position).rgb);
    } else if(tilt.x!=0.) {
        // Match CPU ordering: finish source pixels, then interpolate the tilt.
        vec2 index=export_linear!=0 ? floor(q*sample_size) : q*sample_size-0.5;
        vec2 pixel=sample_grid[0]+sample_grid[2]*index.y+sample_grid[1]*index.x;
        ivec2 p=ivec2(floor(pixel));
        vec2 f=fract(pixel);
        v=mix(mix(finished_texel(p),finished_texel(p+ivec2(1,0)),f.x),
              mix(finished_texel(p+ivec2(0,1)),finished_texel(p+ivec2(1,1)),f.x),f.y);
    } else {
        v=finish_pixel(export_linear!=0 ? texelFetch(source,ivec2(position*size),0).rgb : texture(source,position).rgb);
    }
    if(export_linear==1) {color=vec4(v,1.);return;}
    vec3 rgb=vec3(dot(v,vec3(1.660491,-0.5876411,-0.07284986)),dot(v,vec3(-0.12455047,1.1328999,-0.00834942)),dot(v,vec3(-0.01815076,-0.1005789,1.1187297)));
    vec3 encoded=vec3(gamma_encode(rgb.r),gamma_encode(rgb.g),gamma_encode(rgb.b));
    // Quantize explicitly to match CPU round(), independent of a driver's
    // normalized framebuffer conversion rounding policy.
    if(export_linear==2) encoded=floor(encoded*255.+0.5)/255.;
    color=vec4(encoded,1.);
}
