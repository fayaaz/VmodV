/*{
  "ISFVSN": "2.0",
  "CREDIT": "Port of \"Hot Shower\" by Kali (https://www.shadertoy.com/view/4lf3Rj)",
  "DESCRIPTION": "A volumetric Kaliset fractal flow striking a planet. CC BY-NC-SA 3.0, see source header and THIRD_PARTY_NOTICES.md.",
  "MODV_CATEGORY": "3D Scenes",
  "CATEGORIES": ["GENERATOR"],
  "INPUTS": []
}*/

// Hot Shower — port of "Hot Shower" by Kali
// Source: https://www.shadertoy.com/view/4lf3Rj (created 2013-09-12)
//
// This ported shader is licensed under the Creative Commons
// Attribution-NonCommercial-ShareAlike 3.0 Unported License
// (CC BY-NC-SA 3.0), because the source shader carries no licence of its
// own and the Shadertoy default licence applies:
// https://creativecommons.org/licenses/by-nc-sa/3.0/
//
// Attribution: Kali. Modifications remain under the same licence.
// The source's two textures (a 256x256 noise and a 1024x1024 bark/rock
// photo) are not redistributed — their licences are unknown — and are
// replaced with procedural value noise: a fast hash for the dither and
// hot air, and a ridged fbm with lichen blotches and bark striations for
// the planet's surface. Otherwise the port is faithful.

// rendering params
const float sphsize=.7; // planet size
const float dist=.27; // distance for glow and distortion
const float perturb=.3; // distortion amount of the flow around the planet
const float displacement=.015; // hot air effect
const float windspeed=.4; // speed of wind flow
const float steps=110.; // number of steps for the volumetric rendering
const float stepsize=.025;
const float brightness=.43;
const vec3 planetcolor=vec3(0.55,0.4,0.3);
const float fade=.005; //fade by distance
const float glow=3.5; // glow amount, mainly on hit side


// fractal params
const int iterations=13;
const float fractparam=.7;
const vec3 offset=vec3(1.5,2.,-1.5);

// vec2 to float hash (value noise basis)
float hash21(vec2 p) {
  return fract(sin(dot(p, vec2(425.215, 714.388)))*45758.5453);
}

// smooth value noise in [0, 1]
float vnoise( vec2 p )
{
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f*f*(3.0-2.0*f);
    float n00 = hash21(i);
    float n10 = hash21(i + vec2(1.0, 0.0));
    float n01 = hash21(i + vec2(0.0, 1.0));
    float n11 = hash21(i + vec2(1.0, 1.0));
    return mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y);
}

// layered bark-like surface: ridged fbm + lichen blotches + striations
float planetSurface( vec2 p )
{
    float f = 0.0;
    float a = 0.5;
    vec2 q = p;
    for (int i = 0; i < 5; i++) {
        f += a * vnoise(q);
        q = q * 2.03 + vec2(19.7, 7.3);
        a *= 0.55;
    }
    float blotch  = vnoise(p * 11.0 + 3.3);          // lichen-ish
    float streak  = vnoise(vec2(p.x * 0.8, p.y * 14.0)); // bark striations
    f = f * 0.62 + blotch * 0.22 + streak * 0.16;
    return clamp(f, 0.0, 1.0);
}

float wind(vec3 p) {
	float d=max(0.,dist-max(0.,length(p)-sphsize)/sphsize)/dist; // for distortion and glow area
	float x=max(0.2,p.x*2.); // to increase glow on left side
	p.y*=1.+max(0.,-p.x-sphsize*.25)*1.5; // left side distortion (cheesy)
	p-=d*normalize(p)*perturb; // spheric distortion of flow
	p+=vec3(TIME*windspeed,0.,0.); // flow movement
	p=abs(fract((p+offset)*.1)-.5); // tile folding
	for (int i=0; i<iterations; i++) {
		p=abs(p)/dot(p,p)-fractparam; // the magic formula for the hot flow
	}
	return length(p)*(1.+d*glow*x)+d*glow*x; // return the result with glow applied
}

void main()
{
	// get ray dir
	vec2 uv = gl_FragCoord.xy / RENDERSIZE.xy-.5;
	vec3 dir=vec3(uv,1.);
	dir.x*=RENDERSIZE.x/RENDERSIZE.y;
	vec3 from=vec3(0.,0.,-2.+vnoise(uv*48.0+TIME*2.0)*stepsize); //from+dither

	// volumetric rendering
	float v=0., l=-0.0001, t=TIME*windspeed*.2;
	for (int i = 10; i < 110; i++) {
		float r = float(i);
		vec3 p=from+r*dir*stepsize;
		float tx=vnoise(uv*24.0+vec2(t,0.))*displacement; // hot air effect
		if (length(p)-sphsize-tx>0.)
		// outside planet, accumulate values as ray goes, applying distance fading
			v+=min(50.,wind(p))*max(0.,1.-r*fade);
		else if (l<0.)
		//inside planet, get planet shading if not already
		//loop continues because of previous problems with breaks and not always optimizes much
			l=pow(max(.53,dot(normalize(p),normalize(vec3(-1.,.5,-0.3)))),4.)
			*(.5+planetSurface(uv*vec2(2.,1.)*(1.+p.z*.5)+vec2(tx+t*.5,0.))*2.);
		}
	v/=steps; v*=brightness; // average values and apply bright factor
	vec3 col=vec3(v*1.25,v*v,v*v*v)+l*planetcolor; // set color
	col*=1.-length(pow(abs(uv),vec2(5.)))*14.; // vignette (kind of)
	gl_FragColor = vec4(col,1.0);
}
