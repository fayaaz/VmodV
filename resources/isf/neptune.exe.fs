/*{
  "DESCRIPTION": "A Greek paradise pool at the end of the world, seen through a vaporwave haze. A long turquoise pool tiled in a navy grid and ringed with a Greek key border, its water rippling and throwing caustics on the floor, stretches toward a white marble temple: fluted columns, a heavy entablature and a pediment, a neon glow spilling from its doorway. Domed pavilions stand to either side, marble busts watch from their pedestals, balustrades run along the edges and palm trees lean over the water. The whole terrace floats above an endless magenta grid. Behind it all, a huge striped retro sun sinks into wireframe mountains under a purple-pink sky, and every bit of it is mirrored in the pool. SPEED is the camera drift, SUN HEIGHT and SUN SIZE place the sun, STRIPES cuts its bands, WAVES and CAUSTICS stir the water, NEON the glow of the grid and doorway, GRID, MOUNTAINS, PALMS and STATUES switch the set dressing, SCANLINES the CRT look, HUE spins the palette, EXPOSURE the overall light.",
  "CREDIT": "CC0",
  "ISFVSN": "2",
  "CATEGORIES": ["generator", "vaporwave", "architecture", "water", "raymarch"],
  "INPUTS": [
    { "NAME": "SPEED", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 4.0 },
    { "NAME": "SUN_HEIGHT", "TYPE": "float", "DEFAULT": 0.17, "MIN": -0.1, "MAX": 0.5, "LABEL": "sun height" },
    { "NAME": "SUN_SIZE", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.4, "MAX": 2.0, "LABEL": "sun size" },
    { "NAME": "STRIPES", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "WAVES", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 3.0 },
    { "NAME": "CAUSTICS", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 2.0 },
    { "NAME": "NEON", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 3.0 },
    { "NAME": "GRID", "TYPE": "bool", "DEFAULT": true },
    { "NAME": "MOUNTAINS", "TYPE": "bool", "DEFAULT": true },
    { "NAME": "PALMS", "TYPE": "bool", "DEFAULT": true },
    { "NAME": "STATUES", "TYPE": "bool", "DEFAULT": true },
    { "NAME": "SCANLINES", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "HUE", "TYPE": "float", "DEFAULT": 0.0, "MIN": -3.14159, "MAX": 3.14159 },
    { "NAME": "EXPOSURE", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.3, "MAX": 3.0 }
  ]
}*/

// ---------------------------------------------------------------------------
// Layout (metres, y up). The pool is x in [-9,9], z in [0,28], water at y=0,
// floor at y=-2. The marble terrace is a slab x in [-16,16], z in [-12,48]
// with its top at y=0.3, floating over a neon grid plane far below. The temple
// faces the pool from z=33. Everything solid lives in map(); the water is a
// plane handled analytically, refracting into the pool box (also analytic)
// and reflecting back into the scene (the same march loop, continued).
// ---------------------------------------------------------------------------

#define PI 3.14159265
#define TAU 6.28318531

float hash11(float p) { p = fract(p * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
float hash21(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
float noise2(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2(1.0, 0.0)), u.x),
               mix(hash21(i + vec2(0.0, 1.0)), hash21(i + vec2(1.0, 1.0)), u.x), u.y);
}
float fbm2(vec2 p) {
    float a = 0.5, s = 0.0;
    for (int i = 0; i < 4; i++) { s += a * noise2(p); p = p * 2.03 + vec2(1.7, 9.2); a *= 0.5; }
    return s;
}

// ---- SDF primitives -------------------------------------------------------
float sdBox(vec3 p, vec3 b) {
    vec3 q = abs(p) - b;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
}
float sdEll(vec3 p, vec3 r) {
    float k0 = length(p / r);
    float k1 = length(p / (r * r));
    return k0 * (k0 - 1.0) / k1;
}
float sdCap(vec3 p, vec3 a, vec3 b, float r) {
    vec3 pa = p - a, ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}
// isosceles triangle, apex at origin, base at y=q.y (y grows downward)
float sdTri(vec2 p, vec2 q) {
    p.x = abs(p.x);
    vec2 a = p - q * clamp(dot(p, q) / dot(q, q), 0.0, 1.0);
    vec2 b = p - q * vec2(clamp(p.x / q.x, 0.0, 1.0), 1.0);
    float s = -sign(q.y);
    vec2 d = min(vec2(dot(a, a), s * (p.x * q.y - p.y * q.x)),
                 vec2(dot(b, b), s * (p.y - q.y)));
    return -sqrt(d.x) * sign(d.y);
}
float smin(float a, float b, float k) {
    float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}
vec2 opU(vec2 a, vec2 b) { return a.x < b.x ? a : b; }

// Materials: 1 marble, 2 terrace, 3 palm trunk, 4 palm leaf, 5 bust,
// 6 cella wall, 7 neon doorway

// A fluted Doric-ish column: xz relative to its axis, y from base y0 to top y1.
float column(vec2 xz, float y, float y0, float y1, float r0) {
    float h = y1 - y0;
    float s = clamp((y - y0) / h, 0.0, 1.0);
    float r = r0 * (1.0 - 0.14 * s);
    float a = atan(xz.y, xz.x);
    r -= r0 * 0.05 * abs(sin(a * 10.0));
    float d = max(length(xz) - r, abs(y - (y0 + h * 0.5)) - h * 0.5);
    // base torus + plinth, capital echinus + abacus
    float base = max(length(xz) - r0 * 1.28, abs(y - y0 - r0 * 0.25) - r0 * 0.25);
    float ech = max(length(xz) - r0 * (1.0 + 0.35 * clamp((y - (y1 - r0 * 0.55)) / (r0 * 0.3), 0.0, 1.0)),
                    abs(y - (y1 - r0 * 0.4)) - r0 * 0.16);
    float aba = sdBox(vec3(xz.x, y - (y1 - r0 * 0.12), xz.y), vec3(r0 * 1.42, r0 * 0.12, r0 * 1.42));
    return min(min(d, base), min(ech, aba));
}

vec2 temple(vec3 p) {
    // three stylobate steps
    float d = 1e5;
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        float top = 0.3 + 0.35 * (fi + 1.0);
        float z0 = 31.0 + 0.5 * fi;
        d = min(d, sdBox(p - vec3(0.0, (top - 0.5) * 0.5, (z0 + 45.0) * 0.5),
                         vec3(10.0 - 0.5 * fi, (top + 0.5) * 0.5, (45.0 - z0) * 0.5)));
    }
    // front row of six columns, x = +-1.5, 4.5, 7.5
    float cx = p.x - 3.0 * (clamp(floor(p.x / 3.0), -3.0, 2.0) + 0.5);
    // side peristyle rows down the flanks; evaluate only the nearer column
    vec2 fa = vec2(cx, p.z - 33.0);
    vec2 fb = vec2(abs(p.x) - 7.5, p.z - 33.0 - 3.0 * clamp(floor((p.z - 33.0) / 3.0 + 0.5), 1.0, 3.0));
    d = min(d, column(dot(fa, fa) < dot(fb, fb) ? fa : fb, p.y, 1.35, 8.0, 0.52));
    // entablature with triglyph grooves on the frieze, then cornice
    float ent = sdBox(p - vec3(0.0, 8.8, 38.25), vec3(8.3, 0.8, 5.6));
    float gro = sdBox(vec3(mod(p.x + 0.375, 0.75) - 0.375, p.y - 9.05, p.z - 32.65), vec3(0.05, 0.32, 0.06));
    ent = max(ent, -gro);
    ent = min(ent, sdBox(p - vec3(0.0, 9.75, 38.25), vec3(8.8, 0.15, 6.0)));
    d = min(d, ent);
    // pediment: triangular prism with a recessed tympanum
    float tri = sdTri(vec2(p.x, 12.5 - p.y), vec2(8.8, 2.6));
    float ped = max(tri, abs(p.z - 38.25) - 6.0);
    float inner = sdTri(vec2(p.x, 12.5 - 0.55 - p.y), vec2(7.6, 2.0));
    ped = max(ped, -max(inner, abs(p.z - 32.25) - 0.22));
    // sculpted figures in the tympanum: a row of lumpy reclining forms
    float fx = p.x - 1.6 * clamp(floor(p.x / 1.6 + 0.5), -3.0, 3.0);
    float fh = 0.75 - 0.18 * abs(floor(p.x / 1.6 + 0.5));
    float fig = sdEll(vec3(fx, p.y - 10.05 - fh * 0.5, p.z - 32.35), vec3(0.42, fh * 0.75, 0.2));
    fig = max(fig, inner + 0.05);
    ped = min(ped, fig);
    d = min(d, ped);
    vec2 res = vec2(d, 1.0);
    // cella wall with a glowing doorway
    float cel = sdBox(p - vec3(0.0, 4.6, 40.5), vec3(6.0, 3.4, 3.0));
    float door = sdBox(p - vec3(0.0, 3.6, 37.5), vec3(1.5, 2.25, 0.35));
    cel = max(cel, -door);
    res = opU(res, vec2(cel, 6.0));
    res = opU(res, vec2(sdBox(p - vec3(0.0, 3.6, 37.9), vec3(1.6, 2.3, 0.05)), 7.0));
    return res;
}

// domed pavilion centred at the origin (xz), on the terrace
float pavilion(vec3 p) {
    vec2 c = vec2(abs(p.x) - 1.8, abs(p.z) - 1.8);
    float d = column(c, p.y, 0.3, 4.7, 0.3);
    d = min(d, sdBox(p - vec3(0.0, 0.15, 0.0), vec3(2.5, 0.25, 2.5)));
    d = min(d, sdBox(p - vec3(0.0, 5.05, 0.0), vec3(2.4, 0.35, 2.4)));
    d = min(d, sdBox(p - vec3(0.0, 5.48, 0.0), vec3(2.55, 0.08, 2.55)));
    float dome = sdEll(p - vec3(0.0, 5.5, 0.0), vec3(1.9, 1.6, 1.9));
    d = min(d, max(dome, 5.5 - p.y));
    d = min(d, sdCap(p, vec3(0.0, 7.0, 0.0), vec3(0.0, 7.5, 0.0), 0.12));
    return d;
}

// balustrade running along z at x = 0 (local)
float balustrade(vec3 p) {
    float d = sdBox(p - vec3(0.0, 1.17, 0.0), vec3(0.17, 0.07, 30.0));
    d = min(d, sdBox(p - vec3(0.0, 0.4, 0.0), vec3(0.2, 0.1, 30.0)));
    float bz = mod(p.z, 0.45) - 0.225;
    float s = clamp((p.y - 0.5) / 0.6, 0.0, 1.0);
    float r = 0.055 + 0.055 * sin(s * PI * 0.85 + 0.25) * (1.0 - 0.4 * s);
    d = min(d, max(length(vec2(p.x, bz)) - r, abs(p.y - 0.8) - 0.32) * 0.8);
    // posts
    float pz = mod(p.z + 2.25, 4.5) - 2.25;
    d = min(d, sdBox(vec3(p.x, p.y - 0.75, pz), vec3(0.22, 0.48, 0.22)));
    return d;
}

// classical bust on a pedestal; local frame, face toward +z, y=0 on the terrace
vec2 bust(vec3 p) {
    float ped = sdBox(p - vec3(0.0, 1.0, 0.0), vec3(0.38, 0.7, 0.38));
    ped = min(ped, sdBox(p - vec3(0.0, 1.76, 0.0), vec3(0.46, 0.06, 0.46)));
    ped = min(ped, sdBox(p - vec3(0.0, 0.38, 0.0), vec3(0.48, 0.08, 0.48)));
    vec3 b = (p - vec3(0.0, 1.82, 0.0)) / 1.25;
    float plinth = max(length(b.xz) - 0.22 + 0.1 * b.y, abs(b.y - 0.1) - 0.1);
    // torso cut off in the classic U, shoulders as a smoothed bar
    float chest = sdEll(b - vec3(0.0, 0.55, 0.0), vec3(0.44, 0.4, 0.23));
    chest = smin(chest, sdCap(b, vec3(-0.4, 0.74, -0.02), vec3(0.4, 0.74, -0.02), 0.14), 0.12);
    chest = max(chest, 0.2 + 0.9 * b.x * b.x - b.y);
    float neck = sdCap(b, vec3(0.0, 0.7, -0.02), vec3(0.0, 1.02, 0.02), 0.13);
    float head = sdEll(b - vec3(0.0, 1.18, 0.04), vec3(0.18, 0.24, 0.22));
    float jaw = sdEll(b - vec3(0.0, 1.06, 0.1), vec3(0.15, 0.13, 0.15));
    float nose = sdCap(b, vec3(0.0, 1.22, 0.26), vec3(0.0, 1.13, 0.3), 0.035);
    float hair = sdEll(b - vec3(0.0, 1.25, -0.03), vec3(0.21, 0.22, 0.23));
    hair += 0.025 * sin(b.x * 38.0) * sin(b.y * 41.0) * sin(b.z * 37.0);
    hair = max(hair, -(b.z - 0.08 - 1.4 * (b.y - 1.2)));
    float d = smin(chest, neck, 0.1);
    d = smin(d, head, 0.06);
    d = smin(d, jaw, 0.05);
    d = smin(d, nose, 0.02);
    d = smin(d, hair, 0.04);
    d = min(d, plinth);
    d *= 1.25;
    return opU(vec2(ped, 1.0), vec2(d * 0.9, 5.0));
}

// palm tree: base at origin, height H, top displaced by lean
vec2 palm(vec3 p, float H, vec2 lean, float seed) {
    float h = clamp(p.y, 0.0, H);
    float s = h / H;
    vec2 off = lean * s * s;
    float r = 0.24 - 0.08 * s + 0.025 * abs(fract(p.y * 3.5) - 0.5);
    float trunk = (length(p.xz - off) - r) * 0.75;
    trunk = max(trunk, p.y - H - 0.2);
    vec2 res = vec2(trunk, 3.0);
    vec3 c = p - vec3(lean.x, H, lean.y);
    float bc = length(c) - 5.2;
    if (bc > 0.5) return opU(res, vec2(bc, 4.0));
    float sec = TAU / 11.0;
    float a = atan(c.z, c.x) + seed;
    float k = floor(a / sec + 0.5);
    float ang = k * sec - seed;
    float ca = cos(ang), sa = sin(ang);
    float lx = ca * c.x + sa * c.z;
    float lz = -sa * c.x + ca * c.z;
    float hk = hash11(k * 7.13 + seed * 3.1);
    float L = 3.7 + 1.0 * hk;
    float x = clamp(lx, 0.0, L);
    float sway = 0.05 * sin(TIME * 1.3 + k * 1.7 + seed) * x;
    // arch up out of the crown, then hang
    float yc = (0.75 - 0.2 * hk) * x - (0.3 + 0.1 * hk) * x * x / L * 1.9 + sway;
    float w = 0.36 * pow(max(sin(PI * x / L), 0.0), 0.5);
    // leaflets: fingers across the frond, drooping either side of the rib
    float leaflet = abs(fract(x * 6.0) - 0.5);
    float wl = w * (0.55 + 0.45 * smoothstep(0.08, 0.4, leaflet));
    float dz = abs(lz) - wl;
    float dy = abs(c.y - yc + 0.9 * abs(lz)) - 0.022;
    float leaf = max(max(dz, dy), max(lx - L, -lx));
    res = opU(res, vec2(leaf * 0.5, 4.0));
    res = opU(res, vec2(length(c + vec3(0.0, 0.25, 0.0)) - 0.42, 3.0));
    return res;
}

vec2 map(vec3 p) {
    // terrace slab with the pool cut out
    float d = sdBox(p - vec3(0.0, -2.2, 18.0), vec3(16.0, 2.5, 30.0));
    d = max(d, -sdBox(p - vec3(0.0, -1.0, 14.0), vec3(9.0, 1.6, 14.0)));
    vec2 res = vec2(d, 2.0);

    float bt = sdBox(p - vec3(0.0, 6.5, 38.0), vec3(10.2, 6.6, 7.3));
    if (bt < 0.6) res = opU(res, temple(p));
    else res = opU(res, vec2(bt, 1.0));

    vec3 q = vec3(abs(p.x), p.y, p.z);
    float bp = sdBox(q - vec3(13.0, 4.0, 22.0), vec3(2.8, 4.0, 2.8));
    if (bp < 0.6) res = opU(res, vec2(pavilion(q - vec3(13.0, 0.0, 22.0)), 1.0));
    else res = opU(res, vec2(bp, 1.0));

    float bb = sdBox(q - vec3(15.4, 0.8, 18.0), vec3(0.4, 0.6, 30.0));
    if (bb < 0.4) res = opU(res, vec2(balustrade(q - vec3(15.4, 0.0, 18.0)), 1.0));
    else res = opU(res, vec2(bb, 1.0));

    if (STATUES) {
        float zc = p.z < 18.0 ? 6.0 : 29.6;
        vec3 bq = q - vec3(10.4, 0.0, zc);
        float bs = sdBox(bq - vec3(0.0, 1.6, 0.0), vec3(0.8, 1.7, 0.8));
        if (bs < 0.4) {
            // face the pool and a little toward the camera end
            vec2 f = normalize(vec2(-1.0, zc < 18.0 ? -0.35 : -0.9));
            vec3 lp = vec3(f.y * bq.x - f.x * bq.z, bq.y, f.x * bq.x + f.y * bq.z);
            res = opU(res, bust(lp));
        } else res = opU(res, vec2(bs, 1.0));
    }

    if (PALMS) {
        // one palm per z band (inlined once); the band edge bounds the step
        float sgn = p.x < 0.0 ? -1.0 : 1.0;
        vec3 pb; float ph, ps, edge; vec2 pl;
        if (p.z < 21.0) {
            pb = vec3(13.6, 0.3, 11.0); ph = 8.5; pl = vec2(-1.6, 0.4); ps = 0.3 + sgn; edge = 21.0 - p.z;
        } else if (p.z < 39.0) {
            pb = vec3(13.4, 0.3, 30.5); ph = 10.5; pl = vec2(-1.2, -0.8); ps = 1.7 + sgn * 0.6; edge = min(p.z - 21.0, 39.0 - p.z);
        } else {
            pb = vec3(5.0, 0.3, 46.5); ph = 13.5; pl = vec2(1.6, 0.5); ps = 2.9 + sgn * 1.3; edge = p.z - 39.0;
        }
        vec2 pr = palm(q - pb, ph, pl, ps);
        pr.x = min(pr.x, edge + 0.3);
        res = opU(res, pr);
    }
    return res;
}

// distance to leave the world box, from inside it
float worldExit(vec3 ro, vec3 rd) {
    vec3 bmin = vec3(-17.0, -5.0, -13.0), bmax = vec3(17.0, 19.0, 52.0);
    vec3 t1 = (bmin - ro) / rd, t2 = (bmax - ro) / rd;
    vec3 tf = max(t1, t2);
    return min(min(tf.x, tf.y), tf.z);
}

vec3 calcNormal(vec3 p, float t) {
    vec2 e = vec2(1.0, -1.0) * 0.0006 * max(t, 1.0);
    return normalize(e.xyy * map(p + e.xyy).x + e.yyx * map(p + e.yyx).x +
                     e.yxy * map(p + e.yxy).x + e.xxx * map(p + e.xxx).x);
}

float softShadow(vec3 ro, vec3 rd) {
    float res = 1.0, t = 0.06;
    for (int i = 0; i < 32; i++) {
        float h = map(ro + rd * t).x;
        res = min(res, 9.0 * h / t);
        t += clamp(h, 0.05, 1.2);
        if (res < 0.01 || t > 30.0) break;
    }
    return clamp(res, 0.0, 1.0);
}

float ambOcc(vec3 p, vec3 n) {
    float o = 0.0, s = 1.0;
    for (int i = 0; i < 4; i++) {
        float h = 0.05 + 0.18 * float(i);
        o += (h - map(p + n * h).x) * s;
        s *= 0.7;
    }
    return clamp(1.0 - 1.6 * o, 0.0, 1.0);
}

// ---- lights & sky --------------------------------------------------------
vec3 sunDir() {
    float az = -0.3, alt = SUN_HEIGHT;
    return normalize(vec3(sin(az) * cos(alt), sin(alt), cos(az) * cos(alt)));
}
const vec3 KEY_DIR = vec3(-0.462, 0.431, -0.775);
const vec3 KEY_COL = vec3(1.0, 0.6, 0.52) * 2.5;
vec3 horizonCol() { return vec3(1.0, 0.42, 0.5); }

float mountainH(float a) {
    float h = 0.0, f = 1.0, w = 0.05;
    for (int i = 0; i < 4; i++) { h += w * noise2(vec2(a * 3.0 * f, 1.3 * float(i))); f *= 2.1; w *= 0.48; }
    return h * (0.4 + 0.8 * smoothstep(0.0, 0.9, abs(a - 0.1)));
}

vec3 sky(vec3 ro, vec3 rd) {
    float y = rd.y;
    vec3 top = vec3(0.07, 0.02, 0.2);
    vec3 mid = vec3(0.55, 0.1, 0.55);
    vec3 hor = horizonCol();
    vec3 col = mix(mid, top, smoothstep(0.05, 0.6, y));
    col = mix(hor, col, smoothstep(-0.02, 0.18, y));
    // stars up high
    vec2 sp = vec2(atan(rd.x, rd.z) * 60.0, y * 60.0);
    float st = hash21(floor(sp));
    col += vec3(0.9, 0.8, 1.0) * step(0.985, st) * smoothstep(0.25, 0.6, y) *
           (0.5 + 0.5 * sin(TIME * 2.0 + st * 40.0)) * smoothstep(0.3, 0.0, length(fract(sp) - 0.5));

    // the retro sun: vertical gradient, bands cut through its lower half
    vec3 sd = sunDir();
    vec3 sr = normalize(cross(vec3(0.0, 1.0, 0.0), sd));
    vec3 su = cross(sd, sr);
    vec2 uv = vec2(dot(rd, sr), dot(rd, su)) / (0.17 * SUN_SIZE);
    float r = length(uv);
    if (dot(rd, sd) > 0.0) {
        vec3 scol = mix(vec3(1.0, 0.12, 0.55), vec3(1.0, 0.85, 0.25), smoothstep(-0.9, 0.8, uv.y)) * 2.2;
        float disk = smoothstep(1.0, 0.985, r);
        float yy = -uv.y + 0.3;
        float bandW = clamp(yy * 0.4, 0.0, 0.55) * STRIPES;
        float cut = step(0.0, yy) * step(fract(yy * 7.0 - TIME * 0.12), bandW);
        col = mix(col, scol, disk * (1.0 - cut));
        col += vec3(1.0, 0.3, 0.55) * 0.5 * exp(-max(r - 1.0, 0.0) * 3.5) * (1.0 - disk * (1.0 - cut));
    }

    // wireframe mountains standing on the horizon
    if (MOUNTAINS) {
        float a = atan(rd.x, rd.z);
        float mh = mountainH(a);
        if (y < mh && y > -0.01) {
            float v = y / max(mh, 1e-3);
            vec3 mcol = mix(vec3(0.06, 0.01, 0.14), vec3(0.2, 0.03, 0.3), v);
            float lu = abs(fract(a * 28.0) - 0.5);
            float lv = abs(fract(v * 6.0 - 0.0) - 0.5);
            float wire = smoothstep(0.43, 0.5, max(lu, lv));
            mcol += vec3(0.1, 0.9, 1.0) * wire * 0.6 * NEON * (0.4 + 0.6 * v);
            mcol += vec3(1.0, 0.2, 0.8) * NEON * 1.5 * smoothstep(0.012, 0.0, mh - y);
            col = mcol;
        }
    }

    // below the horizon: the infinite neon grid far beneath the terrace
    if (y < 0.0) {
        vec3 g = vec3(0.06, 0.0, 0.12);
        if (GRID) {
            float t = (-40.0 - ro.y) / y;
            vec2 gp = ro.xz + rd.xz * t;
            gp.y -= TIME * 6.0 * SPEED;
            vec2 gd = abs(fract(gp / 6.0) - 0.5) * 6.0;
            float dl = 3.0 - max(gd.x, gd.y);
            float w = 0.05 + t * 0.0012;
            float line = smoothstep(w, 0.0, dl) + 0.35 * exp(-dl / (w * 4.0));
            g += vec3(1.0, 0.15, 0.85) * line * NEON * 1.4 * exp(-t * 0.0025);
        }
        col = mix(g, hor, exp(y * 18.0));
    }
    return col;
}

// ---- pool -------------------------------------------------------------------
float keyBit(float col, float row) {
    float v = row < 0.5 ? 63.0 : row < 1.5 ? 1.0 : row < 2.5 ? 29.0 : row < 3.5 ? 21.0 : row < 4.5 ? 17.0 : row < 5.5 ? 31.0 : row < 6.5 ? 0.0 : 63.0;
    return mod(floor(v / exp2(col)), 2.0);
}

// Greek key band: s runs along the band, a across it (0..0.8)
float meander(vec2 sa) {
    vec2 c = floor(sa / 0.1);
    if (c.y < 0.0 || c.y > 7.0) return 0.0;
    return keyBit(mod(c.x, 6.0), c.y);
}

float caustic(vec2 xz) {
    vec2 p = mod(xz * 0.55, TAU) - 250.0;
    vec2 i = p;
    float c = 1.0, inten = 0.005, t = TIME * 0.45;
    for (int n = 0; n < 4; n++) {
        float tt = t * (1.0 - 3.5 / float(n + 1));
        i = p + vec2(cos(tt - i.x) + sin(tt + i.y), sin(tt - i.y) + cos(tt + i.x));
        c += 1.0 / length(vec2(p.x / (sin(i.x + tt) / inten), p.y / (cos(i.y + tt) / inten)));
    }
    c /= 4.0;
    c = 1.17 - pow(c, 1.4);
    return pow(abs(c), 8.0);
}

vec3 poolInterior(vec3 p, vec3 r, float viewT) {
    float tx = (r.x > 0.0 ? 9.0 - p.x : -9.0 - p.x) / r.x;
    float tz = (r.z > 0.0 ? 28.0 - p.z : -p.z) / r.z;
    float ty = (-2.0 - p.y) / r.y;
    float tt = min(min(tx, tz), ty);
    vec3 h = p + r * tt;
    vec3 tile = vec3(0.86, 0.96, 1.0);
    vec3 ink = vec3(0.1, 0.06, 0.38);
    float lineW = 0.035 + viewT * 0.0012;
    float pat;
    if (ty <= tx && ty <= tz) {
        // floor: big grid, Greek key band round the edge
        float dx = 9.0 - abs(h.x), dz = min(h.z, 28.0 - h.z);
        float de = min(dx, dz);
        float s = dx < dz ? h.z : h.x;
        float key = meander(vec2(s, de - 0.3));
        vec2 g = abs(fract(h.xz / 1.75) - 0.5) * 1.75;
        float grid = smoothstep(lineW, 0.0, 0.875 - max(g.x, g.y)) * step(1.25, de);
        pat = max(key, grid);
    } else {
        float s = tx < tz ? h.z : h.x;
        float key = meander(vec2(s, h.y + 0.95));
        vec2 g = abs(fract(vec2(s, h.y) / vec2(1.75, 0.6)) - 0.5) * vec2(1.75, 0.6);
        float grid = smoothstep(lineW * 0.6, 0.0, 0.875 - g.x) * step(h.y, -1.0);
        pat = max(key, grid);
    }
    // patterns blur away with distance rather than alias
    pat = mix(pat, 0.22, smoothstep(12.0, 45.0, viewT + tt * 2.0));
    vec3 alb = mix(tile, ink, pat);
    float cau = caustic(h.xz + r.xz * 0.0) * CAUSTICS;
    vec3 lit = alb * (vec3(0.35, 0.45, 0.75) + KEY_COL * 0.35 * (0.4 + 1.6 * cau));
    float L = tt + 1.2;
    vec3 absorb = exp(-L * vec3(0.42, 0.075, 0.05));
    return lit * absorb + vec3(0.0, 0.32, 0.42) * (1.0 - exp(-L * 0.22));
}

float waveH(vec2 p) {
    float t = TIME * 0.7;
    return 0.03 * sin(p.x * 1.3 + t * 1.7) + 0.025 * sin(p.y * 1.7 - t * 1.3 + p.x * 0.4)
         + 0.03 * (noise2(p * 1.4 + vec2(t * 0.6, t * 0.4)) - 0.5)
         + 0.015 * (noise2(p * 3.6 - vec2(t * 0.9, 0.3 * t)) - 0.5);
}

// ---- surface shading -------------------------------------------------------
vec3 shade(vec3 p, vec3 n, vec3 rd, float m, float t, bool full) {
    vec3 alb = vec3(0.92, 0.89, 0.92);
    float spec = 0.15, emit = 0.0;
    if (m < 1.5 || m > 4.5 && m < 5.5) {
        float v = fbm2(p.xz * 0.7 + p.y * vec2(0.6, -0.4));
        float vein = smoothstep(0.035, 0.0, abs(v - 0.5));
        alb *= 1.0 - 0.25 * vein;
        if (m > 4.5) { alb = vec3(0.96, 0.93, 0.97); spec = 0.3; }
    } else if (m < 2.5) {
        // terrace: pastel checkerboard slabs on top, plain stone on the sides
        vec2 c = floor(p.xz / 1.6);
        float ch = mod(c.x + c.y, 2.0);
        vec3 a = mix(vec3(0.95, 0.93, 0.95), vec3(1.0, 0.7, 0.84), ch);
        vec2 g = abs(fract(p.xz / 1.6) - 0.5) * 1.6;
        float grout = smoothstep(0.02 + t * 0.0008, 0.0, 0.8 - max(g.x, g.y));
        a = mix(a, vec3(0.5, 0.4, 0.55), grout * 0.6);
        a *= 0.9 + 0.1 * fbm2(p.xz * 1.3);
        alb = n.y > 0.5 ? a : vec3(0.85, 0.82, 0.86);
        spec = 0.25;
    } else if (m < 3.5) {
        alb = vec3(0.32, 0.2, 0.24) * (0.8 + 0.4 * abs(fract(p.y * 3.5) - 0.5));
        spec = 0.0;
    } else if (m < 4.5) {
        alb = vec3(0.1, 0.4, 0.33);
        spec = 0.05;
    } else if (m < 6.5) {
        alb = vec3(0.6, 0.55, 0.66);
    } else {
        // the doorway glows: hot pink at the floor fading to cyan above
        emit = 1.0;
    }
    if (emit > 0.5) {
        float v = clamp((p.y - 1.35) / 4.5, 0.0, 1.0);
        vec3 e = mix(vec3(1.0, 0.15, 0.6), vec3(0.15, 0.8, 1.0), v);
        float scan = 0.85 + 0.15 * sin(p.y * 40.0 - TIME * 4.0);
        return e * (0.6 + 1.6 * NEON) * scan;
    }
    float dif = clamp(dot(n, KEY_DIR), 0.0, 1.0);
    if (m > 3.5 && m < 4.5) dif = clamp(0.4 + 0.6 * dot(n, KEY_DIR), 0.0, 1.0) * 0.9;
    float sh = 1.0, ao = 1.0;
    if (full) {
        if (dif > 0.001) sh = softShadow(p + n * 0.02, KEY_DIR);
        ao = ambOcc(p, n);
    }
    vec3 amb = mix(vec3(0.3, 0.12, 0.38), vec3(0.32, 0.3, 0.75), 0.5 + 0.5 * n.y);
    vec3 sd = sunDir();
    float back = clamp(dot(n, sd), 0.0, 1.0);
    float rim = pow(clamp(1.0 + dot(n, rd), 0.0, 1.0), 3.0);
    vec3 bounce = vec3(0.1, 0.45, 0.55) * clamp(-n.y, 0.0, 1.0);
    vec3 h = normalize(KEY_DIR - rd);
    float sp = pow(clamp(dot(n, h), 0.0, 1.0), 40.0) * spec;
    vec3 col = alb * (KEY_COL * dif * sh + amb * 0.8 * ao + bounce * ao)
             + vec3(1.0, 0.35, 0.6) * back * 0.9 * ao * alb
             + vec3(1.0, 0.4, 0.75) * rim * 0.35 * ao
             + KEY_COL * sp * sh;
    // a faint neon spill from the doorway onto the temple floor and columns
    vec3 dp = p - vec3(0.0, 2.5, 37.0);
    col += vec3(1.0, 0.25, 0.75) * NEON * 0.6 * alb / (1.0 + dot(dp, dp) * 0.25) * ao;
    return col;
}

vec3 fogged(vec3 col, vec3 rd, float t) {
    float f = 1.0 - exp(-t * 0.005);
    vec3 fc = mix(horizonCol(), vec3(0.55, 0.15, 0.55), smoothstep(0.0, 0.4, rd.y));
    return mix(col, fc, f * 0.85);
}

vec3 aces(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

vec3 hueRot(vec3 c, float a) {
    vec3 k = vec3(0.57735);
    float ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

void main() {
    vec2 frag = gl_FragCoord.xy;
    vec2 uv = (2.0 * frag - RENDERSIZE) / RENDERSIZE.y;
    float T = TIME * SPEED;

    // a slow dolly along the near end of the pool, always looking at the temple
    vec3 ro = vec3(4.5 * sin(T * 0.11), 2.15 + 0.35 * sin(T * 0.17), -4.0 + 2.5 * sin(T * 0.07 + 1.0));
    vec3 ta = vec3(1.8 * sin(T * 0.09 + 0.5), 4.6, 32.0);
    vec3 fw = normalize(ta - ro);
    vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), fw));
    vec3 up = cross(fw, rt);
    vec3 rd = normalize(uv.x * rt + uv.y * up + 1.55 * fw);

    // the water plane, if this ray reaches it inside the pool
    float tw = rd.y < 0.0 ? -ro.y / rd.y : 1e5;
    vec3 pw = ro + rd * tw;
    bool wcand = abs(pw.x) < 9.0 && pw.z > 0.0 && pw.z < 28.0;
    vec3 wn = vec3(0.0, 1.0, 0.0);
    vec3 rr = rd;
    if (wcand) {
        float amp = WAVES / (1.0 + tw * 0.04);
        float e = 0.05;
        float hx = waveH(pw.xz + vec2(e, 0.0)) - waveH(pw.xz - vec2(e, 0.0));
        float hz = waveH(pw.xz + vec2(0.0, e)) - waveH(pw.xz - vec2(0.0, e));
        wn = normalize(vec3(-hx * amp / (2.0 * e), 1.0, -hz * amp / (2.0 * e)));
        rr = reflect(rd, wn);
        rr.y = max(rr.y, 0.01);
        rr = normalize(rr);
    }

    // One march loop serves both rays: when the primary ray gets past the
    // water plane it turns into the reflection ray. Keeps the scene (and the
    // shading below) inlined once, which is what the compile time pays for.
    vec3 cro = ro, crd = rd;
    float t = 0.02, m = -1.0;
    float tmax = worldExit(ro, rd);
    if (wcand) tmax = min(tmax, tw);
    bool water = false;
    for (int i = 0; i < 200; i++) {
        vec2 h = map(cro + crd * t);
        if (h.x < 0.0012 * (t + (water ? tw : 0.0))) { m = h.y; break; }
        t += h.x;
        if (t > tmax) {
            if (wcand && !water) {
                water = true;
                cro = pw + vec3(0.0, 0.01, 0.0);
                crd = rr;
                t = 0.02;
                tmax = worldExit(cro, crd);
            } else break;
        }
    }
    if (wcand && !water && m < 0.0) water = true;

    float tt = water ? tw + t : t;
    vec3 col;
    if (m > 0.0) {
        vec3 p = cro + crd * t;
        vec3 n = calcNormal(p, tt);
        col = shade(p, n, crd, m, tt, !water);
        // terrace near the pool edge picks up rippling light from the water
        if (m > 1.5 && m < 2.5) {
            float ed = max(abs(p.x) - 9.0, max(-p.z, p.z - 28.0));
            col += vec3(0.2, 0.8, 0.9) * 0.25 * caustic(p.xz * 1.3) * smoothstep(1.2, 0.0, ed) * step(0.0, ed) * CAUSTICS;
        }
        col = fogged(col, crd, tt);
    } else {
        col = sky(cro, crd);
    }

    if (water) {
        vec3 rf = refract(rd, wn, 1.0 / 1.333);
        vec3 under = poolInterior(pw, rf, tw);
        float fr = 0.02 + 0.98 * pow(1.0 - clamp(dot(-rd, wn), 0.0, 1.0), 5.0);
        fr = mix(fr, 1.0, 0.05);
        col = fogged(mix(under, col, fr), rd, tw);
    }

    // ---- vaporwave grade ----
    col *= EXPOSURE;
    col = aces(col);
    col = pow(col, vec3(0.92, 1.0, 0.88));
    col = col * 0.97 + vec3(0.02, 0.0, 0.04);
    col = hueRot(col, HUE);

    // CRT scanlines, a drifting tracking band, grain and vignette
    float px = max(RENDERSIZE.y / 360.0, 1.0);
    float scan = 0.5 + 0.5 * cos(frag.y / px * PI);
    col *= 1.0 - SCANLINES * 0.22 * scan;
    float by = fract(TIME * 0.07) * 1.4 - 0.2;
    float vy = frag.y / RENDERSIZE.y;
    float band = smoothstep(0.03, 0.0, abs(vy - by));
    col += SCANLINES * band * 0.08 * vec3(1.0, 0.6, 1.0) * (0.5 + hash21(vec2(floor(frag.x / 3.0), floor(TIME * 30.0))));
    col += (hash21(frag + fract(TIME) * 100.0) - 0.5) * 0.035;
    vec2 q = frag / RENDERSIZE;
    col *= 0.55 + 0.45 * pow(16.0 * q.x * q.y * (1.0 - q.x) * (1.0 - q.y), 0.2);

    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}

