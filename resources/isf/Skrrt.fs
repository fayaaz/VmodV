/*{
  "DESCRIPTION": "TRUENO 86 - a 1983 Sprinter Trueno AE86 going down a Japanese mountain pass at night, sideways, on its headlights. The car is one signed distance field built from the real thing's measurements: 4180 long, 1625 wide, 1335 tall on a 2400 wheelbase, 185/60 on fourteens, and the overhangs deliberately unequal - 830 in front, 970 behind - because a body box centred on the axles is what makes a model car look like a toy. The road is a SHEAR OF SPACE: a cross-section drawn once as a polyline - cut face, gutter, six metres of asphalt, shoulder, then nothing - and extruded along z while every sample is displaced sideways and downward by the centreline's own functions, so the whole descending pass is three sines and an exact 2D distance rather than a mesh. Because the profile is a polyline and the road is an extrusion, that distance is EXACT; the shear costs one constant Lipschitz factor and nothing else. The drift is not an animation, it is the road: the car's yaw is the road's heading plus a term proportional to its CURVATURE, so it points into the corner wherever the corner is, the front wheels counter-steer against that angle, the body leans on it, the line runs wide on it and the tyre smoke comes off it. The drift, the camber and the suspension are all one transform on the RAY - the sample is pushed backward through the car's yaw, roll, pitch and a volume-preserving squash before the body is evaluated, and the returned distance is multiplied by the smallest scale factor so the field stays conservative and the march stays legal. Night is what makes it: one dim moon and two headlamps, which are elliptical cones with a flat CUTOFF along the top the way a low beam really is, and they are integrated through the air as well as landed on the road - sixteen taps down the view ray gathering haze, tail-light red and the smoke off the rear tyres, which the tail lights are the only thing lighting because the beams point the other way. The camera is a DIRECTOR rather than a chase rig: every uCut seconds the frame cuts to a shot drawn from a hash of the cut index - front three-quarter, head on, off the bank, chase, down at wheel height, or a roadside pass-by where the camera stops following and the car comes to it - the draw is squared so the front views come up most often, and every shot dollies and swings inside itself, because a cut from one locked-off camera to another is still locked off. Everything on the ground is keyed to the ABSOLUTE distance down the pass rather than to the car, because the car sits at z = 0 forever and anything drawn at its own z is nailed to it - the asphalt, the tar seams every eight metres, the cat's eyes, the guardrail posts and the delineator poles all stream past because of that one substitution, and they carry half a frame of travel of ordered motion blur with them while the car, which is not moving relative to the camera, stays sharp. The reflectors are modelled as RETROREFLECTORS - emission proportional to the beam landing on them, not a specular lobe - which is why they flare white as the pool reaches them and go out the moment it leaves. When the rear quarter leans on the barrier it throws sparks: eighteen ballistic particles shed backward at road speed, each drawn as the segment it swept since the last frame and glowed by the closest distance from the view ray to that segment, with a floor on the radius of what one pixel is worth at that depth - as a density in the volumetric they were invisible, because a spray forty centimetres across is missed by sixteen taps three metres apart. Nothing can clip the mountain: the car's line is clamped by its own SWEPT WIDTH, which for a box yawed by the drift angle is halfW*|cos| + halfL*|sin| - two metres of a six metre road at forty degrees - so the harder it is sideways the less of the road it is allowed to use, and the camera climbs out of the cut face by querying the same cross-section it would otherwise be buried in. Every edge on the car is a hard intersection of planes - no rounded boxes anywhere - because a smoothed corner is a modern renderer's tell. The PS2 look is the render and not a filter: the fragment coordinate is quantised before the ray is built, so the frame is genuinely traced at a couple of hundred lines, then posterised through an ordered 4x4 Bayer dither. Thirty-five controls: body, face, pop-ups, spoiler, paint (as three channels, because a colour input is the one thing hosts disagree about), panda, gloss, wheels, tyre, speed, drift, bumps, lean, curve, grade, camera distance, camera height, camera angle, seconds per cut, lens, headlamps, beam spread, haze, smoke, sparks, motion blur, moon, stars, town, render resolution, colour depth, dither and grain.",
  "CREDIT": "CC0. Original. An homage - no marque assets, every number typed in by hand.",
  "CATEGORIES": ["generator", "raymarch", "retro", "ps2", "car", "vehicle", "jdm", "night", "procedural", "3d"],
  "INPUTS": [
    { "NAME": "uBody",    "TYPE": "float", "DEFAULT": 0.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uFace",    "TYPE": "float", "DEFAULT": 0.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uPopup",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uSpoiler", "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uPaintR",  "TYPE": "float", "DEFAULT": 0.86,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uPaintG",  "TYPE": "float", "DEFAULT": 0.87,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uPaintB",  "TYPE": "float", "DEFAULT": 0.88,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uPanda",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uGloss",   "TYPE": "float", "DEFAULT": 0.75,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uWheel",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uTyre",    "TYPE": "float", "DEFAULT": 0.35,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uSpeed",   "TYPE": "float", "DEFAULT": 16.0,  "MIN": 0.0,  "MAX": 40.0 },
    { "NAME": "uDrift",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.6 },
    { "NAME": "uBumps",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.0 },
    { "NAME": "uLean",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.0 },
    { "NAME": "uCurve",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 1.6 },
    { "NAME": "uGrade",   "TYPE": "float", "DEFAULT": 0.065, "MIN": 0.0,  "MAX": 0.16 },
    { "NAME": "uCamBack", "TYPE": "float", "DEFAULT": 7.00,  "MIN": 2.5,  "MAX": 30.0 },
    { "NAME": "uCamH",    "TYPE": "float", "DEFAULT": 0.80,  "MIN": 0.35, "MAX": 8.0 },
    { "NAME": "uCamSide", "TYPE": "float", "DEFAULT": 2.62,  "MIN": -3.14,"MAX": 3.14 },
    { "NAME": "uCut",     "TYPE": "float", "DEFAULT": 3.20,  "MIN": 0.0,  "MAX": 9.0 },
    { "NAME": "uLens",    "TYPE": "float", "DEFAULT": 1.25,  "MIN": 0.7,  "MAX": 3.5 },
    { "NAME": "uBeam",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.5 },
    { "NAME": "uSpread",  "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.4,  "MAX": 2.0 },
    { "NAME": "uHaze",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 3.0 },
    { "NAME": "uSmoke",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.5 },
    { "NAME": "uSpark",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.0 },
    { "NAME": "uBlur",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 3.0 },
    { "NAME": "uMoon",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 3.0 },
    { "NAME": "uStars",   "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.0 },
    { "NAME": "uTown",    "TYPE": "float", "DEFAULT": 1.0,   "MIN": 0.0,  "MAX": 2.0 },
    { "NAME": "uRes",     "TYPE": "float", "DEFAULT": 320.0, "MIN": 96.0, "MAX": 1080.0 },
    { "NAME": "uBits",    "TYPE": "float", "DEFAULT": 5.0,   "MIN": 2.0,  "MAX": 8.0 },
    { "NAME": "uDither",  "TYPE": "float", "DEFAULT": 0.80,  "MIN": 0.0,  "MAX": 1.0 },
    { "NAME": "uGrain",   "TYPE": "float", "DEFAULT": 0.10,  "MIN": 0.0,  "MAX": 1.0 }
  ]
}*/

// TRUENO 86 - ISF / GLSL ES 1.00 (gl_FragColor, TIME, RENDERSIZE).
// uBody: 0 = three-door liftback, 1 = two-door notchback coupe.
// uFace: 0 = Trueno (pop-up lamps), 1 = Levin (fixed quad lamps).
// +z is down the hill. The car stays at z = 0 and the pass flows past it.
// Everything is in metres, off the real car.

#define FAR 90.0

float gT, gPassF, gJit, gScrape, gHead, gPixK;
vec3  gSparkP, gSparkB, gSparkC, gPaint;
vec3  gSky, gHor, gFog, gMoonC, gMoonD, gLampC, gTailC;
// the road, resolved once per frame
float gZc, gX0, gY0, gBend, gCurv, gGrade, gBank;
// the car, resolved once per frame
vec3  carP, dScale;
float carYaw, carRoll, carPitch, carSteer, carSpin, carLane;
vec3  lampL, lampR, lampF, lampS, lampU, tailP;
// the body, resolved once per frame
float zNose, zTail, bWid, bSill, bBelt, bRoof, bHgt, bHlf, gYc, gHgt, gW;
float cowlZ, backZ, zPill, splitY;
vec3  nScr, nBak, nBon;
float wbase, wtrack, wrad, wwid;
float popA, popX, popY, popZ, popHW, popH, popL, popSink, popUp;
float spoil;

float hash12(vec2 p){
    vec3 q = fract(vec3(p.xyx)*0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y)*q.z);
}

float vnoise(vec2 p){
    vec2 i = floor(p), f = fract(p);
    f = f*f*(3.0 - 2.0*f);
    return mix(mix(hash12(i), hash12(i + vec2(1.0, 0.0)), f.x),
               mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), f.x), f.y);
}

float sdBox(vec3 p, vec3 b){
    vec3 q = abs(p) - b;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
}

// a wheel: a hard-edged cylinder with its axis on x
float sdWheel(vec3 p, float R, float w){
    vec2 d = vec2(length(p.yz) - R, abs(p.x) - w);
    return min(max(d.x, d.y), 0.0) + length(max(d, 0.0));
}

float segD(vec2 p, vec2 a, vec2 b){
    vec2 pa = p - a, ba = b - a;
    return length(pa - ba*clamp(dot(pa, ba)/dot(ba, ba), 0.0, 1.0));
}

// ---------------------------------------------------------------
// the pass
// ---------------------------------------------------------------
// The centreline, in absolute metres down the hill: three sines for the plan
// and one for the fall. The derivatives are written out rather than
// differenced, because the heading, the camber, the drift angle and the
// counter-steer all come out of them and a numerical derivative would put
// noise into every one of those at once.
float rdX(float s){
    return gCurv*(7.0*sin(s*0.0330) + 4.0*sin(s*0.0715 + 1.7) + 1.1*sin(s*0.0980 + 4.1));
}
float rdXd(float s){
    return gCurv*(0.23100*cos(s*0.0330) + 0.28600*cos(s*0.0715 + 1.7) + 0.10780*cos(s*0.0980 + 4.1));
}
float rdXdd(float s){
    return -gCurv*(0.0076230*sin(s*0.0330) + 0.0204490*sin(s*0.0715 + 1.7) + 0.0105644*sin(s*0.0980 + 4.1));
}
float rdY(float s){ return -gGrade*s + 1.70*sin(s*0.0231 + 2.2); }

// the same, relative to wherever the car has got to, so the world stays small
// and the float precision stays good however long it runs
float rx(float z){ return rdX(z + gZc) - gX0; }
float ry(float z){ return rdY(z + gZc) - gY0; }

// How far down the hill a sample is, in ROAD metres rather than in metres from
// the car. Every mark on the ground has to be keyed to this: the car sits at
// z = 0 forever, so anything drawn at the car-relative z is nailed to the car
// and the ground does not move, however fast the scene says it is going. It
// carries the per-pixel motion blur too, because smearing a texture along the
// direction it is travelling is the same thing as sampling it slightly early.
float zRoad(float z){ return mod(z + gZc + gJit, 5200.0); }

// 5200 metres is a whole number of guardrail posts, tar seams and cat's eyes,
// so the wrap is invisible in the marks; the noise needs its own wrap because
// it is sampled at up to ninety-five cycles a metre.
float nz(float x, float lim){ return mod(x, lim); }

// The shear. Every sample is pushed sideways and up by the centreline, which
// turns the whole descending pass into one straight extrusion along z.
vec3 bend(vec3 p){
    vec2 q = vec2(p.x - rx(p.z), p.y - ry(p.z));
    // Camber: the road banks into the corner. A rotation about the centreline
    // is an isometry, so it costs the field nothing at all.
    float bk = gBank*rdXdd(p.z + gZc);
    float c = cos(bk), s = sin(bk);
    return vec3(c*q.x + s*q.y, -s*q.x + c*q.y, p.z);
}

// The cross-section, drawn once: the cut face, the gutter, six metres of
// asphalt, the shoulder, and the mountain falling away. It is a POLYLINE, so
// the distance to it is exact - and because the road is an extrusion along z,
// that 2D distance is the 3D distance.
float crossSec(vec2 q){
    vec2 P0 = vec2(-40.00, 55.80);   // up the cut
    vec2 P1 = vec2( -3.30,  0.10);   // the lip above the gutter
    vec2 P2 = vec2( -3.16, -0.34);   // the bottom of it
    vec2 P3 = vec2( -2.86,  0.00);   // the left edge of the asphalt
    vec2 P4 = vec2(  3.06,  0.00);   // the right edge
    vec2 P5 = vec2(  3.62, -0.62);   // the shoulder
    vec2 P6 = vec2( 60.00,-69.00);   // and away down the mountain

    float d = segD(q, P0, P1);
    d = min(d, segD(q, P1, P2));
    d = min(d, segD(q, P2, P3));
    d = min(d, segD(q, P3, P4));
    d = min(d, segD(q, P4, P5));
    d = min(d, segD(q, P5, P6));

    // The height of that polyline, as a base slope plus one kink per vertex -
    // exact for any piecewise-linear profile, and branch-free.
    float s0 = (P1.y - P0.y)/(P1.x - P0.x);
    float s1 = (P2.y - P1.y)/(P2.x - P1.x);
    float s2 = (P3.y - P2.y)/(P3.x - P2.x);
    float s4 = (P5.y - P4.y)/(P5.x - P4.x);
    float s5 = (P6.y - P5.y)/(P6.x - P5.x);
    float h = P0.y + s0*(q.x - P0.x)
            + (s1 - s0)*max(q.x - P1.x, 0.0)
            + (s2 - s1)*max(q.x - P2.x, 0.0)
            + (0.0 - s2)*max(q.x - P3.x, 0.0)
            + (s4 - 0.0)*max(q.x - P4.x, 0.0)
            + (s5 - s4)*max(q.x - P5.x, 0.0);
    return (q.y < h) ? -d : d;
}

// The guardrail, its posts every 2.6 m, and the delineator poles beyond it
// every 15.6 m. The poles are there for one reason: a retroreflector answering
// a headlamp is the thing that tells you how fast you are going on a road with
// nothing else on it.
float railSDF(vec3 q){
    float zr = zRoad(q.z);
    float u = q.x - 3.46;
    float r = max(abs(u) - 0.055, abs(q.y - 0.585) - 0.150);
    float pz = mod(zr + 1.3, 2.6) - 1.3;
    float po = max(max(abs(u) - 0.048, abs(pz) - 0.055), abs(q.y - 0.24) - 0.34);
    float dz = mod(zr + 7.8, 15.6) - 7.8;
    float pole = max(length(vec2(q.x - 3.80, dz)) - 0.036, abs(q.y - 0.47) - 0.47);
    return min(min(r, po), pole);
}

// ---------------------------------------------------------------
// the pop-up lamp pod: hinged at its LEADING edge, up by the nose
// ---------------------------------------------------------------
// Which end the hinge is on is the whole mechanism. Pivot the front edge and
// the pod stands up out of the bonnet with its lens coming round to face
// forward; pivot the back edge instead and you have built a bonnet scoop that
// opens into the wind. The pod therefore lives at NEGATIVE local z - behind
// its own hinge - and a positive angle lifts that end.
vec3 podLocal(vec3 q){
    vec3 h = vec3(abs(q.x) - popX, q.y - popY, q.z - popZ);
    float c = cos(popA), s = sin(popA);
    return vec3(h.x, h.z*s + h.y*c, h.z*c - h.y*s);
}

// ---------------------------------------------------------------
// the car
// ---------------------------------------------------------------
// .x = distance, .y = material (2 shell, 4 wheel, 5 lamp pod)
vec2 carSDF(vec3 p){
    vec3 s = vec3(abs(p.x), p.y, p.z);
    float bz = 0.5*(zNose + zTail), bl = 0.5*(zNose - zTail);

    // ---- the lower body ----
    // A box between the sill and the beltline, then eight half-spaces. Every
    // one of them is a HARD cut: bevel a single corner and the era goes.
    float body = sdBox(p - vec3(0.0, bHgt, bz), vec3(bWid, bHlf, bl));
    // The bonnet falls toward the nose. Written as rise over run off the real
    // car - about nine centimetres across the length of it - because a quarter
    // of a radian turns a hatchback into a doorstop.
    body = max(body, dot(p - vec3(0.0, bBelt, cowlZ), nBon));
    // the valances tuck under at both ends
    body = max(body, dot(p - vec3(0.0, bSill + 0.220, zNose), normalize(vec3(0.0, -1.0, 2.20))));
    body = max(body, dot(p - vec3(0.0, bSill, zTail), normalize(vec3(0.0, -1.0, -1.05))));
    // the plan view is an octagon, not a rectangle
    body = max(body, dot(s - vec3(bWid*0.87, 0.0, zNose), normalize(vec3(1.0, 0.0, 1.30))));
    body = max(body, dot(vec3(s.x, p.y, -p.z) - vec3(bWid*0.86, 0.0, -zTail), normalize(vec3(1.0, 0.0, 1.15))));
    // A flank on this car is very nearly VERTICAL - a couple of degrees of
    // tumblehome at the shoulder, a hint of tuck at the sill. Wedge it in like
    // a doorstop and the wheels stand proud of the body, which is the single
    // thing that makes a car model read as a toy.
    body = max(body, dot(s - vec3(bWid, bBelt, 0.0), normalize(vec3(1.0, 0.26, 0.0))));
    body = max(body, dot(s - vec3(bWid - 0.012, bSill, 0.0), normalize(vec3(1.0, -0.30, 0.0))));

    // ---- the greenhouse ----
    // The box spans cowl to backlight base exactly; the screen and the tailgate
    // are two planes through the beltline, and where they cut this box is
    // where the pillars are.
    float hz = 0.5*(cowlZ + backZ), hl = 0.5*(cowlZ - backZ);
    float house = sdBox(p - vec3(0.0, gYc, hz), vec3(gW, gHgt, hl));
    house = max(house, dot(p - vec3(0.0, bBelt, cowlZ), nScr));
    house = max(house, dot(p - vec3(0.0, bBelt, backZ), nBak));
    house = max(house, dot(s - vec3(gW, bBelt, 0.0), normalize(vec3(1.0, 0.21, 0.0))));

    float shell = min(body, house);

    // ---- the recess the pop-ups came out of ----
    // Pushed half a metre away when they are down, so a closed bonnet is one
    // unbroken surface. Under-carving keeps the field conservative.
    float rec = sdBox(vec3(s.x - popX, p.y - (popY - 0.055), p.z - (popZ - popL)),
                      vec3(popHW*1.03, 0.055, popL*1.03));
    shell = max(shell, -(rec + (1.0 - popUp)*0.5));

    // ---- the hatch lip ----
    // Scaled to nothing rather than switched off, and the nothing is parked
    // inside the bodywork where min() can never see it.
    float sw = max(gW*0.90*spoil, 0.001), sh = max(0.026*spoil, 0.001), sl = max(0.075*spoil, 0.001);
    shell = min(shell, sdBox(p - vec3(0.0, bBelt + sh - 0.060*(1.0 - spoil), backZ - 0.045), vec3(sw, sh, sl)));

    // ---- door mirrors ----
    shell = min(shell, sdBox(vec3(s.x - (bWid + 0.030), p.y - (bBelt + 0.050), p.z - (cowlZ - 0.14)),
                             vec3(0.048, 0.034, 0.070)));

    // ---- the arches, cut with the cylinders the wheels stand in ----
    vec3 aw = vec3(s.x, p.y, abs(p.z));
    float arch = sdWheel(vec3(aw.x - wtrack - 0.30, p.y - wrad*0.99, aw.z - wbase), wrad + 0.056, 0.55);
    shell = max(shell, -arch);

    vec2 r = vec2(shell, 2.0);

    // The rear pair can share one mirrored evaluation. The front pair cannot,
    // because they are STEERED and both turn the same way in the world - fold
    // them onto |x| and the near wheel gets the opposite lock to the far one.
    float wh = sdWheel(vec3(aw.x - wtrack, p.y - wrad, p.z + wbase), wrad, wwid);
    float cs = cos(carSteer), ss = sin(carSteer);
    vec3 fl = vec3(p.x + wtrack, p.y - wrad, p.z - wbase);
    vec3 fr = vec3(p.x - wtrack, p.y - wrad, p.z - wbase);
    wh = min(wh, sdWheel(vec3(cs*fl.x - ss*fl.z, fl.y, ss*fl.x + cs*fl.z), wrad, wwid));
    wh = min(wh, sdWheel(vec3(cs*fr.x - ss*fr.z, fr.y, ss*fr.x + cs*fr.z), wrad, wwid));
    if (wh < r.x) r = vec2(wh, 4.0);

    vec3 pl = podLocal(p);
    float pod = sdBox(pl - vec3(0.0, -popH - popSink, -popL), vec3(popHW, popH, popL));
    if (pod < r.x) r = vec2(pod, 5.0);

    return r;
}

// The car's whole attitude is a transform on the RAY: the sample is pushed
// backward through the yaw, the roll, the pitch and a volume-preserving squash
// before the body is evaluated, and the distance that comes back is multiplied
// by the SMALLEST scale factor - which is what keeps a non-uniformly scaled
// field conservative and the sphere trace legal.
vec3 toCar(vec3 p){
    vec3 q = p - carP;
    float cy = cos(carYaw), sy = sin(carYaw);
    q = vec3(cy*q.x - sy*q.z, q.y, sy*q.x + cy*q.z);
    float cr = cos(carRoll), sr = sin(carRoll);
    q = vec3(cr*q.x + sr*q.y, -sr*q.x + cr*q.y, q.z);
    float cp = cos(carPitch), sp = sin(carPitch);
    q = vec3(q.x, cp*q.y + sp*q.z, -sp*q.y + cp*q.z);
    return q/dScale;
}

// A normal goes the other way under a scale: the inverse transpose of 1/S is
// S, so it MULTIPLIES by the factors the position divides by.
vec3 toCarN(vec3 n){
    float cy = cos(carYaw), sy = sin(carYaw);
    vec3 q = vec3(cy*n.x - sy*n.z, n.y, sy*n.x + cy*n.z);
    float cr = cos(carRoll), sr = sin(carRoll);
    q = vec3(cr*q.x + sr*q.y, -sr*q.x + cr*q.y, q.z);
    float cp = cos(carPitch), sp = sin(carPitch);
    q = vec3(q.x, cp*q.y + sp*q.z, -sp*q.y + cp*q.z);
    return normalize(q*dScale);
}

// and back out again, for hanging the headlamps on the car
vec3 fromCar(vec3 q){
    float cp = cos(carPitch), sp = sin(carPitch);
    vec3 p = vec3(q.x, cp*q.y - sp*q.z, sp*q.y + cp*q.z);
    float cr = cos(carRoll), sr = sin(carRoll);
    p = vec3(cr*p.x - sr*p.y, sr*p.x + cr*p.y, p.z);
    float cy = cos(carYaw), sy = sin(carYaw);
    return vec3(cy*p.x + sy*p.z, p.y, -sy*p.x + cy*p.z) + carP;
}

vec2 carWorld(vec3 p){
    vec2 r = carSDF(toCar(p));
    r.x *= min(dScale.x, min(dScale.y, dScale.z));
    return r;
}

// .y: 1 road and mountain, 2 shell, 4 wheel, 5 lamp pod, 6 guardrail
vec2 map(vec3 p){
    vec3 q = bend(p);
    vec2 r = vec2(crossSec(q.xy)*gBend, 1.0);
    float rl = railSDF(q)*gBend;
    if (rl < r.x) r = vec2(rl, 6.0);
    vec2 c = carWorld(p);
    if (c.x < r.x) r = c;
    return r;
}

vec3 calcN(vec3 p){
    vec2 e = vec2(1.0, -1.0)*0.0015;
    return normalize(e.xyy*map(p + e.xyy).x + e.yyx*map(p + e.yyx).x
                   + e.yxy*map(p + e.yxy).x + e.xxx*map(p + e.xxx).x);
}

// ---------------------------------------------------------------
// the lights
// ---------------------------------------------------------------
// A low beam is not a cone. It is wide, it is flat, and it has a CUTOFF along
// the top so it lights the road and not the oncoming driver - which is most of
// why a pair of them reads as headlamps rather than as two torches.
float beam(vec3 p, vec3 lp){
    vec3 d = p - lp;
    float r = length(d);
    vec3 dir = d/max(r, 0.0001);
    float f = dot(dir, lampF);
    if (f < 0.02) return 0.0;
    vec2 a = vec2(dot(dir, lampS), dot(dir, lampU))/f;
    float e = length(a/(vec2(0.62, 0.26)*uSpread));
    float cone = smoothstep(1.0, 0.30, e);
    cone *= 1.0 - smoothstep(0.045, 0.21, a.y);          // the cutoff
    return cone/(1.0 + 0.030*r*r);
}

float tailGlow(vec3 p){
    float r = length(p - tailP);
    return 1.0/(1.0 + 1.6*r*r);
}

// The tyre smoke, written in the ROAD's frame: the trail is then a straight
// line behind the car whatever the corner is doing, and it costs one bend.
float smokeD(vec3 p){
    vec3 q = bend(p);
    float a = -q.z;                                  // metres behind the car
    if (a < 0.0 || a > 17.0) return 0.0;
    float w = 0.40 + 0.155*a;                        // it spreads as it goes
    float u = q.x - (carLane + 0.55 + 0.085*a);
    float v = q.y - (0.36 + 0.055*a);
    float d = length(vec2(u, v))/w;
    float body = smoothstep(1.0, 0.15, d)*smoothstep(16.0, 5.0, a)*smoothstep(0.9, 3.4, a);
    float n = vnoise(vec2(nz(zRoad(q.z)*0.60, 4096.0) - gT*2.2, (u*1.4 + v*2.0) + a*0.3));
    return body*(0.45 + 1.30*n)*uSmoke*smoothstep(0.02, 0.17, abs(carSteer));
}

// A spark is a few centimetres of white-hot steel travelling at road speed. As
// a DENSITY in the volumetric it was invisible - a spray forty centimetres
// across, sampled by sixteen taps three metres apart, is missed by almost every
// ray - so the sparks are twelve ballistic PARTICLES instead, each drawn as the
// segment it swept since the last frame, glowed by the closest distance from
// the view ray to that segment. No integration, no density, and it is exactly
// the shape a spark is: a short bright streak with a hot head.
//
// The world here is car-fixed - the car sits at z = 0 and the pass flows past -
// so a spark shed at the barrier travels BACKWARD at about road speed, which is
// what gives the streaks their length without any motion blur.
float raySeg(vec3 ro, vec3 rd, vec3 a, vec3 b, out float th){
    vec3 ba = b - a, w0 = ro - a;
    float B = dot(rd, ba), C = dot(ba, ba), D = dot(rd, w0), E = dot(ba, w0);
    float den = max(C - B*B, 0.00001);
    float sc = clamp((E - B*D)/den, 0.0, 1.0);
    th = max(B*sc - D, 0.0);
    return length(ro + rd*th - (a + ba*sc));
}

vec3 sparks(vec3 ro, vec3 rd, float tEnd){
    if (gScrape < 0.01) return vec3(0.0);
    // a right-handed frame on the road at the contact: where the car is going,
    // where the barrier is, and up
    float ch = cos(gHead), sh = sin(gHead);
    vec3 fwd = vec3(sh, 0.0, ch), sid = vec3(ch, 0.0, -sh), upv = vec3(0.0, 1.0, 0.0);
    vec3 acc = vec3(0.0);
    for (int i = 0; i < 18; i++){
        float fi = float(i);
        float ph = gT*6.5 + fi*3.77;
        float sd = floor(ph);
        float h1 = hash12(vec2(sd, fi + 0.5));
        float h2 = hash12(vec2(sd + 17.0, fi*2.3 + 1.1));
        float h3 = hash12(vec2(sd + 41.0, fi*1.7 + 4.4));
        float age = fract(ph)*0.30;                       // seconds since it was shed
        // shed backward at road speed, in off the barrier, and up a little
        vec3 vel = -fwd*(uSpeed*(0.55 + 0.45*h1))
                 - sid*(0.6 + 2.4*h2)
                 + upv*(0.9 + 2.6*h3);
        vec3 pa = gSparkP + vel*age - upv*(4.9*age*age);
        float a2 = max(age - 0.022, 0.0);
        vec3 pb = gSparkP + vel*a2 - upv*(4.9*a2*a2);
        float th;
        float d = raySeg(ro, rd, pa, pb, th);
        if (th > tEnd + 0.4) continue;                    // behind something solid
        float life = 1.0 - age/0.30;
        float rad = max(0.022, 2.4*gPixK*th);
        vec3 hot = mix(vec3(1.00, 0.52, 0.10), vec3(1.00, 0.95, 0.80), life*life);
        float g = rad/(rad + d);
        acc += hot*life*life*(0.85 + 0.6*h2)*3.4*g*g*g;
    }
    // and the contact itself, which is brighter than any of them
    vec3 oc = gSparkP - ro;
    float tc = dot(oc, rd);
    float dc = length(oc - rd*max(tc, 0.0));
    float rc = max(0.055, 3.2*gPixK*max(tc, 0.0));
    float gc = rc/(rc + dc);
    acc += gSparkC*step(0.0, tc)*step(tc, tEnd + 0.4)*2.4*gc*gc*gc;
    return acc*gScrape;
}

// ---------------------------------------------------------------
vec3 sky(vec3 rd){
    float h = rd.y;
    vec3 col = mix(gHor, gSky, smoothstep(-0.02, 0.55, h));
    float az = atan(rd.x, rd.z);

    // stars, on a grid fine enough that two rarely share a cell
    vec2 sc = vec2(az*130.0, rd.y*130.0);
    vec2 ci = floor(sc);
    vec2 off = 0.35*vec2(hash12(ci + 3.1) - 0.5, hash12(ci + 7.7) - 0.5);
    float star = step(0.974, hash12(ci))*smoothstep(0.26, 0.02, length(fract(sc) - 0.5 - off));
    col += vec3(0.85, 0.90, 1.0)*star*uStars*smoothstep(0.02, 0.22, h)*(0.20 + 1.45*hash12(ci + 11.0));

    // the moon, and the halo it puts on the haze
    float m = dot(rd, gMoonD);
    col += gMoonC*(smoothstep(0.99930, 0.99970, m)*1.9 + 0.22*pow(max(m, 0.0), 90.0))*uMoon;

    // two ridge lines, the near one darker, standing above the fog
    float r1 = 0.105 + 0.085*vnoise(vec2(az*2.1 + 4.0, 0.5)) + 0.035*vnoise(vec2(az*7.0, 1.5));
    float r2 = 0.045 + 0.065*vnoise(vec2(az*1.4 + 9.0, 2.5));
    col = mix(col, gHor*0.55, smoothstep(r1 + 0.008, r1 - 0.008, h));
    col = mix(col, gHor*0.20, smoothstep(r2 + 0.006, r2 - 0.006, h));

    // The town on the valley floor, a long way down and a long way off.
    // A grid of lit cell centres is a LATTICE and reads as one instantly, so
    // each light is jittered inside its own cell, and a slow noise gathers them
    // into a town instead of spreading them evenly along the whole valley.
    float tl = smoothstep(-0.020, -0.042, h)*smoothstep(-0.150, -0.105, h);
    vec2 tc = vec2(az*150.0, h*900.0);
    vec2 tf = floor(tc);
    vec2 tj = vec2(hash12(tf + 1.7), hash12(tf + 5.3)) - 0.5;
    float clump = smoothstep(0.35, 0.72, vnoise(vec2(az*7.0, 3.0)));
    float td = step(0.88 - 0.10*clump, hash12(tf))*smoothstep(0.32, 0.05, length(fract(tc) - 0.5 - tj*0.7));
    col += vec3(1.00, 0.68, 0.34)*td*tl*uTown*(0.22 + 0.55*hash12(tf + 9.1));
    return col;
}

vec3 shade(vec3 p, vec3 rd, vec3 n, float mat){
    vec3 alb = vec3(0.5);
    float gloss = 0.0, metal = 0.0;
    vec3 emis = vec3(0.0);

    if (mat < 1.5){
        // ---- the road, and the mountain it is cut into ----
        vec3 q = bend(p);
        float u = q.x;
        float zr = zRoad(q.z);
        float agg = vnoise(vec2(u*26.0, nz(zr*26.0, 4096.0)))*0.6
                  + vnoise(vec2(u*95.0, nz(zr*95.0, 4096.0)))*0.4;
        alb = vec3(0.052, 0.052, 0.056)*(0.72 + 0.62*agg);
        gloss = 0.30 + 0.34*agg;

        // the cut face above the gutter: rock, and the concrete frames the
        // engineers put on every mountain road in Japan
        float rock = smoothstep(0.02, 0.30, q.y - 0.10)*step(u, -3.0);
        float grid = max(smoothstep(0.06, 0.0, abs(fract(zr*0.40) - 0.5) - 0.42),
                         smoothstep(0.06, 0.0, abs(fract(q.y*0.36) - 0.5) - 0.42));
        vec3 rockC = mix(vec3(0.088, 0.082, 0.070), vec3(0.055, 0.060, 0.048), vnoise(vec2(nz(zr*1.7, 4096.0), q.y*1.7)));
        alb = mix(alb, mix(rockC, rockC*1.9, grid*step(q.y, 6.0)), rock);
        gloss = mix(gloss, 0.05, rock);

        // the gutter, and the shoulder past the white line
        float gut = smoothstep(-2.82, -2.90, u)*step(-3.30, u);
        alb = mix(alb, vec3(0.150, 0.150, 0.145)*(0.75 + 0.5*agg), gut);
        alb = mix(alb, vec3(0.085, 0.080, 0.062)*(0.7 + 0.6*agg), smoothstep(3.02, 3.12, u));

        // The markings. A no-overtaking centre line on a pass like this is
        // YELLOW and the edge lines are white - get that one colour wrong and
        // the road stops being Japanese.
        float onRoad = step(abs(q.y), 0.06);
        float ctr = (1.0 - smoothstep(0.070, 0.090, abs(u - 0.10)))*onRoad;
        float edg = (1.0 - smoothstep(0.055, 0.075, abs(u + 2.66)))
                  + (1.0 - smoothstep(0.055, 0.075, abs(u - 2.90)));
        edg = min(edg, 1.0)*onRoad;
        float wear = 0.55 + 0.45*vnoise(vec2(nz(zr*3.0, 4096.0), u*3.0));
        alb = mix(alb, vec3(0.62, 0.55, 0.13)*wear, ctr);
        alb = mix(alb, vec3(0.60, 0.60, 0.57)*wear, edg);
        gloss = mix(gloss, 0.45, max(ctr, edg));

        // Tar seams across the carriageway every eight metres. A dark line on
        // dark asphalt is invisible standing still and invisible moving; what
        // makes them read is that fresh tar is GLOSSIER than the road around
        // it, so they answer the beam as they go past.
        float seamZ = 1.0 - smoothstep(0.014, 0.034, abs(fract(zr*0.125 + 0.35) - 0.5) - 0.482);
        alb = mix(alb, alb*0.60, seamZ*onRoad);
        gloss = mix(gloss, 0.85, seamZ*onRoad);

        // Cat's eyes down the centre every five metres. A retroreflector sends
        // the light back the way it came and the camera is sitting next to the
        // lamps, so the right model is emission proportional to the BEAM at that
        // point rather than a specular lobe - which is why they flare white in
        // the pool and go out the instant the beam leaves them.
        float ce = step(0.90, fract(zr*0.2))*(1.0 - smoothstep(0.055, 0.085, abs(u - 0.10)))*onRoad;
        alb = mix(alb, vec3(0.80, 0.82, 0.85), ce);
        emis += vec3(1.00, 0.98, 0.90)*ce*4.5*(beam(p, lampL) + beam(p, lampR))*uBeam;
        gloss = mix(gloss, 1.0, ce);
    } else if (mat < 3.5){
        // ---- the shell ----
        // Undo the car's attitude to ask the body where on itself we are,
        // because the trim, the glass and the lamps are fixed to the CAR and
        // not to the road it is sliding down. The normal comes back too, which
        // is what lets a seam know whether it is on a flank or on the bonnet.
        vec3 q  = toCar(p);
        vec3 nb = toCarN(n);
        vec3 s  = vec3(abs(q.x), q.y, q.z);

        // signed distance to the two planes that make the car a car
        float dScr = dot(q - vec3(0.0, bBelt, cowlZ), nScr);
        float dBak = dot(q - vec3(0.0, bBelt, backZ), nBak);
        // the height of the nose panel: the bonnet has been falling for a metre
        // and a half by the time it gets there, and every feature on the front
        // hangs off THIS, not off the beltline
        float noseY = bBelt - (zNose - cowlZ)*0.085;

        alb = gPaint;
        gloss = 0.55 + 0.45*uGloss;
        metal = 0.65*uGloss;

        // ---- panda ----
        // One level plane through the whole body. Masking tape does not care
        // which panel it is crossing, and neither does this.
        float low = 1.0 - step(splitY, q.y);
        alb = mix(alb, vec3(0.048, 0.048, 0.052), low*uPanda);
        gloss = mix(gloss, gloss*0.75, low*uPanda);

        // ---- glass ----
        // Found by distance to the screen and backlight planes, never by z: a
        // surface that IS the windscreen sits at zero from the windscreen
        // plane, and the pillars are the band either side of that zero.
        float gTop = 1.0 - step(bRoof - 0.032, q.y);          // roof header rail
        float gBot = step(bBelt + 0.022, q.y);                // beltline
        float scr  = (1.0 - step(0.014, abs(dScr)))*step(s.x, gW - 0.052);
        float bak  = (1.0 - step(0.014, abs(dBak)))*step(s.x, gW - 0.052);
        float side = step(0.55, abs(nb.x))*step(dScr, -0.065)*step(dBak, -0.065)
                   * step(0.030, abs(q.z - zPill));
        float glass = max(max(scr, bak), side)*gTop*gBot;
        alb = mix(alb, vec3(0.030, 0.034, 0.042), glass);
        gloss = mix(gloss, 1.0, glass);
        metal = mix(metal, 0.10, glass);
        // The B-pillar is black trim, not paint, and the door frames with it -
        // which is why the whole side of the glasshouse reads as one dark band
        // on this car and not as three holes in a white wall.
        float pillar = step(0.55, abs(nb.x))*gTop*gBot
                     * step(dScr, -0.065)*step(dBak, -0.065)*(1.0 - step(0.030, abs(q.z - zPill)));
        alb = mix(alb, vec3(0.072, 0.074, 0.078), pillar);
        gloss = mix(gloss, 0.35, pillar);
        metal = mix(metal, 0.0, pillar);

        // ---- black plastic: bumpers, valances, rocker, mirror shells ----
        float bump = step(zNose - 0.20, q.z)*step(q.y, noseY - 0.155);
        bump = max(bump, step(q.z, zTail + 0.18)*step(q.y, bBelt - 0.300));
        bump = max(bump, step(q.y, bSill + 0.085)*step(0.5, abs(nb.x)));
        bump = max(bump, step(bWid + 0.010, s.x)*step(bBelt - 0.02, q.y));   // mirrors
        alb = mix(alb, vec3(0.052, 0.052, 0.056), bump);
        gloss = mix(gloss, 0.22, bump);
        metal = mix(metal, 0.0, bump);

        // ---- the face ----
        float onNose = step(0.5, nb.z);
        float onTail = step(0.5, -nb.z);
        // the one slot under the bonnet's leading edge - narrow on the Trueno,
        // narrower still on the Levin, which has lamps to either side of it
        float slot = onNose*step(noseY - 0.085, q.y)*step(q.y, noseY - 0.018)
                   * step(s.x, bWid*mix(0.46, 0.20, uFace));
        float duct = onNose*step(noseY - 0.305, q.y)*step(q.y, noseY - 0.200)*step(s.x, bWid*0.70);
        alb = mix(alb, vec3(0.028, 0.028, 0.032), max(slot, duct));
        gloss = mix(gloss, 0.30, max(slot, duct));
        // Levin: four rectangular units where the pods would be.
        float quad = onNose*step(abs(q.y - (noseY - 0.078)), 0.034)
                   * step(0.17, s.x)*step(s.x, bWid*0.86)*step(0.018, abs(s.x - 0.435))*uFace;
        alb = mix(alb, vec3(0.88, 0.87, 0.80), quad);
        emis += vec3(1.00, 0.94, 0.78)*quad*2.6*uBeam;
        gloss = mix(gloss, 1.0, quad);
        // indicators, out at the corners of the bumper
        float ind = onNose*step(noseY - 0.215, q.y)*step(q.y, noseY - 0.135)*step(0.58, s.x);
        alb = mix(alb, vec3(0.85, 0.42, 0.06), ind);
        emis += vec3(0.85, 0.42, 0.06)*ind*0.10;

        // ---- the tail ----
        // Two big clusters and a black garnish between them, which is the whole
        // back of this car - and at night it is most of what anyone behind sees.
        float tY = step(abs(q.y - (bBelt - 0.155)), 0.092);
        float tL = onTail*tY*step(0.125, s.x)*step(s.x, bWid - 0.012);
        float tG = onTail*tY*step(s.x, 0.125);
        alb = mix(alb, vec3(0.34, 0.030, 0.026), tL);
        alb = mix(alb, vec3(0.040, 0.040, 0.044), tG);
        emis += vec3(1.00, 0.12, 0.08)*tL*0.55;
        gloss = mix(gloss, 1.0, tL);
        // number plate, both ends, lit at the back
        float plateY = mix(bBelt - 0.315, noseY - 0.250, onNose);
        float plate = step(abs(q.y - plateY), 0.078)*step(s.x, 0.165)*max(onNose, onTail);
        alb = mix(alb, vec3(0.86, 0.87, 0.83), plate);
        emis += vec3(0.90, 0.90, 0.80)*plate*onTail*0.07;
        gloss = mix(gloss, 0.40, plate);

        // ---- shut lines ----
        float flank = step(0.55, abs(nb.x));
        float upper = step(0.55, nb.y);
        float seam = 0.0;
        seam = max(seam, flank*(1.0 - smoothstep(0.006, 0.013, abs(q.z - (cowlZ - 0.02))))*step(q.y, bBelt));
        seam = max(seam, flank*(1.0 - smoothstep(0.006, 0.013, abs(q.z - zPill)))*step(q.y, bBelt));
        seam = max(seam, upper*(1.0 - smoothstep(0.005, 0.011, abs(s.x - (bWid - 0.075))))*step(cowlZ + 0.02, q.z));
        seam = max(seam, upper*(1.0 - smoothstep(0.005, 0.011, abs(q.z - (cowlZ + 0.03))))*step(s.x, bWid - 0.070));
        seam = max(seam, flank*(1.0 - smoothstep(0.005, 0.011, abs(q.z - (backZ + 0.02))))*step(bBelt, q.y));
        // the shut line round each lamp cover, drawn as a rectangle: an edge in
        // one axis wherever the other axis is still inside the footprint
        vec2 pf = vec2(abs(s.x - popX) - popHW, abs(q.z - (popZ - popL)) - popL);
        seam = max(seam, upper*(1.0 - popUp)*(1.0 - uFace)
                       * max((1.0 - smoothstep(0.005, 0.011, abs(pf.x)))*step(pf.y, 0.0),
                             (1.0 - smoothstep(0.005, 0.011, abs(pf.y)))*step(pf.x, 0.0)));
        alb *= 1.0 - 0.55*seam*(1.0 - glass);

        // a rub strip along the flank, sitting on the two-tone split
        float strip = flank*(1.0 - smoothstep(0.012, 0.026, abs(q.y - (splitY + 0.030))))
                    * step(q.z, zNose - 0.16)*step(zTail + 0.14, q.z);
        alb = mix(alb, vec3(0.055, 0.055, 0.058), strip);
        gloss = mix(gloss, 0.25, strip);
    } else if (mat < 4.5){
        // ---- wheels ----
        vec3 q = toCar(p);
        float rr = length(vec2(q.y - wrad, abs(q.z) - wbase));
        float face = smoothstep(wrad*0.70, wrad*0.62, rr);          // rim, inside the tyre
        alb = mix(vec3(0.030, 0.030, 0.034),
                  mix(vec3(0.40, 0.41, 0.43), vec3(0.66, 0.67, 0.70), uTyre), face);
        // eight spokes, or five holes in a steel wheel - and they TURN, which
        // costs one addition because the pattern is drawn in polar anyway
        float a = atan(q.y - wrad, abs(q.z) - wbase) + carSpin;
        float petal = mix(abs(sin(a*2.5)), abs(sin(a*4.0)), step(0.5, uWheel));
        float gap = smoothstep(0.62, 0.86, petal)*face*step(rr, wrad*0.52)*step(wrad*0.20, rr);
        alb = mix(alb, vec3(0.085, 0.085, 0.095), gap);
        alb = mix(alb, vec3(0.30, 0.30, 0.32), smoothstep(wrad*0.20, wrad*0.16, rr));   // the hub
        gloss = 0.14 + 0.60*face;
        metal = 0.55*face;
    } else if (mat < 5.5){
        // ---- the pop-up pods ----
        // The lens only exists on the face that comes round to point forward,
        // which is exactly why raising them is worth doing as geometry.
        vec3 nw = toCarN(n);
        float c = cos(popA), sn = sin(popA);
        vec3 nl = vec3(nw.x, nw.z*sn + nw.y*c, nw.z*c - nw.y*sn);
        float lens = step(0.55, nl.y);
        alb = mix(gPaint, vec3(0.95, 0.93, 0.86), lens);
        alb = mix(alb, vec3(0.045, 0.045, 0.050), step(0.55, -nl.y));   // the underside
        emis += vec3(1.00, 0.95, 0.80)*lens*popUp*3.4*uBeam;
        gloss = mix(0.55 + 0.45*uGloss, 1.0, lens);
        metal = mix(0.65*uGloss, 0.05, lens);
    } else {
        // ---- the guardrail, and the delineator poles behind it ----
        vec3 q = bend(p);
        float zg = zRoad(q.z);
        alb = vec3(0.30, 0.31, 0.32)*(0.55 + 0.70*vnoise(vec2(nz(zg*6.0, 4096.0), q.y*6.0)));
        alb = mix(alb, vec3(0.22, 0.13, 0.08), 0.5*vnoise(vec2(nz(zg*2.2, 4096.0) + 9.0, q.y*3.0)));   // rust
        gloss = 0.55;
        metal = 0.45;
        // the poles stand 30 cm further out than the rail ever does, so one
        // threshold tells them apart without a second material id
        float isPole = step(3.62, q.x);
        alb = mix(alb, vec3(0.62, 0.62, 0.60), isPole);
        float refl = isPole*step(abs(q.y - 0.755), 0.055);
        alb = mix(alb, vec3(0.55, 0.20, 0.06), refl);
        emis += vec3(1.00, 0.42, 0.10)*refl*5.5*(beam(p, lampL) + beam(p, lampR))*uBeam;
        gloss = mix(gloss, 0.25, isPole*(1.0 - refl));
        metal = mix(metal, 0.0, isPole);
    }

    // ---- the light: a moon, two headlamps, and the red off the back ----
    vec3 col = alb*gMoonC*(0.10 + 0.45*max(dot(n, gMoonD), 0.0))*uMoon;

    vec3 dL = lampL - p, dR = lampR - p;
    col += alb*gLampC*uBeam*3.3*(beam(p, lampL)*max(dot(n, normalize(dL)), 0.0)
                               + beam(p, lampR)*max(dot(n, normalize(dR)), 0.0));

    // the tail lamps wash the road behind, and it is the only red in the frame
    col += alb*gTailC*tailGlow(p)*max(dot(n, normalize(tailP - p)), 0.0)*1.3;

    vec3 dS = gSparkP - p;
    col += alb*gSparkC*gScrape*2.6*max(dot(n, normalize(dS)), 0.0)/(1.0 + 3.5*dot(dS, dS));

    vec3 dB = fromCar(vec3(0.0, 0.06, 9.0)) - p;
    col += alb*gLampC*uBeam*0.55*max(dot(n, normalize(dB)), 0.0)/(1.0 + 0.045*dot(dB, dB));

    // the sky is a light too, and at night it is most of what a horizontal
    // surface gets when the beams are pointing somewhere else
    col += alb*mix(gHor, gSky, clamp(n.y*0.5 + 0.5, 0.0, 1.0))*(0.45 + 0.30*uMoon);
    col += emis;

    if (gloss > 0.02){
        vec3 rf = reflect(rd, n);
        float fres = 0.04 + 0.96*pow(1.0 - max(dot(-rd, n), 0.0), 5.0);
        vec3 env = sky(rf)*0.55 + gLampC*uBeam*(beam(p + rf*0.6, lampL) + beam(p + rf*0.6, lampR))*0.8;
        col = mix(col, env*mix(vec3(1.0), alb, metal), fres*gloss*(0.35 + 0.65*metal + 0.25));
        // a specular off each lamp, which is what makes night asphalt read wet
        float sp = pow(max(dot(n, normalize(normalize(dL) - rd)), 0.0), mix(18.0, 220.0, gloss))*beam(p, lampL)
                 + pow(max(dot(n, normalize(normalize(dR) - rd)), 0.0), mix(18.0, 220.0, gloss))*beam(p, lampR);
        col += gLampC*sp*gloss*uBeam*2.2;
    }
    return col;
}

// ---------------------------------------------------------------
void main(){
    vec2 R = RENDERSIZE.xy;
    // The pixelation is the render: the coordinate is quantised BEFORE the ray
    // is built, so every chunky pixel is a real sample of the geometry.
    float sc = max(R.y/max(uRes, 16.0), 1.0);
    vec2 fc = (floor(gl_FragCoord.xy/sc) + 0.5)*sc;
    vec2 bp = mod(floor(gl_FragCoord.xy/max(sc*0.5, 1.0)), 4.0);
    float bayer = mod(bp.x*5.0 + bp.y*3.0 + bp.x*bp.y*2.0, 16.0)/16.0;
    // The run starts part way down the pass rather than at the top of it: t = 0
    // is then a corner with the car already sideways and the barrier coming up,
    // which is what an ISF host shows you the moment it loads the file.
    float T = TIME + 115.38;
    gT = T;
    gPassF = 0.0;

    gSky   = vec3(0.020, 0.030, 0.062);
    gHor   = vec3(0.048, 0.060, 0.098);
    gFog   = vec3(0.030, 0.038, 0.062);
    gMoonC = vec3(0.52, 0.62, 0.85);
    gMoonD = normalize(vec3(-0.55, 0.42, -0.72));
    gLampC = vec3(1.00, 0.94, 0.80);
    gPaint = vec3(uPaintR, uPaintG, uPaintB);
    gTailC = vec3(1.00, 0.10, 0.06);

    // ---- the pass ----
    gCurv  = uCurve;
    gGrade = uGrade;
    gBank  = 2.2;
    gZc    = uSpeed*T;                       // how far down the hill we have got
    // Half a frame of travel, dithered across the pixel grid: the ground smears
    // along the direction it is going and the car, which is not moving relative
    // to the camera, stays sharp. That asymmetry is exactly the cue that says
    // one of these things is moving and the other is not.
    gJit   = (bayer - 0.5)*uBlur*uSpeed*0.030;
    gX0    = rdX(gZc);
    gY0    = rdY(gZc);
    // The shear is not an isometry, so the field it returns over-estimates by
    // at most the largest stretch the displacement can apply. Bound it once,
    // from the amplitudes themselves - a bound taken at the sample would be
    // smaller, and wrong wherever the ray is heading somewhere steeper.
    float mdx = gCurv*0.6248, mdy = gGrade + 0.0393;
    gBend = inversesqrt(1.0 + mdx*mdx + mdy*mdy);

    // ---- the car, in millimetres off the real one ----
    // 4180 x 1625 x 1335 on a 2400 wheelbase, 185/60R14, and the overhangs
    // UNEQUAL - 830 in front, 970 behind. A body box centred on the axles is
    // what makes a model car look like a toy, and it is the cheapest mistake in
    // the file to avoid: two numbers instead of one.
    float coupe = clamp(uBody, 0.0, 1.0);
    wbase  = 1.200;
    zNose  = wbase + 0.830;
    zTail  = -(wbase + 0.970);
    bWid   = 0.8125;
    bSill  = 0.300;
    bBelt  = 0.882;
    bRoof  = 1.335;
    bHgt   = 0.5*(bSill + bBelt);
    bHlf   = 0.5*(bBelt - bSill);
    gYc    = 0.5*(bBelt + bRoof);
    gHgt   = 0.5*(bRoof - bBelt);
    gW     = bWid - 0.058;
    splitY = 0.615;
    wtrack = 0.677;
    wrad   = 0.289;
    wwid   = 0.0925;

    // The screen and the backlight ARE the silhouette. Both are written as a
    // rake angle off vertical and turned into a plane normal here, so the
    // number in the file is the number you would measure on the car.
    cowlZ  = 0.600;
    backZ  = mix(-1.960, -1.420, coupe);
    zPill  = mix(-0.560, -0.520, coupe);
    float aScr = radians(49.0);
    float aBak = radians(mix(62.0, 44.0, coupe));
    nScr = vec3(0.0, sin(aScr),  cos(aScr));
    nBak = vec3(0.0, sin(aBak), -cos(aBak));
    nBon = normalize(vec3(0.0, 1.0, 0.085));

    spoil = clamp(uSpoiler, 0.0, 1.0);

    // ---- the pop-ups ----
    // Sunk six millimetres when closed, so the body always wins the tie and a
    // shut bonnet is one unbroken surface.
    popX  = 0.410;
    popHW = 0.240;
    popL  = 0.088;
    popH  = 0.055;
    popZ  = zNose - 0.175;
    popY  = bBelt - (popZ - cowlZ)*0.085;
    popUp = clamp(uPopup, 0.0, 1.0)*(1.0 - clamp(uFace, 0.0, 1.0));
    // Closed, the pod's top face is COPLANAR with the bonnet: the hinge sits on
    // that surface and the rest angle is exactly the bonnet's own fall, so the
    // far end rises by the same amount the bonnet does.
    popA  = popUp*1.02 + atan(0.085);
    popSink = 0.006*(1.0 - popUp);

    // ---- the drift ----
    // It is not an animation. The car's yaw is the road's own heading plus a
    // term proportional to its CURVATURE, so the car points into the corner
    // wherever the corner happens to be - and everything else follows from that
    // one number: the front wheels counter-steer against it, the body leans on
    // it, the line runs wide on it and the smoke comes off it.
    float head = atan(rdXd(gZc));                    // where the road is going
    gHead = head;
    float curv = rdXdd(gZc);
    // A car does not stop being sideways at the apex. Half the drift angle
    // comes from the curvature the car is IN and half from the curvature it was
    // in twelve metres ago, which is what makes it hang the angle out of the
    // corner instead of snapping straight the moment the road does.
    float curvD = 0.45*curv + 0.55*rdXdd(gZc - 12.0);
    float wob  = 0.075*sin(T*1.9) + 0.045*sin(T*3.7 + 1.1);
    float slip = clamp(uDrift*(21.0*curvD + wob), -0.95, 0.95);
    carYaw   = head + slip;
    carSteer = clamp(-0.95*slip, -0.62, 0.62);       // opposite lock, to the stop
    float wideT = -1.25 - uDrift*20.0*curvD;         // the line it wants
    // Every twenty-odd seconds the line runs all the way out and the rear
    // quarter leans on the barrier. It is written as a rare, brief peak in the
    // ROAD rather than in the clock, so the same corner does it every time.
    float kiss = pow(max(sin(gZc*0.0172 + 1.3), 0.0), 8.0);
    wideT += uSpark*kiss*3.4;
    // The car has to stay ON the road, and a car at forty degrees is using far
    // more of it than its own width: the swept half-extent of a yawed box is
    // halfW*|cos| + halfL*|sin|, which at that angle is two metres of a road
    // six wide. Clamp the LINE to that and the car can never clip the cut face
    // or the barrier however hard it is sideways - and it is the drift angle
    // itself that decides how wide it is allowed to run. The upper stop is the
    // INNER FACE of the guardrail, so running into it parks the car against the
    // barrier rather than through it, and how hard it is pressing is how far
    // past the stop it wanted to be.
    float swept = min(bWid*abs(cos(slip)) + 2.10*abs(sin(slip)) + 0.14, 2.80);
    float hiL = 3.40 - swept;
    carLane  = clamp(wideT, -2.86 + swept, hiL);
    gScrape  = smoothstep(0.03, 0.60, wideT - hiL)*step(0.01, uSpark);

    float bk = gBank*curv;
    float bumps = uBumps*(0.016*vnoise(vec2(gZc*0.9, 0.0)) + 0.009*sin(gZc*2.7));
    carRoll  = bk - uLean*(2.9*curv + 0.020*sin(T*1.6));
    carPitch = uLean*0.035*sin(T*0.77) + 0.5*bumps;
    carP     = vec3(carLane, carLane*tan(bk) + 0.004 + bumps, 0.0);
    carSpin  = -gZc/wrad;

    // Volume preserving: squash the height by k and the horizontals go as one
    // over root k. That relationship is the whole of why a body working on its
    // springs reads as having mass in it rather than being resized.
    float k = 1.0 + uBumps*(0.020*sin(T*5.1) + 0.014*sin(gZc*1.9));
    k = max(k, 0.35);
    dScale = vec3(inversesqrt(k), k, inversesqrt(k));

    // ---- the headlamps, hung on the car itself ----
    lampL = fromCar(vec3(-0.410, popY + 0.055, popZ - 0.02));
    lampR = fromCar(vec3( 0.410, popY + 0.055, popZ - 0.02));
    vec3 o0 = fromCar(vec3(0.0, 0.0, 0.0));
    lampF = normalize(fromCar(vec3(0.0, -0.075, 1.0)) - o0);
    lampS = normalize(fromCar(vec3(1.0, 0.0, 0.0)) - o0);
    lampU = normalize(cross(lampS, lampF));
    tailP = fromCar(vec3(0.0, bBelt - 0.155, zTail + 0.02));
    gSparkP = fromCar(vec3(bWid*0.92, 0.52, mix(zTail, zNose, step(0.0, slip))));
    gSparkB = bend(gSparkP);
    gSparkC = vec3(1.00, 0.56, 0.15);

    // ---- the camera ----
    // A fixed chase is a locked-off camera, and this is a car chase. With uCut
    // on, the frame cuts every uCut seconds to a shot drawn from a hash of the
    // cut index - front three-quarter, over the cut face, chase, down at wheel
    // height, head on, or a roadside pass-by where the camera stops following
    // and the car comes to it. Every shot also MOVES inside itself, because a
    // cut from one locked-off camera to another is still locked off.
    float side = uCamSide, back = uCamBack, hgt = uCamH, lens = uLens, pass = 0.0;
    if (uCut > 0.05){
        float ci = floor(T/uCut), f = fract(T/uCut);
        // squaring the draw biases it toward the low indices, and the table is
        // ordered so those are the ones looking at the FRONT of the car
        float hh = hash12(vec2(ci, 0.7));
        float k = floor(hh*hh*6.0);
        vec4 sh = vec4( 2.62,  7.0, 0.80, 1.25);                    // front 3/4, low
        sh = mix(sh, vec4( 3.05, 11.0, 1.10, 2.00), step(0.5, k));  // head on
        sh = mix(sh, vec4(-2.35,  7.0, 1.10, 1.35), step(1.5, k));  // front 3/4, off the bank
        sh = mix(sh, vec4( 0.38,  9.2, 2.10, 1.60), step(2.5, k));  // the chase
        sh = mix(sh, vec4( 1.45,  4.0, 0.46, 1.20), step(3.5, k));  // down at wheel height
        sh = mix(sh, vec4( 0.00, 26.0, 0.95, 1.60), step(4.5, k));  // the pass-by
        pass = step(4.5, k);
        float dir = sign(hash12(vec2(ci, 3.1)) - 0.5);
        side = sh.x + dir*0.20*(f - 0.5);
        back = sh.y*(1.0 + dir*0.13*(f - 0.5));
        hgt  = sh.z*(1.0 - 0.22*(f - 0.5));
        lens = sh.w;
        // the pass-by: the camera stands beside the road and the car goes by it
        side = mix(side, 0.0, pass);
        back = mix(back, 1.0, pass);
        gPassF = f;
    }
    float cz = mix(-cos(side)*back, mix(15.0, -14.0, gPassF), pass);
    float cx = mix( sin(side)*back, 2.90, pass);
    // The camera stands on the ROAD, not on the car. Offsetting it from the
    // car's own line looked right until the line started running all the way
    // out to the barrier, at which point every side shot was dragged out over
    // the drop with the rail across the lens. Anchored to the road, the car
    // moves WITHIN the frame instead - which is a second thing telling you it
    // is going somewhere.
    float camU = -1.25 + cx;
    // it can still end up outside the rail, and out there it has to see over it
    hgt += smoothstep(2.9, 3.9, camU)*1.35;
    vec3 ro  = vec3(rx(cz) + camU, ry(cz) + hgt, cz);
    // the camera orbits into the mountain at negative angles: rather than clamp
    // the slider, let it CLIMB - one cross-section query says how far inside the
    // rock it is and it rises by that much plus clearance
    ro.y += max(0.55 - crossSec(bend(ro).xy), 0.0)*1.9;
    ro.y += max(0.55 - crossSec(bend(ro).xy), 0.0)*1.9;
    ro.y += max(0.55 - crossSec(bend(ro).xy), 0.0)*1.9;
    vec3 tgt = vec3(rx(1.0) + carLane, ry(1.0) + 0.76, 1.0);
    vec3 fw  = normalize(tgt - ro);
    vec3 rt  = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
    vec3 up  = cross(fw, rt);
    vec2 uv  = (fc - 0.5*R)/R.y;
    vec3 rd  = normalize(uv.x*rt + uv.y*up + lens*fw);
    gPixK    = 1.0/(lens*max(uRes, 16.0));   // world metres per pixel, per metre of depth

    float tt = 0.05, tHit = FAR, mat = 0.0;
    for (int i = 0; i < 128; i++){
        vec2 m = map(ro + rd*tt);
        if (m.x < 0.0012*tt + 0.0007){ tHit = tt; mat = m.y; break; }
        tt += m.x*0.90;
        if (tt > FAR) break;
    }

    float tEnd = min(tHit, FAR);
    vec3 col;
    if (mat > 0.5){
        vec3 p = ro + rd*tEnd;
        col = shade(p, rd, calcN(p), mat);
        // the haze eats the pass by about fifty metres, which is the whole
        // reason a night scene can be this cheap
        col = mix(gFog, col, exp(-tEnd*0.0175*(0.4 + 0.6*uHaze)));
    } else {
        col = sky(rd);
    }

    // ---- the air itself ----
    // Sixteen taps down the ray, gathering the beams, the tail-light red and
    // the smoke off the rear tyres. No map() in here: the only occluder that
    // matters is the car, and the beams start in front of it.
    float tv = min(tEnd, 46.0), dt = tv/16.0;
    vec3 vol = vec3(0.0);
    for (int i = 0; i < 16; i++){
        vec3 p = ro + rd*((float(i) + bayer)*dt);
        vec3 beams = gLampC*uBeam*(beam(p, lampL) + beam(p, lampR));
        // The haze and the smoke are lit differently and it matters: the haze
        // is mostly beam, and the SMOKE is behind the car where no beam ever
        // reaches, so the only things on it are the moon and the tail lamps.
        // That is why tyre smoke at night is grey with a red heart.
        vol += (beams*1.25 + gTailC*tailGlow(p)*0.55 + gMoonC*0.30*uMoon)*(0.0076*uHaze)*dt;
        vol += (gMoonC*1.00*uMoon + gTailC*tailGlow(p)*1.7 + beams*0.50)*smokeD(p)*0.155*dt;
    }
    col += vol;
    col += sparks(ro, rd, tEnd)*uSpark;

    // ---- the framebuffer ----
    col = col*1.22;
    col = col/(1.0 + col*0.55);
    vec2 sp = fc/R;
    col *= 1.0 - 0.38*dot(sp - 0.5, sp - 0.5);

    float levels = pow(2.0, floor(uBits + 0.5)) - 1.0;
    col += (bayer - 0.5)*uDither/levels;
    col = floor(clamp(col, 0.0, 1.0)*levels + 0.5)/levels;

    col += (hash12(gl_FragCoord.xy + floor(T*24.0)*vec2(13.7, 91.3)) - 0.5)*uGrain*0.06;
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}

