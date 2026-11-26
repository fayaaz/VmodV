/*{
  "DESCRIPTION": "Running for your life through a black forest at night, first person. The path winds between tall dead trunks that vanish up into fog; a torch bounces in your hand and throws a hard beam through the mist, moonlight leaks through holes in a canopy you never see. Every few seconds you can't help it: you whip round over your shoulder -- and it is still there, a tall gaunt thing loping after you down the path with its arms reaching out, eyes catching the light. Then back to the path. The camera is a body: every footfall jolts it, the hands shake, the torch flickers when you least want it to, and now and then you stumble. SPEED is how fast you run, SHAKE how hard the camera jolts, LOOK BACK how often you check behind you, CLOSENESS how near it has got, TORCH whether you have a light, FLICKER how bad the batteries are, FOG the mist, MOON the moonlight, TREES how thick the forest is, BLUR the smear of a panicked turn, GRAIN the film, EXPOSURE the overall light. It all comes off a worn camcorder tape: VHS sets how chewed up it is -- chroma smeared off the edges, wobbling lines, head-switching noise along the bottom, a tracking band rolling through that tears worst when you look back, dropouts -- and OSD burns in the blinking REC and the running counter.",
  "CREDIT": "CC0",
  "ISFVSN": "2",
  "CATEGORIES": ["generator", "horror", "forest", "raymarch"],
  "INPUTS": [
    { "NAME": "SPEED", "TYPE": "float", "DEFAULT": 14.0, "MIN": 3.0, "MAX": 22.0 },
    { "NAME": "SHAKE", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 2.0 },
    { "NAME": "LOOK_BACK", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0, "LABEL": "look back" },
    { "NAME": "CLOSENESS", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "TORCH", "TYPE": "bool", "DEFAULT": true },
    { "NAME": "FLICKER", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "FOG", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "MOON", "TYPE": "float", "DEFAULT": 0.6, "MIN": 0.0, "MAX": 1.5 },
    { "NAME": "TREES", "TYPE": "float", "DEFAULT": 0.8, "MIN": 0.2, "MAX": 1.0 },
    { "NAME": "BLUR", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 2.0 },
    { "NAME": "GRAIN", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "EXPOSURE", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.3, "MAX": 3.0 },
    { "NAME": "VHS", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 2.0 },
    { "NAME": "OSD", "TYPE": "bool", "DEFAULT": true }
  ],
  "PASSES": [
    { "TARGET": "scene" },
    {}
  ]
}*/

// =============================================================================
//  don't look back -- first person, running through the woods, being chased
// =============================================================================
//
//  THE FOREST. Two offset 8 m lattices, one trunk per cell, jittered at most
//  1.2 m off centre. Only the ray's OWN cell is evaluated, which is not a valid
//  distance on its own -- a neighbour's tree can be closer. But a neighbour's
//  geometry can never come nearer its own cell edge than
//      half cell - jitter - (trunk radius + branch reach)  =  4 - 1.2 - 2.25
//  so "distance to my cell edge + 0.55" is a guaranteed lower bound on it, and
//  min(own tree, that bound) is a safe step. One cell per layer, no 3x3 search.
//
//  THE CAMERA IS A BODY. |sin| of the stride puts a sharp corner at every
//  footfall, which is the jolt; the hands add incommensurate sines; the look
//  back is aimed at the pursuer itself (the path bends, so "behind" is not
//  -forward) and turns over a shoulder picked per glance. Stumbles and torch
//  flicker run on their own clocks.
//
//  MOTION BLUR costs nothing: every pixel renders at its own instant inside a
//  1/30 s shutter, so a whip pan smears into grain like cheap film would.
//
//  LIGHT. A torch at the right hand (inverse square, soft cone, hot centre)
//  and a moon ahead of you, gated by a canopy of noise projected along the
//  moon direction -- the same gate lights the ground in patches and the fog in
//  shafts. The fog is 20 dithered samples between the eye and the hit.
// =============================================================================

#define PI 3.14159265

const vec3 MOON_DIR = vec3(0.3035, 0.7586, 0.5766);
const vec3 TORCH_COL = vec3(1.0, 0.86, 0.66);
const vec3 MOON_COL = vec3(0.42, 0.55, 0.85);

// ---- hashes (Dave Hoskins: intermediates stay small) --------------------------
float h11(float n) { n = fract(n * 0.1031); n *= n + 33.33; n *= n + n; return fract(n); }
float h12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
vec2 h22(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * vec3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}
float vnoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(h12(i), h12(i + vec2(1.0, 0.0)), f.x),
               mix(h12(i + vec2(0.0, 1.0)), h12(i + vec2(1.0, 1.0)), f.x), f.y);
}

float smin(float a, float b, float k) {
    float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}
float capsule(vec3 p, vec3 a, vec3 b, float ra, float rb) {
    vec3 pa = p - a, ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - mix(ra, rb, h);
}

// ---- the path and the ground ---------------------------------------------------
float pathX(float z) {
    return 4.0 * sin(z * 0.043) + 2.0 * sin(z * 0.107 + 1.3) + 0.7 * sin(z * 0.23 + 4.0);
}
float pathDX(float z) {
    return 4.0 * 0.043 * cos(z * 0.043) + 2.0 * 0.107 * cos(z * 0.107 + 1.3)
         + 0.7 * 0.23 * cos(z * 0.23 + 4.0);
}
float groundH(vec2 p) {
    float h = 0.55 * sin(p.x * 0.11 + sin(p.y * 0.07)) * sin(p.y * 0.09)
            + 0.22 * sin(p.x * 0.31 + p.y * 0.23)
            + 0.08 * sin(p.x * 0.8 - p.y * 0.9);
    float dp = abs(p.x - pathX(p.y));
    return h - 0.16 * (1.0 - smoothstep(0.6, 2.0, dp));
}

// ---- globals set once per pixel by setup() -------------------------------------
float gT;
vec3 gRo, gFw, gRt, gUp;          // eye
vec3 gFlP, gFlD;                  // torch position / direction
float gFlash;                     // torch intensity (flicker, off)
float gLook;                      // 0 = facing forward, 1 = looking back
vec3 gFig, gFigF, gFigR;          // pursuer: foot position, forward, right
float gFigPh;                     // its stride phase

// ---- trees ---------------------------------------------------------------------
// one lattice layer: returns (distance, 1 if a real tree is closest)
vec2 trees(vec3 p, vec2 off, float seed) {
    vec2 q = p.xz / 8.0 + off;
    vec2 c = floor(q);
    vec2 l = (q - c - 0.5) * 8.0;
    float bound = 4.0 - max(abs(l.x), abs(l.y)) + 0.55;

    vec2 h = h22(c + seed);
    float hb = h12(c * 1.7 + seed + 3.1);
    vec2 j = (h - 0.5) * 2.4;
    vec2 wc = (c + 0.5 - off) * 8.0 + j;
    if (hb < 1.0 - TREES || abs(wc.x - pathX(wc.y)) < 2.8) return vec2(bound, 0.0);

    vec2 d2 = l - j;
    float gy = groundH(wc);
    float y = p.y - gy;
    float rho = length(d2);
    float R = 0.26 + 0.3 * h.x * h.y + 0.08 * hb;
    float ang = atan(d2.y, d2.x);
    float base = exp(-max(y, 0.0) * 1.6);
    float r = R * (1.0 - 0.005 * y)
            + base * R * (0.35 + 0.35 * max(0.0, sin(ang * 5.0 + hb * 17.0)))  // root buttresses
            + 0.035 * sin(ang * 7.0 + hb * 23.0 + y * 0.13)                     // bark ridges
            + 0.018 * sin(ang * 15.0 - y * 0.5);
    float d = (rho - r) * 0.6;

    // two crooked dead branches, well above head height
    if (y > 3.0 && y < 17.5 && rho < 2.5) {
        vec3 pl = vec3(d2.x, y, d2.y);
        for (int k = 0; k < 2; k++) {
            float fk = float(k);
            float hk = 5.0 + 10.0 * h11(hb * 91.0 + fk * 13.0);
            float ak = 6.2832 * h11(hb * 57.0 + fk * 7.7);
            vec3 dir = vec3(cos(ak), 0.45, sin(ak));
            vec3 a = vec3(0.0, hk, 0.0) + vec3(dir.x, 0.0, dir.z) * R * 0.5;
            vec3 m = a + dir * 0.95;
            vec3 e = m + vec3(dir.x * 0.6, -0.25 + 0.7 * h11(hk), dir.z * 0.6);
            d = min(d, capsule(pl, a, m, 0.13, 0.07));
            d = min(d, capsule(pl, m, e, 0.07, 0.02));
        }
    }
    return d < bound ? vec2(d, 1.0) : vec2(bound, 0.0);
}

// ---- the pursuer ---------------------------------------------------------------
// local frame: y up from its feet, +z the way it runs (toward you)
float figure(vec3 p) {
    float b = length(p - vec3(0.0, 1.5, 0.35)) - 1.95;
    if (b > 0.4) return b;
    float ph = gFigPh;
    float bob = 0.08 * abs(sin(ph));
    vec3 hip = vec3(0.0, 1.42 + bob, 0.0);
    vec3 chest = vec3(0.0, 2.15 + bob, 0.45);
    vec3 head = vec3(0.0, 2.36 + bob, 0.74);
    float d = capsule(p, hip, chest, 0.12, 0.17);
    d = smin(d, capsule(p, chest + vec3(-0.27, 0.02, -0.04), chest + vec3(0.27, 0.02, -0.04), 0.07, 0.07), 0.1);
    d = smin(d, capsule(p, chest, head, 0.05, 0.06), 0.06);
    vec3 hq = p - head;
    d = smin(d, length(hq * vec3(1.0, 0.78, 0.82)) - 0.14, 0.05);   // long skull
    d = min(d, capsule(p, head + vec3(0.0, -0.1, 0.06), head + vec3(0.0, -0.3, 0.12), 0.07, 0.03)); // hanging jaw
    for (int i = 0; i < 2; i++) {
        float s = float(i) * 2.0 - 1.0;
        float sw = sin(ph + float(i) * PI);
        vec3 hp = hip + vec3(0.13 * s, 0.0, 0.0);
        float th = 0.8 * sw;
        vec3 kn = hp + 0.74 * vec3(0.0, -cos(th), sin(th));
        float sh = th - 0.25 - 1.2 * max(0.0, -cos(ph + float(i) * PI));
        vec3 ft = kn + 0.74 * vec3(0.0, -cos(sh), sin(sh));
        d = min(d, capsule(p, hp, kn, 0.085, 0.055));
        d = min(d, capsule(p, kn, ft, 0.055, 0.035));
        // arms reach forward for you, swinging against the legs
        vec3 sp = chest + vec3(0.29 * s, 0.0, -0.04);
        float ca = 1.25 - 0.35 * sw;
        vec3 el = sp + 0.8 * vec3(0.1 * s, -cos(ca), sin(ca));
        float cb = ca + 0.3 + 0.2 * sw;
        vec3 hd = el + 0.85 * vec3(-0.04 * s, -cos(cb), sin(cb));
        vec3 fg = hd + 0.32 * vec3(0.03 * s, -cos(cb + 0.4), sin(cb + 0.4));
        d = min(d, capsule(p, sp, el, 0.05, 0.04));
        d = min(d, capsule(p, el, hd, 0.04, 0.022));
        d = min(d, capsule(p, hd, fg, 0.018, 0.004));
    }
    return d;
}

vec2 map(vec3 p) {
    vec2 res = vec2((p.y - groundH(p.xz)) * 0.75, 0.0);
    vec2 t1 = trees(p, vec2(0.0), 0.0);
    if (t1.x < res.x) res = vec2(t1.x, t1.y);
    vec2 t2 = trees(p, vec2(0.5, 0.37), 17.0);
    if (t2.x < res.x) res = vec2(t2.x, t2.y);
    vec3 w = p - gFig;
    float f = figure(vec3(dot(w, gFigR), w.y, dot(w, gFigF)));
    if (f < res.x) res = vec2(f, 2.0);
    return res;
}

// ---- the body holding the camera ------------------------------------------------
void setup(float t) {
    gT = t;
    float stride = t * PI * (2.2 + 0.16 * SPEED);  // cadence rises with speed: 4.4 steps/s flat out
    float bob = abs(sin(stride));
    float impact = exp(-fract(stride / PI) * 9.0);  // jolt right after each footfall

    float cz = SPEED * t + 1.5 * sin(t * 0.41);
    float cx = pathX(cz) + 0.3 * sin(t * 0.53);
    float gy = groundH(vec2(cx, cz));

    // the pursuer, somewhere back down the path
    float dist = mix(26.0, 6.0, CLOSENESS) * (1.0 + 0.3 * sin(t * 0.19));
    float fz = cz - dist;
    float fx = pathX(fz);
    gFig = vec3(fx, groundH(vec2(fx, fz)), fz);
    gFigF = normalize(vec3(pathDX(fz), 0.0, 1.0));
    gFigR = vec3(gFigF.z, 0.0, -gFigF.x);
    gFigPh = t * PI * (1.8 + 0.16 * SPEED);

    // when to look back: one glance per cycle, random length, random shoulder
    float cyc = mix(16.0, 5.0, LOOK_BACK);
    float k = floor(t / cyc);
    float st = t - k * cyc;
    float dur = 1.3 + 0.9 * h11(k * 3.7 + 1.0);
    float t0 = cyc - dur - 0.9;
    gLook = LOOK_BACK <= 0.0 ? 0.0 :
            smoothstep(t0, t0 + 0.38, st) * (1.0 - smoothstep(t0 + dur - 0.32, t0 + dur, st));
    float side = h11(k * 5.3 + 2.0) > 0.5 ? 1.0 : -1.0;

    // stumble every so often
    float sc = 21.0;
    float ss = t - floor(t / sc) * sc - 13.0;
    float stumble = smoothstep(0.0, 0.12, ss) * (1.0 - smoothstep(0.12, 1.0, ss));

    float yawF = atan(pathDX(cz + 7.0));
    vec3 toFig = gFig + vec3(0.0, 1.9, 0.0) - vec3(cx, gy + 1.6, cz);
    float yawB = atan(toFig.x, toFig.z);
    float dy = yawB - yawF;
    dy = dy - 2.0 * PI * floor(dy / (2.0 * PI));     // 0..2pi
    if (side < 0.0) dy -= 2.0 * PI;                 // turn over the other shoulder
    dy += side * 0.12 * sin(t * 2.3);               // eyes never settle

    float hand1 = sin(t * 11.3) * 0.5 + sin(t * 17.9 + 1.0) * 0.3 + sin(t * 29.1 + 2.0) * 0.2;
    float hand2 = sin(t * 13.7 + 4.0) * 0.5 + sin(t * 19.3 + 3.0) * 0.3 + sin(t * 31.7 + 1.0) * 0.2;
    float hand3 = sin(t * 9.1 + 2.5) * 0.6 + sin(t * 23.3 + 5.0) * 0.4;
    float shk = SHAKE * (1.0 + 1.5 * gLook);

    float yaw = yawF + dy * gLook + 0.03 * sin(stride * 0.5) * SHAKE + 0.012 * shk * hand1;
    float pitch = -0.065 - 0.03 * gLook - 0.35 * stumble
                + 0.012 * shk * hand2 - 0.035 * SHAKE * impact + 0.015 * SHAKE * cos(stride * 2.0);
    float roll = 0.035 * SHAKE * sin(stride) + side * 0.14 * gLook + 0.01 * shk * hand3
               + 0.2 * stumble;

    gFw = vec3(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch));
    vec3 rt0 = normalize(vec3(gFw.z, 0.0, -gFw.x));
    vec3 up0 = cross(gFw, rt0);
    gRt = rt0 * cos(roll) + up0 * sin(roll);
    gUp = up0 * cos(roll) - rt0 * sin(roll);
    gRo = vec3(cx, gy + 1.62 + (0.07 + 0.006 * SPEED) * SHAKE * bob - 0.5 * stumble, cz)
        + gRt * 0.05 * SHAKE * sin(stride);

    // the torch swings a beat behind the head and points at the path
    gFlP = gRo + gRt * 0.24 - gUp * 0.32 + gFw * 0.25;
    gFlD = normalize(gFw + gRt * (0.05 * sin(t * 3.1) + 0.03 * shk * hand2)
                         + gUp * (-0.1 + 0.06 * bob + 0.03 * shk * hand1));
    // flicker: bursts of dying batteries every 13 s
    float fc = 13.0;
    float fk = floor(t / fc);
    float fs = t - fk * fc - 4.0 - 5.0 * h11(fk * 2.1);
    float burst = step(0.0, fs) * step(fs, 0.4 + 1.2 * FLICKER);
    float flick = step(0.5, h11(floor(t * 23.0) * 1.37 + fk));
    gFlash = TORCH ? mix(1.0, flick * 0.8 + 0.05, burst * step(0.01, FLICKER)) : 0.0;
}

float canopy(vec3 p) {
    vec2 c = p.xz + MOON_DIR.xz * ((30.0 - p.y) / MOON_DIR.y);
    float n = vnoise(c * 0.085) * 0.65 + vnoise(c * 0.23 + 7.1) * 0.35;
    return smoothstep(0.56, 0.76, n);
}

void sceneMain() {
    vec2 fc = gl_FragCoord.xy;
    float dith = h12(fc + fract(TIME * 7.13) * 97.0);
    setup(TIME + (dith - 0.5) * BLUR / 30.0);

    vec2 uv = (fc - 0.5 * RENDERSIZE) / RENDERSIZE.y;
    uv *= 1.0 + 0.12 * dot(uv, uv);                       // cheap wide lens
    vec3 rd = normalize(uv.x * gRt + uv.y * gUp + 0.95 * gFw);
    vec3 ro = gRo;

    // ---- march ----
    float t = 0.05;
    vec2 hit = vec2(1.0, -1.0);
    for (int i = 0; i < 120; i++) {
        hit = map(ro + rd * t);
        if (hit.x < 0.0015 * t) break;
        t += hit.x;
        if (t > 70.0) { hit.y = -1.0; break; }
    }
    if (t > 70.0) hit.y = -1.0;

    float fogSig = 0.003 + 0.035 * FOG;
    vec3 col;
    float tEnd = t;
    if (hit.y < -0.5) {
        tEnd = 70.0;
        col = MOON_COL * MOON * 0.008 * (0.6 + 1.2 * canopy(ro + rd * 20.0));
    } else {
        vec3 p = ro + rd * t;
        float e = 0.003 + 0.0006 * t;
        vec3 n = normalize(vec3(map(p + vec3(e, 0.0, 0.0)).x - hit.x,
                                map(p + vec3(0.0, e, 0.0)).x - hit.x,
                                map(p + vec3(0.0, 0.0, e)).x - hit.x));
        float ao = clamp(0.25 + map(p + n * 0.5).x * 1.5, 0.0, 1.0);

        vec3 alb;
        float spec = 0.0;
        if (hit.y < 0.5) {
            // leaf litter, wet mud on the path
            float dp = abs(p.x - pathX(p.z)) + 0.5 * vnoise(p.xz * 1.7);
            float lit = vnoise(p.xz * 2.3) * 0.6 + vnoise(p.xz * 7.1) * 0.4;
            alb = mix(vec3(0.04, 0.032, 0.024), vec3(0.09, 0.065, 0.04), lit);
            // fallen leaves: one randomly turned ellipse per cell, two scales
            float leaves = 0.0;
            for (int i = 0; i < 2; i++) {
                vec2 lp = p.xz * (4.5 + 2.3 * float(i)) + float(i) * 3.7;
                vec2 lc = floor(lp);
                float lh = h12(lc + float(i) * 11.0);
                float la = lh * 6.2832;
                vec2 lf = fract(lp) - 0.5 - (h22(lc) - 0.5) * 0.35;
                vec2 lr = vec2(cos(la) * lf.x + sin(la) * lf.y, cos(la) * lf.y - sin(la) * lf.x);
                leaves = max(leaves, step(0.4, lh) * smoothstep(0.03, 0.0, length(lr * vec2(1.0, 2.3)) - 0.24));
            }
            leaves *= smoothstep(22.0, 6.0, t);
            float path = 1.0 - smoothstep(0.8, 1.5, dp);
            alb = mix(alb, mix(vec3(0.13, 0.075, 0.035), vec3(0.2, 0.15, 0.08), h12(floor(p.xz * 4.5))), leaves * (1.0 - 0.8 * path));
            float wet = smoothstep(0.45, 0.7, vnoise(p.xz * 0.9 + 3.0)) * path;
            alb = mix(alb, vec3(0.06, 0.05, 0.042), path * 0.8);
            alb *= 1.0 - 0.5 * wet;
            spec = path * 0.2 + wet * 1.5;
        } else if (hit.y < 1.5) {
            float an = atan(n.z, n.x);
            float streak = vnoise(vec2(an * 6.0, p.y * 0.35)) * 0.6 + vnoise(vec2(an * 25.0, p.y * 2.5)) * 0.4;
            alb = vec3(0.075, 0.07, 0.064) * (0.45 + 0.9 * streak);
            alb = mix(alb, vec3(0.11, 0.115, 0.1), smoothstep(0.62, 0.72, vnoise(vec2(an * 9.0, p.y * 1.3))));  // lichen
            float moss = smoothstep(0.9, 0.0, p.y - groundH(p.xz)) * smoothstep(0.3, 0.8, vnoise(p.xz * 3.0 + p.y));
            alb = mix(alb, vec3(0.04, 0.06, 0.03), moss);
            spec = 0.08;
        } else {
            alb = vec3(0.15, 0.14, 0.13);      // pale, wet skin
            spec = 0.5;
        }

        // torch
        vec3 lv = p - gFlP;
        float dl2 = dot(lv, lv);
        vec3 ld = lv / sqrt(dl2);
        float cz = dot(ld, gFlD);
        float cone = smoothstep(0.84, 0.95, cz) * (1.0 + 1.5 * smoothstep(0.975, 0.995, cz));
        float tI = cone * gFlash * 16.0 / (dl2 + 1.0);
        vec3 hv = normalize(-ld - rd);
        vec3 light = TORCH_COL * tI * (max(dot(n, -ld), 0.0) + spec * pow(max(dot(n, hv), 0.0), 40.0));

        // moon, through the holes in the canopy
        float cm = canopy(p);
        vec3 hm = normalize(MOON_DIR - rd);
        light += MOON_COL * MOON * 0.22 * cm * (max(dot(n, MOON_DIR), 0.0) + spec * pow(max(dot(n, hm), 0.0), 30.0));
        light += MOON_COL * MOON * 0.035 * (0.55 + 0.45 * n.y) * ao;
        col = alb * light;
    }

    // ---- fog: torch beam and moon shafts ----
    float tm = min(tEnd, 45.0);
    vec3 acc = vec3(0.0);
    float T = 1.0;
    float gM = 0.55;
    float cosM = dot(rd, MOON_DIR);
    float phM = (1.0 - gM * gM) / pow(1.0 + gM * gM - 2.0 * gM * cosM, 1.5) * 0.08;
    for (int i = 0; i < 20; i++) {
        float s0 = (float(i) + dith) / 20.0;
        float ts = tm * s0 * s0;
        float dt = tm * 2.0 * s0 / 20.0 + 0.01;
        vec3 p = ro + rd * ts;
        float wisp = 0.5 + 0.5 * sin(p.x * 0.35 + gT * 0.4 + sin(p.z * 0.3))
                              * sin(p.z * 0.27 - gT * 0.25 + sin(p.y * 0.8 + p.x * 0.2));
        float dens = fogSig * (0.3 + 1.0 * exp(-max(p.y - groundH(p.xz), 0.0) * 0.7)) * (0.3 + 1.4 * wisp);
        vec3 lv = p - gFlP;
        float dl2 = dot(lv, lv);
        float cz = dot(lv, gFlD) / sqrt(dl2 + 1e-4);
        float cone = smoothstep(0.9, 0.975, cz) * smoothstep(1.0, 9.0, dl2);
        vec3 L = TORCH_COL * cone * gFlash * 1.2 / (dl2 + 2.0)
               + MOON_COL * MOON * (phM * canopy(p) + 0.008);
        acc += T * dens * L * dt;
        T *= exp(-dens * dt);
    }
    // beyond the marched stretch: plain moonlit haze, the same colour the sky ends in
    float Tf = exp(-fogSig * 0.5 * max(tEnd - tm, 0.0));
    acc += T * (1.0 - Tf) * MOON_COL * MOON * 0.008;
    T *= Tf;
    col = col * T + acc;

    // ---- its eyes ----
    for (int i = 0; i < 2; i++) {
        float s = float(i) * 2.0 - 1.0;
        float bob = 0.08 * abs(sin(gFigPh));
        vec3 E = gFig + gFigR * (0.055 * s) + vec3(0.0, 2.39 + bob, 0.0) + gFigF * 0.86;
        vec3 v = E - ro;
        float tc = dot(v, rd);
        if (tc > 0.0 && tc < tEnd + 0.4) {
            float a = length(v - rd * tc) / tc;
            float face = max(dot(gFigF, -normalize(v)), 0.0);
            float g = 4e-8 / (a * a + 6e-7) + 1.2e-6 / (a * a + 1.5e-4);
            col += vec3(1.0, 0.8, 0.55) * g * face * exp(-fogSig * 0.5 * tc) * (0.4 + gFlash);
        }
    }

    // ---- film ----
    col *= EXPOSURE * 2.2;
    col = 1.0 - exp(-col);
    col = pow(col, vec3(0.4545));
    float lum = dot(col, vec3(0.299, 0.587, 0.114));
    col = mix(vec3(lum), col, 0.7) + vec3(-0.005, 0.0, 0.012) * (1.0 - lum);

    // panic: the vignette tightens with every heartbeat, harder when looking back
    float fear = 0.5 + 0.5 * gLook + 0.3 * CLOSENESS;
    float hb = fract(TIME * 2.3);
    float beat = exp(-hb * 14.0) + 0.6 * exp(-max(hb - 0.18, 0.0) * 14.0) * step(0.18, hb);
    vec2 q = fc / RENDERSIZE - 0.5;
    float vig = 1.0 - dot(q, q) * (1.4 + 0.8 * fear * (1.0 + 0.5 * beat));
    col *= clamp(vig, 0.0, 1.0);
    col += (h12(fc * 1.3 + fract(TIME * 13.7) * 311.0) - 0.5) * GRAIN * 0.09;
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}

// =============================================================================
//  pass 2: the tape. Everything above was recorded; this is it played back.
// =============================================================================
// 3x5 camcorder font. Each glyph is 15 bits, top row first, packed as octal
// digits so a row is floor(v / 8^k) mod 8 -- exact float arithmetic, no pow().
float glyph(float g) {
    if (g < 0.5) return 31599.0;  if (g < 1.5) return 11415.0;
    if (g < 2.5) return 29671.0;  if (g < 3.5) return 29647.0;
    if (g < 4.5) return 23497.0;  if (g < 5.5) return 31183.0;
    if (g < 6.5) return 31215.0;  if (g < 7.5) return 29257.0;
    if (g < 8.5) return 31727.0;  if (g < 9.5) return 31695.0;
    if (g < 10.5) return 1040.0;  if (g < 11.5) return 27565.0;
    if (g < 12.5) return 31143.0; return 31015.0;
}

// returns (text coverage, red dot coverage) at pixel fc
vec2 osd(vec2 fc) {
    float S = max(2.0, floor(RENDERSIZE.y / 110.0));        // one font cell in pixels
    float M = floor(RENDERSIZE.y * 0.06);
    // REC, top left, after the dot
    vec2 pr = (fc - vec2(M + 5.0 * S, RENDERSIZE.y - M - 5.0 * S)) / S;
    // counter 00:MM:SS, bottom right
    vec2 pc = (fc - vec2(RENDERSIZE.x - M - 32.0 * S, M)) / S;
    float code = -1.0;
    vec2 cp = vec2(0.0);
    float secs = floor(TIME);
    if (pr.y >= 0.0 && pr.y < 5.0 && pr.x >= 0.0 && pr.x < 12.0) {
        float k = floor(pr.x / 4.0);
        code = 11.0 + k;
        cp = vec2(pr.x - 4.0 * k, pr.y);
    } else if (pc.y >= 0.0 && pc.y < 5.0 && pc.x >= 0.0 && pc.x < 32.0) {
        float k = floor(pc.x / 4.0);
        float mn = mod(floor(secs / 60.0), 60.0), sc = mod(secs, 60.0);
        code = k < 1.5 ? 0.0 : k < 2.5 ? 10.0 : k < 3.5 ? floor(mn / 10.0) : k < 4.5 ? mod(mn, 10.0)
             : k < 5.5 ? 10.0 : k < 6.5 ? floor(sc / 10.0) : mod(sc, 10.0);
        cp = vec2(pc.x - 4.0 * k, pc.y);
    }
    float on = 0.0;
    if (code >= 0.0 && cp.x < 3.0) {
        float yy = floor(cp.y), xx = floor(cp.x);
        float rowDiv = yy < 0.5 ? 1.0 : yy < 1.5 ? 8.0 : yy < 2.5 ? 64.0 : yy < 3.5 ? 512.0 : 4096.0;
        float row = mod(floor(glyph(code) / rowDiv), 8.0);
        on = mod(floor(row / (xx < 0.5 ? 4.0 : xx < 1.5 ? 2.0 : 1.0)), 2.0);
    }
    vec2 dc = fc - vec2(M + 1.5 * S, RENDERSIZE.y - M - 2.5 * S);
    float dot_ = step(length(dc), 1.6 * S) * step(0.5, fract(TIME * 0.8));
    return vec2(on, dot_);
}

void tapeMain() {
    vec2 fc = gl_FragCoord.xy;
    vec2 uv = fc / RENDERSIZE;
    float A = VHS;
    setup(TIME);
    float t = TIME;
    float line = floor(uv.y * 243.0);                       // one field's worth of lines
    float fr = floor(t * 29.97);

    // tracking band rolling down the picture; tears hardest while you look back
    float by = fract(-t * 0.045 + 0.37);
    float dy = uv.y - by; dy -= floor(dy + 0.5);
    float bandAmt = 0.25 + 0.75 * max(gLook, smoothstep(0.62, 0.85, vnoise(vec2(t * 0.35, 4.0))));
    float band = smoothstep(0.035, 0.0, abs(dy)) * bandAmt;
    float head = smoothstep(0.05, 0.0, uv.y);               // head-switching strip

    float off = (h11(line * 0.731 + fr * 0.113) - 0.5) * 0.0018
              + sin(uv.y * 7.0 + t * 1.1) * 0.0012 + sin(uv.y * 41.0 - t * 4.3) * 0.0005
              + band * (h11(line * 1.37 + fr) - 0.5) * 0.05
              + head * (0.015 + 0.02 * h11(line + fr * 0.7));
    off *= A;
    float x0 = uv.x + off;

    // luma: short smear; chroma: wide smear that lags to the right
    float Y = 0.0, I = 0.0, Q = 0.0;
    for (int i = 0; i < 6; i++) {
        float fi = float(i);
        vec3 c = IMG_NORM_PIXEL(scene, vec2(x0 - (fi - 2.0) * 0.0007 * A, uv.y)).rgb;
        Y += dot(c, vec3(0.299, 0.587, 0.114)) * (fi == 2.0 ? 0.3 : 0.14);
        vec3 k = IMG_NORM_PIXEL(scene, vec2(x0 - 0.004 * A - fi * 0.0028 * A, uv.y)).rgb;
        I += dot(k, vec3(0.596, -0.274, -0.322)) / 6.0;
        Q += dot(k, vec3(0.211, -0.523, 0.312)) / 6.0;
    }
    I *= 1.0 + 0.4 * A; Q *= 1.0 + 0.4 * A;
    // tape noise: horizontal streaky snow, heavier in the band and the head strip
    float snow = h12(vec2(floor(fc.x / (2.0 + 3.0 * h11(line))), line + fr * 17.0));
    Y += (snow - 0.5) * A * (0.035 + 0.25 * band + 0.3 * head);
    // dropouts: a rare white scratch across part of a line
    float dr = h12(vec2(line * 0.37, fr * 0.91));
    float dx = h11(line + fr * 3.1);
    Y += step(0.9992, dr) * smoothstep(0.06, 0.0, abs(uv.x - dx)) * (0.25 + 0.3 * snow) * min(A, 1.0);

    vec3 col = vec3(Y + 0.956 * I + 0.621 * Q, Y - 0.272 * I - 0.647 * Q, Y - 1.106 * I + 1.703 * Q);
    // the OSD is burned onto the tape, so it wobbles with the lines
    if (OSD) {
        vec2 o = osd(vec2(x0 * RENDERSIZE.x, fc.y));
        vec2 os = osd(vec2(x0 * RENDERSIZE.x - 2.0, fc.y));   // camcorder drop shadow / bleed
        col = mix(col, vec3(0.0), os.x * 0.5);
        col = mix(col, vec3(0.92, 0.95, 0.9), o.x * 0.9);
        col = mix(col, vec3(0.95, 0.12, 0.08), o.y);
    }
    col *= 0.93 + 0.07 * sin(fc.y * PI * 0.5) * min(A, 1.0);  // interlace lines
    col = mix(col, col * vec3(1.02, 0.97, 1.05) + vec3(0.012, 0.0, 0.018), min(A, 1.0)); // tape tint, lifted blacks
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}

void main() {
    if (PASSINDEX == 0) sceneMain();
    else tapeMain();
}

