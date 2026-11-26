/*{
  "DESCRIPTION": "The Windows XP window-trail glitch, rendered with the gloss the era actually wanted. A dialog is being dragged across a Bliss-green hill, and the desktop never repaints behind it, so every position it has ever occupied stays painted on the screen. The trail is not decoration: each copy in the cascade is the SAME window sampled at a point in its own past, drawn oldest first, so what you are looking at is literally the path it has been dragged along. Some stale copies never finished repainting and smear one scanline sideways across their own body. Over the top of that is the Frutiger Aero treatment the period was reaching for and the hardware could not quite manage - Luna titlebars with a specular sweep riding across them, soap bubbles with iridescent films, a sun with lens ghosts, oversaturated skies, bloom, a taskbar with task buttons and a system tray, desktop icons, and a start menu. Everything is drawn from signed distance fields; there is no texture anywhere. Twenty-two sliders and one switch: speed, how many copies survive, the gap between them, drag distance, per-copy glitch jitter, tearing, how much the oldest copies wash out, Aero glass, bubbles, sun flare, cloud cover, vividness, bloom, taskbar, scanlines, vignette, plus how many windows are being dragged at once, desktop icons, the two-pane Luna start menu, open or shut, and the pointer that is still holding the live window's titlebar, and three that decide what the failed repaint left behind - text returning as characters from no alphabet, query dialogs and stray question marks, and rows that tear sideways or invert into a stale selection block.",
  "CREDIT": "CC0",
  "CATEGORIES": ["generator", "windows-xp", "glitch", "frutiger-aero", "nostalgia", "2d", "sdf"],
  "INPUTS": [
    { "NAME": "uSpeed",    "TYPE": "float", "DEFAULT": 1.00,  "MIN": 0.0,   "MAX": 3.0 },
    { "NAME": "uCount",    "TYPE": "float", "DEFAULT": 34.0,  "MIN": 1.0,   "MAX": 48.0 },
    { "NAME": "uTrail",    "TYPE": "float", "DEFAULT": 0.130, "MIN": 0.005, "MAX": 0.25 },
    { "NAME": "uDrift",    "TYPE": "float", "DEFAULT": 1.00,  "MIN": 0.0,   "MAX": 2.0 },
    { "NAME": "uJitter",   "TYPE": "float", "DEFAULT": 0.35,  "MIN": 0.0,   "MAX": 3.0 },
    { "NAME": "uSmear",    "TYPE": "float", "DEFAULT": 0.45,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uFade",     "TYPE": "float", "DEFAULT": 0.18,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uGlass",    "TYPE": "float", "DEFAULT": 0.55,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uBubbles",  "TYPE": "float", "DEFAULT": 0.60,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uFlare",    "TYPE": "float", "DEFAULT": 0.70,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uClouds",   "TYPE": "float", "DEFAULT": 0.75,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uSat",      "TYPE": "float", "DEFAULT": 1.15,  "MIN": 0.0,   "MAX": 2.0 },
    { "NAME": "uBloom",    "TYPE": "float", "DEFAULT": 0.45,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uTaskbar",  "TYPE": "float", "DEFAULT": 1.00,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uScan",     "TYPE": "float", "DEFAULT": 0.15,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uVignette", "TYPE": "float", "DEFAULT": 0.30,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uCascades", "TYPE": "float", "DEFAULT": 1.00,  "MIN": 1.0,   "MAX": 4.0 },
    { "NAME": "uIcons",    "TYPE": "float", "DEFAULT": 1.00,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uStart",    "TYPE": "bool",  "DEFAULT": false },
    { "NAME": "uCursor",   "TYPE": "float", "DEFAULT": 1.00,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uAlien",    "TYPE": "float", "DEFAULT": 0.40,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uQuestion", "TYPE": "float", "DEFAULT": 0.45,  "MIN": 0.0,   "MAX": 1.0 },
    { "NAME": "uGarble",   "TYPE": "float", "DEFAULT": 0.40,  "MIN": 0.0,   "MAX": 1.0 }
  ]
}*/

// NOT RESPONDING - ISF port of the Shadertoy/WebGL2 version
// (GLSL ES 1.00: gl_FragColor, TIME/RENDERSIZE).

#define PI 3.14159265359
// ---- the virtual desktop is 1280x720 -----------------------
const vec2 SCREEN = vec2(1280.0, 720.0);
// The visible desktop width, which follows the viewport's aspect. Anchoring
// the UI to a fixed 1280 box and centring it left gaps at both ends on
// anything wider than 16:9 — the taskbar floated, the icons sat inset.
float gSW;
const vec2 WSZ    = vec2(430.0, 196.0);   // dialog size, in desktop pixels
float hash11(float n){ return fract(sin(n) * 43758.5453123); }
float hash12(vec2 p){
 p = fract(p * vec2(123.34, 456.21));
 p += dot(p, p + 45.32);
 return fract(p.x * p.y);
}
float vnoise2(vec2 p){
 vec2 i = floor(p), f = fract(p);
 f = f * f * (3.0 - 2.0 * f);
 float a = hash12(i);
 float b = hash12(i + vec2(1.0, 0.0));
 float c = hash12(i + vec2(0.0, 1.0));
 float d = hash12(i + vec2(1.0, 1.0));
 return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
float fbm2(vec2 p){
 float s = 0.0, a = 0.5;
 for (int i = 0; i < 5; i++){
  s += a * vnoise2(p);
  p *= 2.03;
  a *= 0.5;
 }
 return s;
}
// Billow noise — the ridges of |2n-1| flipped, which gives rounded lumps
// instead of fbm's smooth hills. This is what makes cumulus look cauliflowered
// rather than smeared.
float billow2(vec2 p){
 float s = 0.0, a = 0.5;
 for (int i = 0; i < 5; i++){
  s += a * (1.0 - abs(2.0 * vnoise2(p) - 1.0));
  p *= 2.11;
  a *= 0.5;
 }
 return s;
}
// signed distance to a rounded box, in pixels
float rbox(vec2 p, vec2 hs, float r){
 vec2 d = abs(p) - hs + r;
 return min(max(d.x, d.y), 0.0) + length(max(d, 0.0)) - r;
}
// 1 inside, 0 outside, antialiased over ~1 desktop pixel
float fill(float d, float aa){ return smoothstep(aa, -aa, d); }
// distance to a 2D segment
float sdSeg2(vec2 p, vec2 a, vec2 b){
 vec2 pa = p - a, ba = b - a;
 float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
 return length(pa - ba * h);
}
// A question mark, built from a hook of capsules and a dot rather than a
// masked ring — no angle test, so nothing aliases at small sizes.
float sdQuestion(vec2 p, float s){
 p /= s;
 float d = sdSeg2(p, vec2(-3.2, -4.4), vec2(-1.2, -7.1));
 d = min(d, sdSeg2(p, vec2(-1.2, -7.1), vec2( 1.9, -6.7)));
 d = min(d, sdSeg2(p, vec2( 1.9, -6.7), vec2( 3.2, -4.0)));
 d = min(d, sdSeg2(p, vec2( 3.2, -4.0), vec2( 0.7, -1.0)));
 d = min(d, sdSeg2(p, vec2( 0.7, -1.0), vec2( 0.0,  2.0)));
 d -= 1.35;
 d = min(d, length(p - vec2(0.0, 5.2)) - 1.5);
 return d * s;
}
// One character of an unknown script: a few strokes thrown down by hash,
// sometimes with a ring or a dot. Reads as writing without being any.
float alienGlyph(vec2 p, float sd, float aa){
 float d = 1e9;
 for (int k = 0; k < 3; k++){
  float fk = float(k);
  vec2  p0 = vec2((hash11(sd * 13.0 + fk * 5.1) - 0.5) * 7.0,
      (hash11(sd * 29.0 + fk * 3.7) - 0.5) * 9.0);
  float an = hash11(sd * 7.0 + fk * 11.3) * PI;
  float ln = 1.6 + 3.4 * hash11(sd * 17.0 + fk * 2.3);
  vec2  dr = vec2(cos(an), sin(an)) * ln;
  d = min(d, sdSeg2(p, p0 - dr, p0 + dr) - 1.05);
 }
 float extra = hash11(sd * 41.0);
 if (extra > 0.78)      d = min(d, abs(length(p) - 3.4) - 1.0);
 else if (extra > 0.62) d = min(d, length(p - vec2(0.0, -4.0)) - 1.3);
 return fill(d, aa);
}
float sdTriangle(vec2 p, vec2 p0, vec2 p1, vec2 p2){
 vec2 e0 = p1 - p0, e1 = p2 - p1, e2 = p0 - p2;
 vec2 v0 = p - p0,  v1 = p - p1,  v2 = p - p2;
 vec2 q0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
 vec2 q1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
 vec2 q2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
 float s = sign(e0.x * e2.y - e0.y * e2.x);
 vec2 d = min(min(vec2(dot(q0, q0), s * (v0.x * e0.y - v0.y * e0.x)),
     vec2(dot(q1, q1), s * (v1.x * e1.y - v1.y * e1.x))),
     vec2(dot(q2, q2), s * (v2.x * e2.y - v2.y * e2.x)));
 return -sqrt(d.x) * sign(d.y);
}
// ---- 4x6 bit-packed font -------------------------------------
// 4 cols x 6 rows = 24 bits, exactly float32's exact-integer limit,
// so one glyph is one float. GLSL ES 1.00 has no bitwise ops, so
// bits come out with mod(floor(v / exp2(i)), 2.0).
float glyphBits(float c){
 if (c < 2.5) return 630678.0;
 else if (c < 3.5) return 497559.0;
 else if (c < 4.5) return 921886.0;
 else if (c < 5.5) return 498071.0;
 else if (c < 6.5) return 988959.0;
 else if (c < 7.5) return 71455.0;
 else if (c < 8.5) return 957726.0;
 else if (c < 9.5) return 630681.0;
 else if (c < 10.5) return 467495.0;
 else if (c < 11.5) return 152654.0;
 else if (c < 12.5) return 611161.0;
 else if (c < 13.5) return 987409.0;
 else if (c < 14.5) return 630777.0;
 else if (c < 15.5) return 647097.0;
 else if (c < 16.5) return 432534.0;
 else if (c < 17.5) return 71575.0;
 else if (c < 18.5) return 973206.0;
 else if (c < 19.5) return 612247.0;
 else if (c < 20.5) return 493086.0;
 else if (c < 21.5) return 139823.0;
 else if (c < 22.5) return 432537.0;
 else if (c < 23.5) return 420249.0;
 else if (c < 24.5) return 458649.0;
 else if (c < 25.5) return 628377.0;
 else if (c < 26.5) return 140953.0;
 else if (c < 27.5) return 987727.0;
 else if (c < 28.5) return 433110.0;
 else if (c < 29.5) return 467506.0;
 else if (c < 30.5) return 988807.0;
 else if (c < 31.5) return 493191.0;
 else if (c < 32.5) return 282453.0;
 else if (c < 33.5) return 493343.0;
 else if (c < 34.5) return 431894.0;
 else if (c < 35.5) return 140431.0;
 else if (c < 36.5) return 431766.0;
 else if (c < 37.5) return 429718.0;
 else if (c < 38.5) return 8224.0;
 else if (c < 39.5) return 131072.0;
 else if (c < 40.5) return 3840.0;
 return 0.0;
}
// ---- string table: 6 bits per char, 4 chars per float ---------
vec4 strPack(float s){
 if (s < 0.5) return vec4(4990292.0, 21.0, 0.0, 0.0);   // START
 else if (s < 1.5) return vec4(1054350.0, 5837712.0, 78229.0, 0.0);   // MY COMPUTER
 else if (s < 2.5) return vec4(6832531.0, 287556.0, 62083.0, 0.0);   // RECYCLE BIN
 else if (s < 3.5) return vec4(1316494.0, 3760400.0, 5329862.0, 0.0);   // MY DOCUMENTS
 else if (s < 4.5) return vec4(1659850.0, 5530579.0, 0.0, 0.0);   // INTERNET
 else if (s < 5.5) return vec4(1319315.0, 5665166.0, 1369.0, 0.0);   // README.TXT
 else if (s < 6.5) return vec4(2642318.0, 3477570.0, 5006978.0, 0.0);   // MEDIA PLAYER
 else if (s < 7.5) return vec4(4207562.0, 25.0, 0.0, 0.0);   // INBOX
 else if (s < 8.5) return vec4(4462222.0, 5853450.0, 82323.0, 0.0);   // MY PICTURES
 else if (s < 9.5) return vec4(5853588.0, 6580689.0, 6.0, 0.0);   // SETUP.EXE
 else if (s < 10.5) return vec4(5567492.0, 316435.0, 1634449.0, 13.0);   // CONTROL PANEL
 else if (s < 11.5) return vec4(4510089.0, 1372289.0, 4547841.0, 5583889.0);   // HELP AND SUPPORT
 else if (s < 12.5) return vec4(4989332.0, 580.0, 0.0, 0.0);   // SEARCH
 else if (s < 13.5) return vec4(10286483.0, 2535.0, 0.0, 0.0);   // RUN...
 else if (s < 14.5) return vec4(316226.0, 2163921.0, 5300371.0, 0.0);   // ALL PROGRAMS
 else if (s < 15.5) return vec4(295949.0, 29136.0, 0.0, 0.0);   // LOG OFF
 else if (s < 16.5) return vec4(4011413.0, 1864705.0, 0.0, 0.0);   // TURN OFF
 else if (s < 17.5) return vec4(1659919.0, 20625.0, 0.0, 0.0);   // NOTEPAD
 else if (s < 18.5) return vec4(3973265.0, 21.0, 0.0, 0.0);   // PAINT
 else if (s < 19.5) return vec4(5325198.0, 1606598.0, 19.0, 0.0);   // MESSENGER
 else if (s < 20.5) return vec4(3675790.0, 1090838.0, 0.0, 0.0);   // MY MUSIC
 else if (s < 21.5) return vec4(8284445.0, 28.0, 0.0, 0.0);   // 18:30
 else if (s < 22.5) return vec4(2446993.0, 1577936.0, 5047507.0, 0.0);   // PYTHON ERROR
 else if (s < 23.5) return vec4(349199.0, 4538771.0, 2642896.0, 527.0);   // NOT RESPONDING
 else if (s < 24.5) return vec4(3937934.0, 4293958.0, 787.0, 0.0);   // MY NETWORK
 else if (s < 25.5) return vec4(3495312.0, 50192.0, 0.0, 0.0);   // OUTLOOK
 else if (s < 26.5) return vec4(3479110.0, 5006544.0, 0.0, 0.0);   // EXPLORER
 else if (s < 27.5) return vec4(2675732.0, 5021845.0, 6.0, 0.0);   // SOLITAIRE
 else if (s < 28.5) return vec4(10326153.0, 26182.0, 0.0, 0.0);   // HAX.EXE
 else if (s < 29.5) return vec4(287234.0, 1577424.0, 5022798.0, 7869702.0);   // AGE OF EMPIRES 2
 else if (s < 30.5) return vec4(1630861.0, 1651352.0, 0.0, 0.0);   // LIMEWIRE
 else if (s < 31.5) return vec4(283590.0, 99343.0, 0.0, 0.0);   // END NOW
 else if (s < 32.5) return vec4(1110148.0, 838.0, 0.0, 0.0);   // CANCEL
 else if (s < 33.5) return vec4(316418.0, 5325198.0, 1606598.0, 19.0);   // AOL MESSENGER
 else if (s < 34.5) return vec4(324878.0, 5325198.0, 1606598.0, 19.0);   // MSN MESSENGER
 else if (s < 35.5) return vec4(584198.0, 842.0, 0.0, 0.0);   // E-MAIL
 else if (s < 36.5) return vec4(6035086.0, 4219210.0, 20.0, 0.0);   // MY VIDEOS
 else if (s < 37.5) return vec4(3974353.0, 5321109.0, 0.0, 0.0);   // PRINTERS
 else if (s < 38.5) return vec4(3994628.0, 348422.0, 1045.0, 0.0);   // CONNECT TO
 else if (s < 39.5) return vec4(287234.0, 464.0, 0.0, 0.0);   // AGE OF
 else if (s < 40.5) return vec4(2691974.0, 344467.0, 30.0, 0.0);   // EMPIRES 2
 return vec4(0.0);
}
// One character out of the packed string. 6 bits each, 4 per component.
float charAt(vec4 s, float i){
 if (i > 15.5 || i < 0.0) return 0.0;
 float g = floor(i / 4.0);
 float v = (g < 0.5) ? s.x : (g < 1.5) ? s.y : (g < 2.5) ? s.z : s.w;
 return mod(floor(v / exp2(6.0 * mod(i, 4.0))), 64.0);
}
// Real text. `h` is HALF the cap height; d is relative to the left edge and
// the vertical centre of the line. Returns coverage, 2x2 supersampled so the
// bitmap doesn't crawl when the desktop is scaled by a non-integer factor.
float textCov(vec2 d, float sid, float h){
 // cheap box reject FIRST — strPack and glyphBits are long if-ladders and
 // this is called dozens of times per pixel across the whole UI
 if (d.x < -1.5 || d.x > 32.0 * h || abs(d.y) > h * 1.8) return 0.0;
 vec4  s  = strPack(sid);
 float px = 2.0 * h / 5.0;          // one font pixel; 5 rows make the cap
 float adv = 5.0 * px;              // 4 columns + 1 of spacing
 float cov = 0.0;
 for (int k = 0; k < 4; k++){
  float fk = float(k);
  vec2 o = (vec2(mod(fk, 2.0), floor(fk / 2.0)) - 0.5) * px * 0.5;
  vec2 e = d + o;
  float ci = floor(e.x / adv);
  if (ci < 0.0 || ci > 15.5) continue;
  float ch = charAt(s, ci);
  if (ch < 1.5) continue;                       // end or space
  float cx = floor(mod(e.x, adv) / px);
  float ry = floor((e.y + h) / px);             // row 0 at the cap top
  if (cx > 3.5 || ry < 0.0 || ry > 5.5) continue;
  cov += mod(floor(glyphBits(ch) / exp2(ry * 4.0 + cx)), 2.0);
 }
 return cov * 0.25;
}
// draw a string; at.x = left edge, at.y = line centre
void drawText(inout vec3 col, vec2 q, vec2 at, float sid, float h, vec3 c, float alpha){
 col = mix(col, c, textCov(q - at, sid, h) * alpha);
}
// how wide a string will be, for centring
float textW(float sid, float h){
 vec4 s = strPack(sid);
 float n = 0.0;
 for (int i = 0; i < 16; i++){
  if (charAt(s, float(i)) < 0.5) break;
  n += 1.0;
 }
 return n * 5.0 * (2.0 * h / 5.0);
}
// A run of "text". A row of identical blocks reads as squares, not words —
// so every character gets its own box: cap-height or x-height sitting on a
// shared baseline, varying width, the odd descender, and gaps where the
// spaces fall. Domain repetition off x means the whole run costs O(1)
// instead of a loop, which matters because this is called everywhere.
// h is HALF the cap height; at.x is the left edge, at.y the line centre.
float sdTextRun(vec2 d, float w, float h, float seed){
 if (d.x < -1.0 || d.x > w + 1.0 || abs(d.y) > h * 2.4) return 1e9;
 float cw = h * 1.72;                                  // character advance
 float ci = floor(d.x / cw);
 float fx = mod(d.x, cw) - cw * 0.5;
 float r1 = hash11(seed * 1.7 + ci * 3.7);
 float r2 = hash11(seed * 2.3 + ci * 9.1);
 if (r1 > 0.87) return 1e9;                            // a space
 float gw = cw * (0.24 + 0.15 * r2);
 float gh = mix(0.68, 1.0, step(0.52, r2)) * h;        // x-height or cap
 float cy = h - gh;                                    // sit on the baseline
 if (r1 < 0.11){ gh = h * 0.95; cy = h * 0.28; }       // a descender
 return rbox(vec2(fx, d.y - cy), vec2(gw, gh), 0.35);
}
void textRun(inout vec3 col, vec2 q, vec2 at, float w, float h, vec3 c,
   float seed, float aa, float alpha){
 col = mix(col, c, fill(sdTextRun(q - at, w, h, seed), aa) * alpha);
}
// ============================================================
// THE DESKTOP — Bliss, but glossier than it ever really was
// ============================================================
vec3 desktop(vec2 p, float t, float aa){
 vec2 uvn = p / SCREEN;
 // sky: deep at the top, washing pale toward the horizon.
 // NB uvn.y is 0 at the TOP here — p was flipped to y-down for the UI.
 vec3 col = mix(vec3(0.16, 0.42, 0.86), vec3(0.60, 0.80, 0.97), pow(uvn.y, 0.85));
 col = mix(col, vec3(0.80, 0.90, 0.99), pow(uvn.y, 3.5) * 0.75);
 // sun, high right
 vec2 sunP = vec2(gSW * 0.84, 150.0);
 float sd  = length(p - sunP);
 col += vec3(1.0, 0.96, 0.86) * exp(-sd * 0.010) * 1.30 * uFlare;
 col += vec3(1.0, 0.90, 0.72) * exp(-sd * 0.0022) * 0.42 * uFlare;
 // ---- clouds, in the two kinds Bliss actually has ----
 // 1. cirrus: thin feathered streaks high up, stretched hard in x
 vec2 cirp = p * vec2(0.0016, 0.0090) + vec2(t * 0.011, 0.0);
 float cir = fbm2(cirp);
 cir  = smoothstep(0.50, 0.74, cir) * smoothstep(0.46, 0.01, uvn.y);
 cir *= 0.40 + 0.60 * fbm2(cirp * 3.1);                    // break them into wisps
 col  = mix(col, vec3(0.98, 0.99, 1.0), clamp(cir, 0.0, 1.0) * 0.70 * uClouds);
 // 2. cumulus: rounded billows in a band across the middle and along the
 //    horizon, bright white on top with a faintly cooler underside
 vec2 cum = p * 0.0024 + vec2(t * 0.008, 0.0);
 float bil = billow2(cum * vec2(1.0, 1.8));
 // a broad band through the middle, still going at the horizon
 float band = smoothstep(0.10, 0.34, uvn.y) * smoothstep(0.72, 0.44, uvn.y) + 0.10;
 // a narrow threshold window is what gives cumulus a defined edge instead
 // of blanketing the sky in haze — the blue between them matters as much
 float cmask = smoothstep(0.575, 0.645, bil * (0.62 + 0.62 * band)) * uClouds;
 // the underside is where the billow falls away downward
 float under = smoothstep(0.60, 0.42, billow2(cum * vec2(1.0, 1.8) + vec2(0.0, 0.06)));
 vec3 cumC = mix(vec3(1.0), vec3(0.78, 0.85, 0.94), under * 0.70);
 col = mix(col, cumC, clamp(cmask, 0.0, 1.0));
 // the hill — Bliss's one big green curve
 float hy = 470.0
   + 62.0 * sin(p.x * 0.0042 + 0.6)
   + 26.0 * sin(p.x * 0.0113 + 2.1)
   + 10.0 * fbm2(vec2(p.x * 0.010, 3.0));
 float grass = fill(hy - p.y, aa * 1.5);
 if (grass > 0.001){
  float below = p.y - hy;                              // 0 at the ridge
  float depth = clamp(below / 250.0, 0.0, 1.0);
  // Bliss's green is a bright yellow-green on the crown falling to a
  // deeper green in the near field
  vec3 g = mix(vec3(0.47, 0.76, 0.16), vec3(0.20, 0.50, 0.09), depth);
  g = mix(g, vec3(0.66, 0.86, 0.28), pow(1.0 - depth, 5.0) * 0.75);   // sunlit crown
  // contour banding from the ploughing, following the hill. sqrt packs
  // the bands tighter toward the ridge, which is the perspective.
  float bandPhase = sqrt(max(below, 0.0)) * 2.05
      + 1.6 * sin(p.x * 0.0043)            // the lines aren't straight
      + 0.7 * fbm2(vec2(p.x * 0.004, 1.0));
  g *= 1.0 + 0.055 * sin(bandPhase) * smoothstep(0.0, 40.0, below);
  // mottling and a fine vertical blade grain that coarsens toward us
  g *= 0.90 + 0.20 * fbm2(p * vec2(0.045, 0.14) + 11.0);
  g *= 0.94 + 0.13 * vnoise2(p * vec2(0.9 + depth * 2.2, 0.07));
  // the dark hedgerow shadow strip across the low left
  float hedge = smoothstep(24.0, 0.0, abs(below - 205.0 - 26.0 * sin(p.x * 0.0052)))
     * smoothstep(760.0, 420.0, p.x);
  g = mix(g, vec3(0.10, 0.24, 0.06), hedge * 0.72);
  // buttercups in the near field
  if (below > 150.0){
   vec2 fg  = p * vec2(0.075, 0.135);
   vec2 fid = floor(fg);
   if (hash12(fid) > 0.90){
    vec2 fp = fract(fg) - 0.5;
    fp -= 0.34 * (vec2(hash12(fid + 1.7), hash12(fid + 3.3)) - 0.5);
    g = mix(g, vec3(0.95, 0.92, 0.30),
      smoothstep(0.17, 0.06, length(fp)) * smoothstep(150.0, 260.0, below));
   }
  }
  col = mix(col, g, grass);
 }
 // soap bubbles — the single most Frutiger Aero object there is.
 // Thin-film interference, refraction of what's behind, a Fresnel rim
 // that brightens toward the edge, a sharp specular and a soft second one.
 if (uBubbles > 0.001){
  for (int i = 0; i < 16; i++){
   float fi = float(i);
   float sd0 = hash11(fi * 7.31);
   float rad = 10.0 + 52.0 * hash11(fi * 3.17);
   float life = 9.0 + 9.0 * sd0;
   float ph = fract(t / life + sd0);
   vec2 bp = vec2(gSW * hash11(fi * 5.11)
       + (34.0 + 30.0 * sd0) * sin(t * (0.35 + 0.4 * sd0) + fi * 2.1),
      SCREEN.y * 1.10 - ph * (SCREEN.y * 1.32));
   // they wobble as they rise — surface tension fighting the drag
   float wob = 1.0 + 0.05 * sin(t * 2.3 + fi * 4.0);
   vec2 d = (p - bp) * vec2(wob, 1.0 / wob);
   float dl = length(d);
   if (dl > rad * 1.25) continue;
   float fadeB = smoothstep(0.0, 0.12, ph) * smoothstep(1.0, 0.78, ph) * uBubbles;
   float body  = fill(dl - rad, aa);
   float nz    = clamp(1.0 - dl / rad, 0.0, 1.0);      // "depth" toward the middle
   float grz   = 1.0 - sqrt(nz);                        // 0 centre, 1 at the rim
   // refraction: sample the sky a little offset, so it acts like glass
   vec2 off = d / rad * 26.0 * grz;
   vec3 behind = mix(vec3(0.09, 0.34, 0.78), vec3(0.55, 0.80, 0.99),
       clamp((p.y + off.y) / SCREEN.y, 0.0, 1.0));
   // thin-film: the interference order runs with the path length
   float ord = (0.55 + 2.6 * grz) * (0.7 + 0.6 * hash11(fi * 2.9));
   vec3 film = 0.5 + 0.5 * cos(6.2831 * ord + vec3(0.0, 2.09, 4.19) + fi);
   film = mix(vec3(dot(film, vec3(0.33))), film, 0.85);
   vec3 bub = mix(behind, film, 0.42 + 0.34 * grz);
   bub += vec3(1.0) * pow(grz, 5.0) * 0.55;                    // Fresnel rim
   col = mix(col, bub, body * (0.30 + 0.42 * grz) * fadeB);
   col += vec3(1.0) * fill(abs(dl - rad) - 1.3, aa) * 0.40 * fadeB;   // edge line
   // speculars: one hard, one soft and larger
   col += vec3(1.0) * fill(length(d - vec2(-rad * 0.38, -rad * 0.42)) - rad * 0.13, aa) * 0.90 * fadeB;
   col += vec3(0.85, 0.94, 1.0) * fill(length(d - vec2(rad * 0.34, rad * 0.30)) - rad * 0.22, aa) * 0.22 * fadeB;
  }
 }
 // anamorphic-ish flare streak through the sun
 if (uFlare > 0.001){
  vec2 fd = p - sunP;
  col += vec3(0.75, 0.88, 1.0) * exp(-abs(fd.y) * 0.10) * exp(-abs(fd.x) * 0.0016) * 0.30 * uFlare;
  // ghosts along the sun-centre axis
  vec2 axis = vec2(gSW, SCREEN.y) * 0.5 - sunP;
  for (int i = 1; i < 4; i++){
   float fi = float(i);
   vec2 gp = sunP + axis * (fi * 0.65);
   float gd = length(p - gp);
   col += vec3(0.55, 0.75, 0.95) * exp(-gd * 0.020) * 0.13 * uFlare / fi;
  }
 }
 return col;
}
// A small version of the desktop icon set, for the start menu and the
// taskbar. q is relative to the icon centre; s is its half-size.
void miniIcon(inout vec3 col, vec2 q, float kind, float s, float aaPx){
 if (max(abs(q.x), abs(q.y)) > s * 1.3) return;      // cheap reject
 q /= s;                                   // work in a unit-ish box, +-1
 // aa arrives in DESKTOP pixels but q is now in units of s — using it
 // unconverted made every small icon s times too soft, which is what the
 // blurry start-menu and taskbar icons were
 float aa = aaPx / s;
 float sh = clamp(q.y * 0.5 + 0.5, 0.0, 1.0);
 if (kind < 0.5){                          // monitor
  col = mix(col, vec3(0.88, 0.87, 0.82), fill(rbox(q - vec2(0.0, -0.15), vec2(1.0, 0.78), 0.16), aa));
  col = mix(col, mix(vec3(0.16, 0.44, 0.86), vec3(0.52, 0.78, 0.99), 1.0 - sh),
    fill(rbox(q - vec2(0.0, -0.2), vec2(0.74, 0.54), 0.08), aa));
  col = mix(col, vec3(0.78, 0.77, 0.72), fill(rbox(q - vec2(0.0, 0.82), vec2(0.62, 0.16), 0.08), aa));
 } else if (kind < 1.5){                   // recycle bin
  vec2 bq = q - vec2(0.0, 0.22); bq.x *= 1.0 + bq.y * 0.22;
  col = mix(col, mix(vec3(0.44, 0.58, 0.72), vec3(0.70, 0.82, 0.92), 1.0 - sh),
    fill(rbox(bq, vec2(0.62, 0.68), 0.14), aa));
  col = mix(col, vec3(0.74, 0.85, 0.94), fill(length((q - vec2(0.0, -0.55)) * vec2(1.0, 2.4)) - 0.74, aa));
 } else if (kind < 2.5){                   // folder
  col = mix(col, vec3(0.86, 0.64, 0.16), fill(rbox(q - vec2(-0.42, -0.66), vec2(0.5, 0.24), 0.1), aa));
  col = mix(col, vec3(0.96, 0.78, 0.28), fill(rbox(q - vec2(0.0, 0.06), vec2(1.0, 0.7), 0.13), aa));
  col = mix(col, vec3(1.00, 0.88, 0.46), fill(rbox(q - vec2(0.0, 0.25), vec2(1.0, 0.55), 0.13), aa));
 } else if (kind < 3.5){                   // globe
  float g = fill(length(q) - 0.92, aa);
  col = mix(col, mix(vec3(0.08, 0.34, 0.76), vec3(0.36, 0.68, 0.98), 1.0 - sh), g);
  col = mix(col, vec3(0.90, 0.96, 1.0), g * fill(abs(length(q * vec2(1.0, 0.40)) - 0.36) - 0.08, aa));
  col = mix(col, vec3(0.90, 0.96, 1.0), g * fill(abs(q.y) - 0.08, aa));
 } else if (kind < 4.5){                   // document
  col = mix(col, vec3(0.99, 0.99, 0.97), fill(rbox(q, vec2(0.7, 0.92), 0.1), aa));
  col = mix(col, vec3(0.60, 0.63, 0.68), fill(abs(rbox(q, vec2(0.7, 0.92), 0.1)) - 0.06, aa));
  col = mix(col, vec3(0.42, 0.46, 0.54), fill(rbox(q - vec2(0.0, -0.28), vec2(0.4, 0.06), 0.03), aa));
  col = mix(col, vec3(0.42, 0.46, 0.54), fill(rbox(q - vec2(0.0, 0.02), vec2(0.4, 0.06), 0.03), aa));
  col = mix(col, vec3(0.42, 0.46, 0.54), fill(rbox(q - vec2(-0.14, 0.32), vec2(0.26, 0.06), 0.03), aa));
 } else if (kind < 5.5){                   // media
  float g = fill(length(q) - 0.92, aa);
  col = mix(col, mix(vec3(0.24, 0.09, 0.50), vec3(0.60, 0.34, 0.90), 1.0 - sh), g);
  col = mix(col, vec3(1.0), fill(sdTriangle(q - vec2(0.1, 0.0), vec2(-0.3, -0.42), vec2(-0.3, 0.42), vec2(0.42, 0.0)), aa));
 } else if (kind < 6.5){                   // envelope
  col = mix(col, vec3(0.98, 0.97, 0.93), fill(rbox(q, vec2(0.98, 0.68), 0.1), aa));
  col = mix(col, vec3(0.62, 0.62, 0.58), fill(abs(rbox(q, vec2(0.98, 0.68), 0.1)) - 0.06, aa));
  col = mix(col, vec3(0.78, 0.80, 0.85), fill(max(sdSeg2(q, vec2(-0.98, -0.68), vec2(0.0, 0.1)) - 0.08, -q.y - 0.68), aa));
  col = mix(col, vec3(0.78, 0.80, 0.85), fill(max(sdSeg2(q, vec2( 0.98, -0.68), vec2(0.0, 0.1)) - 0.08, -q.y - 0.68), aa));
 } else if (kind < 7.5){                   // photo
  col = mix(col, vec3(0.95, 0.95, 0.91), fill(rbox(q, vec2(0.92, 0.78), 0.1), aa));
  float ph = fill(rbox(q, vec2(0.72, 0.58), 0.05), aa);
  col = mix(col, mix(vec3(0.30, 0.62, 0.92), vec3(0.74, 0.90, 0.99), 1.0 - sh), ph);
  col = mix(col, vec3(0.30, 0.60, 0.20), ph * fill(sdTriangle(q, vec2(-0.72, 0.58), vec2(-0.05, -0.14), vec2(0.46, 0.58)), aa));
  col = mix(col, vec3(0.98, 0.86, 0.34), ph * fill(length(q - vec2(0.38, -0.3)) - 0.17, aa));
 } else if (kind < 8.5){                   // disc
  float g = fill(length(q) - 0.92, aa);
  col = mix(col, mix(vec3(0.70, 0.73, 0.80), vec3(0.94, 0.96, 0.99), 1.0 - sh), g);
  col = mix(col, vec3(0.32, 0.36, 0.44), fill(length(q) - 0.24, aa));
 } else if (kind < 9.5){                   // limewire
  float g = fill(length(q) - 0.92, aa);
  col = mix(col, mix(vec3(0.34, 0.66, 0.06), vec3(0.74, 0.95, 0.22), 1.0 - sh), g);
  col = mix(col, vec3(0.90, 0.98, 0.60), fill(length(q) - 0.60, aa));
  col = mix(col, vec3(0.44, 0.74, 0.12), fill(abs(q.x) - 0.06, aa) * fill(length(q) - 0.60, aa));
 } else if (kind < 10.5){                  // console
  col = mix(col, vec3(0.10, 0.11, 0.13), fill(rbox(q, vec2(0.96, 0.76), 0.12), aa));
  col = mix(col, vec3(0.44, 0.48, 0.55), fill(abs(rbox(q, vec2(0.96, 0.76), 0.12)) - 0.07, aa));
  col = mix(col, vec3(0.30, 0.95, 0.35), fill(rbox(q - vec2(-0.28, 0.12), vec2(0.36, 0.08), 0.04), aa));
  col = mix(col, vec3(0.30, 0.95, 0.35), fill(rbox(q - vec2(-0.46, -0.28), vec2(0.18, 0.08), 0.04), aa));
 } else if (kind < 11.5){                  // shield
  float s2 = rbox(q - vec2(0.0, -0.12), vec2(0.74, 0.56), 0.12);
  s2 = min(s2, sdTriangle(q, vec2(-0.74, 0.42), vec2(0.74, 0.42), vec2(0.0, 1.0)));
  col = mix(col, mix(vec3(0.58, 0.14, 0.10), vec3(0.90, 0.36, 0.24), 1.0 - sh), fill(s2, aa));
  col = mix(col, vec3(0.98, 0.86, 0.42), fill(abs(s2) - 0.09, aa));
 } else if (kind < 12.5){                  // AIM — the running man
  float g = fill(rbox(q, vec2(0.86, 0.92), 0.14), aa);
  col = mix(col, mix(vec3(0.92, 0.78, 0.10), vec3(1.0, 0.94, 0.42), 1.0 - sh), g);
  col = mix(col, vec3(0.20, 0.16, 0.02), g * fill(length(q - vec2(0.06, -0.5)) - 0.2, aa));
  col = mix(col, vec3(0.20, 0.16, 0.02), g * fill(sdSeg2(q, vec2(-0.1, -0.2), vec2(0.2, 0.28)) - 0.12, aa));
  col = mix(col, vec3(0.20, 0.16, 0.02), g * fill(sdSeg2(q, vec2(0.2, 0.28), vec2(-0.2, 0.72)) - 0.1, aa));
  col = mix(col, vec3(0.20, 0.16, 0.02), g * fill(sdSeg2(q, vec2(0.2, 0.28), vec2(0.6, 0.66)) - 0.1, aa));
 } else {                                  // MSN — the butterfly
  vec2 w = vec2(abs(q.x), q.y);
  float b = length((w - vec2(0.42, -0.22)) * vec2(1.0, 1.5)) - 0.44;
  b = min(b, length((w - vec2(0.34, 0.42)) * vec2(1.0, 1.3)) - 0.34);
  col = mix(col, mix(vec3(0.24, 0.62, 0.96), vec3(0.62, 0.86, 1.0), 1.0 - sh), fill(b, aa));
  col = mix(col, vec3(0.10, 0.34, 0.70), fill(rbox(q, vec2(0.09, 0.62), 0.08), aa));
 }
}
// ============================================================
// DESKTOP ICONS — two columns down the left, XP style: a glossy
// 32px tile, then two lines of shadowed white label under it
// ============================================================
void desktopIcons(inout vec3 col, vec2 p, float aa){
 for (int i = 0; i < 12; i++){
  float fi = float(i);
  // XP's grid: hard against the left edge, 76px cells
  vec2 c = vec2(44.0 + floor(fi / 6.0) * 78.0,
     40.0 + mod(fi, 6.0) * 84.0);
  vec2 q = p - c;
  if (dot(q, q) > 4200.0) continue;                 // cheap reject
  float h = hash11(fi * 9.17);
  float shade = clamp(q.y / 30.0 + 0.5, 0.0, 1.0);
  // A BIJECTION, not a hash. Hashing 12 slots into 12 kinds collides by
  // the birthday problem - it shipped with two HAX.EXE and two AGE OF
  // EMPIRES 2. 5 is coprime with 12, so fi*5 mod 12 walks all twelve
  // kinds exactly once and still scatters them (0,5,10,3,8,1,...).
  // Slot 0 lands on kind 0, which puts My Computer top-left like XP.
  float kind = mod(fi * 5.0, 12.0);
  float tint = hash11(fi * 5.77);
  // the label is DERIVED from the kind, not hashed separately — they
  // used to disagree, so a globe could be captioned RECYCLE BIN
  float lbl = kind + 1.0;                       // kinds 0..8 -> strings 1..9
  if (kind > 8.5) lbl = 30.0;                   // LIMEWIRE
  if (kind > 9.5) lbl = 28.0;                   // HAX.EXE
  if (kind > 10.5) lbl = 29.0;                  // AGE OF EMPIRES 2
  if (kind < 0.5){
   // My Computer — a CRT on its stand
   col = mix(col, vec3(0.80, 0.79, 0.74), fill(rbox(q - vec2(0.0, 10.0), vec2(10.0, 2.5), 1.2), aa));
   col = mix(col, vec3(0.86, 0.85, 0.80), fill(rbox(q - vec2(0.0, 7.0), vec2(4.5, 3.5), 1.0), aa));
   col = mix(col, vec3(0.90, 0.89, 0.84), fill(rbox(q - vec2(0.0, -3.0), vec2(15.0, 11.5), 2.5), aa));
   float scr = fill(rbox(q - vec2(0.0, -4.0), vec2(12.0, 8.2), 1.5), aa);
   col = mix(col, mix(vec3(0.10, 0.34, 0.74), vec3(0.42, 0.70, 0.98), shade), scr);
   col = mix(col, vec3(1.0),
     fill(sdTriangle(q, vec2(-11.0, -11.5), vec2(-2.0, -11.5), vec2(-11.0, -2.0)), aa) * scr * 0.40);
  } else if (kind < 1.5){
   // Recycle Bin — a tapered ridged bin with an ellipse lid
   vec2 bq = q - vec2(0.0, 3.5);
   bq.x *= 1.0 + bq.y * 0.030;
   float body = fill(rbox(bq, vec2(9.5, 10.0), 2.0), aa);
   col = mix(col, mix(vec3(0.42, 0.56, 0.70), vec3(0.66, 0.78, 0.88), 1.0 - shade), body);
   col = mix(col, vec3(0.38, 0.50, 0.63), body * fill(abs(mod(q.x + 12.0, 6.0) - 3.0) - 0.85, aa) * 0.65);
   float lid = length((q - vec2(0.0, -8.5)) * vec2(1.0, 2.4)) - 11.0;
   col = mix(col, vec3(0.72, 0.83, 0.92), fill(lid, aa));
   col = mix(col, vec3(0.46, 0.60, 0.74), fill(abs(lid) - 1.2, aa));
  } else if (kind < 2.5){
   // a manila folder
   col = mix(col, vec3(0.86, 0.64, 0.16), fill(rbox(q - vec2(-6.5, -10.5), vec2(8.0, 4.0), 1.5), aa));
   col = mix(col, vec3(0.95, 0.76, 0.26), fill(rbox(q - vec2(0.0, 1.0), vec2(15.0, 11.0), 2.0), aa));
   col = mix(col, vec3(1.00, 0.87, 0.44), fill(rbox(q - vec2(0.0, 4.0), vec2(15.0, 8.5), 2.0), aa));
  } else if (kind < 3.5){
   // the internet, as a globe
   float g = fill(length(q) - 14.0, aa);
   col = mix(col, mix(vec3(0.06, 0.30, 0.70), vec3(0.30, 0.64, 0.96), 1.0 - shade), g);
   col = mix(col, vec3(0.88, 0.95, 1.0), g * fill(abs(length(q * vec2(1.0, 0.40)) - 5.6) - 1.0, aa) * 0.9);
   col = mix(col, vec3(0.88, 0.95, 1.0), g * fill(abs(q.y) - 1.0, aa) * 0.9);
   col = mix(col, vec3(0.88, 0.95, 1.0), g * fill(abs(length(q * vec2(0.40, 1.0)) - 5.6) - 1.0, aa) * 0.9);
   col += vec3(1.0) * fill(length(q - vec2(-5.0, -6.0)) - 4.0, aa) * 0.28;
  } else if (kind < 4.5){
   // a text document, corner turned down
   float pg = rbox(q, vec2(11.0, 14.0), 1.5);
   col = mix(col, vec3(0.99, 0.99, 0.97), fill(pg, aa));
   col = mix(col, vec3(0.62, 0.64, 0.68), fill(abs(pg) - 0.8, aa));
   col = mix(col, vec3(0.84, 0.85, 0.88),
     fill(sdTriangle(q, vec2(4.0, -14.0), vec2(11.0, -14.0), vec2(11.0, -7.0)), aa));
   for (int j = 0; j < 5; j++){
    float fj = float(j);
    col = mix(col, vec3(0.40, 0.44, 0.52),
      fill(rbox(q - vec2(-1.0, -5.0 + fj * 5.0), vec2(6.5, 0.9), 0.4), aa));
   }
  } else if (kind < 5.5){
   // a media player
   float g = fill(length(q) - 14.0, aa);
   col = mix(col, mix(vec3(0.26, 0.10, 0.52), vec3(0.60, 0.34, 0.90), 1.0 - shade), g);
   col = mix(col, vec3(1.0),
     fill(sdTriangle(q - vec2(1.5, 0.0), vec2(-4.5, -6.5), vec2(-4.5, 6.5), vec2(6.5, 0.0)), aa));
   col += vec3(1.0) * fill(length(q - vec2(-5.0, -6.0)) - 4.0, aa) * 0.24;
  } else if (kind < 6.5){
   // an envelope
   col = mix(col, vec3(0.97, 0.96, 0.92), fill(rbox(q, vec2(15.0, 10.5), 1.5), aa));
   col = mix(col, vec3(0.66, 0.66, 0.62), fill(abs(rbox(q, vec2(15.0, 10.5), 1.5)) - 0.8, aa));
   // the flap, two edges meeting in the middle
   col = mix(col, vec3(0.80, 0.82, 0.86),
     fill(max(sdSeg2(q, vec2(-15.0, -10.5), vec2(0.0, 1.5)) - 1.1,
       -q.y - 10.5), aa));
   col = mix(col, vec3(0.80, 0.82, 0.86),
     fill(max(sdSeg2(q, vec2( 15.0, -10.5), vec2(0.0, 1.5)) - 1.1,
       -q.y - 10.5), aa));
  } else if (kind < 7.5){
   // a photo in a frame
   col = mix(col, vec3(0.94, 0.94, 0.90), fill(rbox(q, vec2(14.0, 12.0), 1.5), aa));
   float ph = fill(rbox(q, vec2(11.0, 9.0), 0.8), aa);
   col = mix(col, mix(vec3(0.30, 0.62, 0.92), vec3(0.72, 0.88, 0.99), 1.0 - shade), ph);
   col = mix(col, vec3(0.30, 0.60, 0.20),
     ph * fill(sdTriangle(q, vec2(-11.0, 9.0), vec2(-1.0, -2.0), vec2(7.0, 9.0)), aa));
   col = mix(col, vec3(0.98, 0.86, 0.34), ph * fill(length(q - vec2(5.5, -4.5)) - 2.6, aa));
  } else if (kind < 8.5){
   // a disc
   float g = fill(length(q) - 14.0, aa);
   col = mix(col, mix(vec3(0.72, 0.74, 0.80), vec3(0.93, 0.95, 0.99), 1.0 - shade), g);
   // the sheen band that CDs always had in these icon sets
   col = mix(col, mix(vec3(0.55, 0.80, 0.95), vec3(0.90, 0.70, 0.95), tint),
     g * fill(abs(q.x * 0.6 + q.y) - 5.0, aa) * 0.55);
   col = mix(col, vec3(0.62, 0.66, 0.72), fill(abs(length(q) - 4.6) - 0.9, aa) * g);
   col = mix(col, vec3(0.30, 0.34, 0.42), fill(length(q) - 3.4, aa));
  } else if (kind < 9.5){
   // LimeWire — the lime-green orb, cut like the fruit
   float g = fill(length(q) - 14.0, aa);
   col = mix(col, mix(vec3(0.36, 0.68, 0.06), vec3(0.72, 0.94, 0.20), 1.0 - shade), g);
   col = mix(col, vec3(0.90, 0.98, 0.60), fill(length(q) - 9.5, aa));
   for (int w = 0; w < 5; w++){                       // the segments
    float fw = float(w) * 1.2566;
    col = mix(col, vec3(0.44, 0.74, 0.12),
      fill(sdSeg2(q, vec2(0.0), vec2(cos(fw), sin(fw)) * 9.5) - 0.8, aa));
   }
   col += vec3(1.0) * fill(length(q - vec2(-5.0, -6.5)) - 3.6, aa) * 0.30;
  } else if (kind < 10.5){
   // hax.exe — a console with a prompt in it
   col = mix(col, vec3(0.10, 0.11, 0.13), fill(rbox(q, vec2(15.0, 12.0), 2.0), aa));
   col = mix(col, vec3(0.42, 0.46, 0.52), fill(abs(rbox(q, vec2(15.0, 12.0), 2.0)) - 0.9, aa));
   col = mix(col, vec3(0.30, 0.33, 0.38), fill(rbox(q - vec2(0.0, -9.0), vec2(15.0, 3.0), 2.0), aa));
   for (int w = 0; w < 3; w++){
    float fw = float(w);
    col = mix(col, vec3(0.30, 0.95, 0.35),
      fill(rbox(q - vec2(-6.0 + fw * 1.5, -1.0 + fw * 5.0),
         vec2(5.0 - fw * 1.2, 1.0), 0.4), aa));
   }
  } else {
   // Age of Empires — a shield with a keep on it
   float sh = rbox(q - vec2(0.0, -2.0), vec2(12.0, 9.0), 2.0);
   sh = min(sh, sdTriangle(q, vec2(-12.0, 6.0), vec2(12.0, 6.0), vec2(0.0, 15.0)));
   col = mix(col, mix(vec3(0.55, 0.13, 0.10), vec3(0.88, 0.34, 0.22), 1.0 - shade), fill(sh, aa));
   col = mix(col, vec3(0.98, 0.86, 0.42), fill(abs(sh) - 1.1, aa));
   // a little keep: three merlons over a block
   col = mix(col, vec3(0.98, 0.90, 0.60), fill(rbox(q - vec2(0.0, -1.0), vec2(6.5, 4.0), 0.6), aa));
   for (int w = 0; w < 3; w++){
    float fw = float(w) - 1.0;
    col = mix(col, vec3(0.98, 0.90, 0.60),
      fill(rbox(q - vec2(fw * 4.6, -7.0), vec2(1.6, 2.6), 0.4), aa));
   }
  }
  // Label — derived from the icon, so the caption always matches.
  // The grid pitch is 78px and a char costs 2h = 6.4px, so anything past
  // 12 chars spills into the next column. Every desktop label is <= 12
  // except AGE OF EMPIRES 2 (16 chars = 102px), which ran into MY
  // COMPUTER and off the left edge of the screen. Shrinking it to fit
  // turned it into an unreadable smudge, so it wraps over two lines the
  // way XP does, out of a pre-split pair of strings. NB any label added
  // past 12 chars needs the same treatment - the font can't wrap itself.
  float l2 = (lbl > 28.5 && lbl < 29.5) ? 40.0 : 0.0;   // EMPIRES 2
  if (l2 > 0.5) lbl = 39.0;                             // AGE OF
  float lw = textW(lbl, 3.2);
  drawText(col, q, vec2(-lw * 0.5 + 1.0, 28.0), lbl, 3.2, vec3(0.02), 0.55);
  drawText(col, q, vec2(-lw * 0.5,       27.0), lbl, 3.2, vec3(1.0),  0.98);
  if (l2 > 0.5){
   float w2 = textW(l2, 3.2);
   drawText(col, q, vec2(-w2 * 0.5 + 1.0, 38.0), l2, 3.2, vec3(0.02), 0.55);
   drawText(col, q, vec2(-w2 * 0.5,       37.0), l2, 3.2, vec3(1.0),  0.98);
  }
 }
}
// ============================================================
// START MENU — the two-pane Luna one
// ============================================================
void startMenu(inout vec3 col, vec2 p, float aa, float open){
 if (open < 0.001) return;
 float bot = SCREEN.y - 34.0;
 float h   = 452.0 * open;
 float top = bot - h;
 vec2  mn = vec2(0.0, top), mx = vec2(384.0, bot);
 vec2  ctr = (mn + mx) * 0.5, hs = (mx - mn) * 0.5;
 vec2  q = p - ctr;
 if (rbox(q, hs, 8.0) > 8.0) return;
 float shell = fill(rbox(q, hs, 8.0), aa);
 col = mix(col, vec3(0.04, 0.12, 0.34), fill(rbox(q - vec2(4.0, 5.0), hs, 8.0), 7.0) * 0.32);
 // ---- header ----
 float hdr = fill(rbox(q - vec2(0.0, -hs.y + 32.0), vec2(hs.x, 32.0), 8.0), aa);
 float g = clamp((p.y - top) / 64.0, 0.0, 1.0);
 vec3 hc = mix(vec3(0.26, 0.50, 0.94), vec3(0.13, 0.31, 0.80), g);
 // NB the max() is load-bearing: pow() is UNDEFINED for a negative base in
 // GLSL, and 1.0 - abs(g - c)*k goes negative away from the band centre.
 // Odd exponents came back negative (black bands), even ones came back as
 // |x|^n which is huge (white blowout). Both symptoms, one bug.
 hc = mix(hc, vec3(0.33, 0.58, 0.97), pow(max(1.0 - abs(g - 0.34) * 3.0, 0.0), 3.0) * 0.55);
 hc += vec3(1.0) * pow(max(1.0 - abs(g - 0.10) * 6.5, 0.0), 5.0) * 0.34;
 col = mix(col, hc, hdr * shell);
 // avatar + user name
 vec2 av = p - vec2(38.0, top + 32.0);
 col = mix(col, vec3(0.85, 0.90, 0.98), fill(rbox(av, vec2(21.0, 21.0), 6.0), aa));
 col = mix(col, vec3(0.42, 0.60, 0.86), fill(length(av - vec2(0.0, -6.0)) - 8.0, aa));
 col = mix(col, vec3(0.42, 0.60, 0.86), fill(rbox(av - vec2(0.0, 12.0), vec2(13.0, 9.0), 7.0), aa));
 drawText(col, p, vec2(70.0, top + 33.0), 24.0, 4.6, vec3(0.02, 0.10, 0.34), 0.5);
 drawText(col, p, vec2(70.0, top + 32.0), 24.0, 4.6, vec3(1.0), 0.98);
 // ---- panes ----
 float paneT = top + 64.0, paneB = bot - 52.0;
 float lp = fill(rbox(p - vec2(104.0, (paneT + paneB) * 0.5),
      vec2(104.0, (paneB - paneT) * 0.5), 1.0), aa);
 col = mix(col, vec3(1.0), lp * shell);
 float rp = fill(rbox(p - vec2(296.0, (paneT + paneB) * 0.5),
      vec2(88.0, (paneB - paneT) * 0.5), 1.0), aa);
 col = mix(col, vec3(0.63, 0.75, 0.90), fill(abs(p.x - 208.0) - 0.6, aa) * shell * 0.8);
 col = mix(col, vec3(0.827, 0.894, 0.976), rp * shell);          // XP's pale blue pane
 // left items: big icon + two lines
 for (int i = 0; i < 7; i++){
  float fi = float(i);
  float y = paneT + 26.0 + fi * 44.0;
  if (y > paneB - 12.0) continue;
  // the pinned programs, each with its own icon
  float lk = (fi < 0.5) ? 3.0 : (fi < 1.5) ? 6.0 : (fi < 2.5) ? 12.0
    : (fi < 3.5) ? 13.0 : (fi < 4.5) ? 5.0 : (fi < 5.5) ? 7.0 : 4.0;
  float ln = (fi < 0.5) ? 4.0 : (fi < 1.5) ? 35.0 : (fi < 2.5) ? 33.0
    : (fi < 3.5) ? 34.0 : (fi < 4.5) ? 6.0 : (fi < 5.5) ? 18.0 : 17.0;
  miniIcon(col, p - vec2(30.0, y), lk, 12.0, aa);
  drawText(col, p, vec2(52.0, y), ln, 3.5, vec3(0.05), 0.97 * shell);
 }
 // separator + "All Programs"
 col = mix(col, vec3(0.66, 0.72, 0.84), fill(rbox(p - vec2(104.0, paneB - 26.0), vec2(96.0, 0.7), 0.3), aa) * shell);
 drawText(col, p, vec2(30.0, paneB - 12.0), 14.0, 3.6, vec3(0.05, 0.15, 0.55), 0.97 * shell);
 col = mix(col, vec3(0.10, 0.30, 0.75),
   fill(sdTriangle(p - vec2(150.0, paneB - 12.0), vec2(0.0, -4.5), vec2(0.0, 4.5), vec2(5.5, 0.0)), aa) * shell);
 // right items
 for (int i = 0; i < 8; i++){
  float fi = float(i);
  float y = paneT + 22.0 + fi * 38.0;
  if (y > paneB - 10.0) continue;
  float rk = (fi < 0.5) ? 2.0 : (fi < 1.5) ? 7.0 : (fi < 2.5) ? 5.0 : (fi < 3.5) ? 0.0
    : (fi < 4.5) ? 2.0 : (fi < 5.5) ? 8.0 : (fi < 6.5) ? 3.0 : 4.0;
  float rn = (fi < 0.5) ? 3.0 : (fi < 1.5) ? 8.0 : (fi < 2.5) ? 20.0 : (fi < 3.5) ? 1.0
    : (fi < 4.5) ? 36.0 : (fi < 5.5) ? 10.0 : (fi < 6.5) ? 38.0 : 12.0;
  miniIcon(col, p - vec2(226.0, y), rk, 8.5, aa);
  drawText(col, p, vec2(242.0, y), rn, 3.2, vec3(0.05, 0.10, 0.30), 0.97 * shell);
  if (mod(fi, 3.0) > 1.5)
   col = mix(col, vec3(0.62, 0.72, 0.88), fill(rbox(p - vec2(296.0, y + 19.0), vec2(82.0, 0.7), 0.3), aa) * shell);
 }
 // ---- footer: log off / turn off ----
 float fty = bot - 52.0;
 float ft = fill(rbox(q - vec2(0.0, hs.y - 26.0), vec2(hs.x, 26.0), 8.0), aa);
 float fg = clamp((p.y - fty) / 52.0, 0.0, 1.0);
 vec3 fc = mix(vec3(0.28, 0.52, 0.95), vec3(0.15, 0.33, 0.82), fg);
 fc += vec3(1.0) * pow(max(1.0 - abs(fg - 0.16) * 4.4, 0.0), 5.0) * 0.26;          // the gloss band
 col = mix(col, fc, ft * shell);
 // a lit rule where the footer meets the panes
 col = mix(col, vec3(0.62, 0.80, 1.0), fill(abs(p.y - fty) - 0.9, aa) * shell * 0.85);
 col = mix(col, vec3(0.03, 0.12, 0.44), fill(abs(p.y - fty - 2.0) - 0.7, aa) * shell * 0.5);
 // Log Off — a key-ish roundel with an arrow leaving it
 vec2 lo = p - vec2(196.0, bot - 26.0);
 col = mix(col, vec3(0.99, 0.80, 0.24), fill(length(lo) - 9.5, aa) * shell);
 col = mix(col, vec3(0.55, 0.36, 0.04), fill(abs(length(lo) - 9.5) - 1.0, aa) * shell);
 col = mix(col, vec3(0.35, 0.22, 0.02), fill(rbox(lo - vec2(1.0, 0.0), vec2(5.0, 1.4), 0.6), aa) * shell);
 col = mix(col, vec3(0.35, 0.22, 0.02),
   fill(sdTriangle(lo - vec2(6.5, 0.0), vec2(-1.0, -3.6), vec2(-1.0, 3.6), vec2(3.4, 0.0)), aa) * shell);
 drawText(col, p, vec2(210.0, bot - 26.0), 15.0, 3.4, vec3(1.0), 0.97 * shell);
 // Turn Off — the power mark: a broken ring with a bar through the gap
 vec2 po = p - vec2(288.0, bot - 26.0);
 col = mix(col, vec3(0.90, 0.32, 0.20), fill(length(po) - 9.5, aa) * shell);
 col = mix(col, vec3(0.52, 0.10, 0.04), fill(abs(length(po) - 9.5) - 1.0, aa) * shell);
 float ring = abs(length(po) - 4.8) - 1.3;
 ring = max(ring, -(abs(po.x) - 2.2 - max(-po.y, 0.0) * 9.0));            // notch out the top
 col = mix(col, vec3(1.0), fill(ring, aa) * shell);
 col = mix(col, vec3(1.0), fill(rbox(po - vec2(0.0, -4.0), vec2(1.2, 4.4), 0.5), aa) * shell);
 drawText(col, p, vec2(302.0, bot - 26.0), 16.0, 3.4, vec3(1.0), 0.97 * shell);
}
// ============================================================
// ONE DIALOG
// returns rgb in .xyz and coverage in .w
// q is the pixel relative to the window's top-left, y DOWN
// ============================================================
vec4 dialog(vec2 q, float aa, float glass, float seed){
 vec4 outp = vec4(0.0);
 // drop shadow, offset down-right
 float dsh = rbox(q - WSZ * 0.5 - vec2(5.0, 6.0), WSZ * 0.5, 4.0);
 outp = vec4(0.03, 0.09, 0.26, fill(dsh + 4.0, 6.0) * 0.28);
 // outer frame — XP Luna blue
 float fr = rbox(q - WSZ * 0.5, WSZ * 0.5, 5.0);
 float frA = fill(fr, aa);
 vec3  frC = mix(vec3(0.12, 0.34, 0.84), vec3(0.26, 0.52, 0.92), q.y / WSZ.y);
 outp.rgb = mix(outp.rgb, frC, frA);
 outp.a   = max(outp.a, frA);
 // ---- title bar ----
 float tb = fill(rbox(q - vec2(WSZ.x * 0.5, 15.5), vec2(WSZ.x * 0.5 - 3.0, 12.5), 3.5), aa);
 if (tb > 0.001){
  float g = clamp((q.y - 3.0) / 25.0, 0.0, 1.0);
  // the Luna gradient: deep blue, a bright band, then blue again
  vec3 c = mix(vec3(0.16, 0.42, 0.90), vec3(0.36, 0.64, 0.98), smoothstep(0.0, 0.45, g));
  c = mix(c, vec3(0.18, 0.42, 0.89), smoothstep(0.55, 1.0, g));
  c += vec3(1.0) * pow(max(1.0 - abs(g - 0.20) * 4.0, 0.0), 6.0) * 0.38;   // gloss band
  outp.rgb = mix(outp.rgb, c, tb);
  outp.a = max(outp.a, tb);
  // title text — blocks, or an unknown alphabet if this copy came back wrong
  float titleAlien = step(1.0 - uAlien * 0.6, hash11(seed * 13.7 + 2.0));
  if (titleAlien > 0.5){
   float txr = q.x - 30.0;
   if (txr > -7.0 && txr < 260.0){
    float cell = floor(txr / 14.0);
    vec2  gq = vec2(mod(txr, 14.0) - 7.0, (q.y - 15.0) * 1.35);
    outp.rgb = mix(outp.rgb, vec3(1.0),
       alienGlyph(gq, hash11(cell * 5.3 + seed * 2.1), aa) * 0.92);
   }
  } else {
   // the dialog's own title — hung copies say NOT RESPONDING
   float tsid = mix(22.0, 23.0, step(1.0 - uQuestion * 0.42, hash11(seed * 3.77 + 0.5)));
   outp.rgb = mix(outp.rgb, vec3(0.02, 0.10, 0.32),
      textCov(q - vec2(31.0, 16.0), tsid, 4.2) * 0.45);   // shadow
   outp.rgb = mix(outp.rgb, vec3(1.0),
      textCov(q - vec2(30.0, 15.0), tsid, 4.2) * 0.96);
  }
  // close button
  vec2 cb = q - vec2(WSZ.x - 22.0, 15.0);
  float cbd = fill(rbox(cb, vec2(11.0, 9.5), 3.0), aa);
  vec3 cbc = mix(vec3(0.90, 0.28, 0.16), vec3(0.99, 0.62, 0.50), 1.0 - clamp(cb.y / 19.0 + 0.5, 0.0, 1.0));
  outp.rgb = mix(outp.rgb, cbc, cbd);
  float xArm = min(fill(rbox(vec2(cb.x - cb.y, cb.x + cb.y) * 0.7071, vec2(6.0, 1.2), 0.4), aa)
     + fill(rbox(vec2(cb.x + cb.y, cb.x - cb.y) * 0.7071, vec2(6.0, 1.2), 0.4), aa), 1.0);
  outp.rgb = mix(outp.rgb, vec3(1.0), cbd * xArm);
 }
 // ---- body ----
 float bd = fill(rbox(q - vec2(WSZ.x * 0.5, (WSZ.y + 30.0) * 0.5), vec2(WSZ.x * 0.5 - 4.0, (WSZ.y - 34.0) * 0.5), 1.0), aa);
 if (bd > 0.001){
  vec3 body = vec3(0.925, 0.914, 0.847);                 // XP dialog beige
  body *= 1.0 - 0.05 * smoothstep(0.0, 1.0, q.y / WSZ.y); // faint vertical shade
  outp.rgb = mix(outp.rgb, body, bd);
  outp.a = max(outp.a, bd);
  // The roundel. Some copies came back from the repaint as a query
  // dialog instead of an error — blue, with a question mark.
  vec2 ip = q - vec2(46.0, 78.0);
  float icd = length(ip) - 17.0;
  float ica = fill(icd, aa);
  // four flavours: error X, query ?, a smiley, and one that didn't make it
  float rr = hash11(seed * 3.77 + 0.5);
  float face = step(1.0 - uQuestion * 0.42, rr);            // smiley or dead
  float dead = face * step(0.5, hash11(seed * 8.13 + 2.0));
  float isQ  = (1.0 - face) * step(1.0 - uQuestion * 0.60, hash11(seed * 5.31 + 9.0));
  vec3 red  = mix(vec3(0.86, 0.10, 0.08), vec3(1.0, 0.42, 0.34), smoothstep(12.0, -18.0, ip.y));
  vec3 blue = mix(vec3(0.06, 0.32, 0.78), vec3(0.42, 0.70, 1.0), smoothstep(12.0, -18.0, ip.y));
  vec3 yell = mix(vec3(0.94, 0.68, 0.04), vec3(1.0, 0.93, 0.40), smoothstep(12.0, -18.0, ip.y));
  vec3 icc = mix(mix(red, blue, isQ), yell, face);
  outp.rgb = mix(outp.rgb, icc, ica);
  outp.rgb += vec3(1.0) * fill(length(ip - vec2(-5.0, -7.0)) - 5.0, aa) * 0.30;   // gloss
  float mark;
  if (face > 0.5){
   vec2 ep = vec2(abs(ip.x) - 5.6, ip.y + 4.0);
   if (dead > 0.5){
    // X for eyes, and a straight line for a mouth
    mark  = fill(rbox(vec2(ep.x - ep.y, ep.x + ep.y) * 0.7071, vec2(3.4, 1.0), 0.4), aa);
    mark += fill(rbox(vec2(ep.x + ep.y, ep.x - ep.y) * 0.7071, vec2(3.4, 1.0), 0.4), aa);
    mark += fill(rbox(ip - vec2(0.0, 7.0), vec2(5.4, 1.1), 0.5), aa);
   } else {
    mark  = fill(length(ep) - 1.9, aa);                       // eyes
    // the smile: a ring cut to its lower arc
    float sm = abs(length(ip - vec2(0.0, -0.5)) - 8.4) - 1.15;
    mark += fill(max(sm, 3.0 - ip.y), aa);
   }
   mark = min(mark, 1.0);
   outp.rgb = mix(outp.rgb, vec3(0.20, 0.12, 0.0), ica * mark);   // dark features on yellow
  } else {
   if (isQ > 0.5){
    mark = fill(sdQuestion(ip - vec2(0.0, 1.0), 1.35), aa);
   } else {
    mark = min(fill(rbox(vec2(ip.x - ip.y, ip.x + ip.y) * 0.7071, vec2(9.0, 2.0), 0.6), aa)
      + fill(rbox(vec2(ip.x + ip.y, ip.x - ip.y) * 0.7071, vec2(9.0, 2.0), 0.6), aa), 1.0);
   }
   outp.rgb = mix(outp.rgb, vec3(1.0), ica * mark);
  }
  // ---- the traceback ----
  // Clean grey rules, unless the repaint left something else behind:
  // torn rows slide sideways, some invert into a selection block, and
  // some come back as characters from no alphabet at all.
  for (int i = 0; i < 8; i++){
   float fi = float(i);
   float y0 = 50.0 + fi * 11.5;
   if (abs(q.y - y0) > 7.0) continue;            // only one row is live per pixel
   float rs = hash11(fi * 4.3 + seed * 9.1);
   float w  = 150.0 + 180.0 * hash11(fi * 4.3 + 1.0);
   float x0 = 80.0;
   // torn: the row never finished moving
   float tear = step(1.0 - uGarble * 0.55, hash11(fi * 7.7 + seed * 5.3));
   x0 += tear * (hash11(fi * 2.2 + seed * 3.3) - 0.5) * 150.0 * uGarble;
   // inverted: a stale selection highlight
   float inv = step(1.0 - uGarble * 0.30, hash11(fi * 11.9 + seed * 2.7));
   if (inv > 0.5){
    float blk = fill(rbox(q - vec2(x0 + w * 0.5, y0), vec2(w * 0.5 + 3.0, 5.5), 1.0), aa);
    outp.rgb = mix(outp.rgb, vec3(0.04, 0.20, 0.62), blk);
   }
   vec3 ink = mix(vec3(0.16, 0.16, 0.18), vec3(1.0), inv);
   if (step(1.0 - uAlien, rs) > 0.5){
    // alien script — one glyph per 13px cell, indexed off x so the
    // whole row costs O(1) instead of a nested loop
    float xr = q.x - x0;
    if (xr > -6.0 && xr < w){
     float cell = floor(xr / 13.0);
     vec2  gq = vec2(mod(xr, 13.0) - 6.5, q.y - y0);
     float gl = alienGlyph(gq, hash11(cell * 3.1 + fi * 7.7 + seed * 1.9), aa);
     outp.rgb = mix(outp.rgb, ink, gl * 0.9);
    }
   } else {
    float ln = fill(rbox(q - vec2(x0 + w * 0.5, y0), vec2(w * 0.5, 2.2), 0.8), aa);
    outp.rgb = mix(outp.rgb, ink, ln * 0.85);
    // a stray question mark dropped into the line
    if (step(1.0 - uQuestion * 0.5, hash11(fi * 19.3 + seed * 7.1)) > 0.5){
     float qm = fill(sdQuestion(q - vec2(x0 + w + 12.0, y0), 0.72), aa);
     outp.rgb = mix(outp.rgb, ink, qm);
    }
   }
  }
  // END NOW / CANCEL, the way the hung-app dialog actually asks
  for (int b = 0; b < 2; b++){
   float fb = float(b);
   vec2 bp = q - vec2(WSZ.x * 0.5 - 52.0 + fb * 104.0, WSZ.y - 24.0);
   float bdd = rbox(bp, vec2(46.0, 12.5), 3.0);
   float bA  = fill(bdd, aa);
   float bg  = clamp(bp.y / 25.0 + 0.5, 0.0, 1.0);
   vec3  bc  = mix(vec3(1.0, 1.0, 0.99), vec3(0.84, 0.84, 0.80), bg);
   bc = mix(bc, vec3(0.93, 0.93, 0.90), smoothstep(0.42, 0.55, bg));
   outp.rgb = mix(outp.rgb, bc, bA);
   outp.rgb = mix(outp.rgb, vec3(0.38, 0.42, 0.40), fill(abs(bdd) - 0.9, aa));
   outp.rgb = mix(outp.rgb, vec3(1.0), fill(abs(bdd + 1.6) - 0.7, aa) * bA * 0.7);
   float bs = (fb < 0.5) ? 31.0 : 32.0;                       // END NOW / CANCEL
   float bw = textW(bs, 3.4);
   outp.rgb = mix(outp.rgb, vec3(0.08),
      textCov(bp - vec2(-bw * 0.5, 0.0), bs, 3.4) * 0.95);
  }
 }
 // ---- Aero: a specular sweep riding across the glass ----
 if (glass > 0.001 && outp.a > 0.001){
  float sweep = pow(clamp(1.0 - abs((q.x + q.y * 0.55) / WSZ.x - 0.42) * 3.2, 0.0, 1.0), 3.0);
  outp.rgb += vec3(0.65, 0.85, 1.0) * sweep * 0.16 * glass * outp.a;
  // (the old top-edge catch-light lived here — it drew a hard white bar
  //  straight across the window under the title, so it's gone)
 }
 return outp;
}
// where cascade k's window sits at time tt. Shared by the cascade loop and
// by the cursor, so the pointer always rides the live window's titlebar.
vec2 dragPath(float fk, float tt){
 float sk  = hash11(fk * 17.7 + 3.0);
 vec2  amp = vec2(300.0, 150.0) * (0.55 + 0.55 * sk);
 vec2  spd = vec2(0.61, 0.47) * (0.7 + 0.7 * hash11(fk * 5.3));
 vec2  hom = vec2(gSW, SCREEN.y) * 0.5;
 if (fk > 0.5) hom += vec2(180.0 * (sk - 0.5), 120.0 * (hash11(fk * 8.9) - 0.5));
 return hom + vec2(amp.x * sin(tt * spd.x) + 90.0 * sin(tt * 1.43),
     amp.y * cos(tt * spd.y) + 55.0 * cos(tt * 1.17)) * uDrift;
}
// ============================================================
void main(){
 vec2 fragCoord = gl_FragCoord.xy;
 vec2  R = RENDERSIZE;
 float scale = R.y / SCREEN.y;
 gSW = SCREEN.y * (R.x / R.y);        // desktop is as wide as the window is
 // desktop pixels, y DOWN, origin at the top-left of the actual viewport
 vec2  p = vec2(fragCoord.x / scale, SCREEN.y - fragCoord.y / scale);
 float aa = 1.0 / max(scale, 0.001) * 0.75;   // one desktop pixel, in screen terms
 float t  = TIME * uSpeed;
 vec3 col = desktop(p, t, aa);
 if (uIcons > 0.001) desktopIcons(col, p, aa);
 // ---- the cascades ------------------------------------------------
 // Each copy is the SAME window sampled further back in its own past,
 // so the trail is literally the path it has been dragged along.
 // Oldest first, so the live one lands on top. uCascades runs several
 // independent drags at once, each on its own path and phase.
 float N  = clamp(uCount, 1.0, 48.0);
 float NC = clamp(floor(uCascades), 1.0, 4.0);
 for (int k = 0; k < 4; k++){
  float fk = float(k);
  if (fk >= NC) continue;
  float tk = t + fk * 7.3;                        // each drag out of step
  for (int i = 47; i >= 0; i--){
   float fi = float(i);
   if (fi >= N) continue;   // float test: keeps the loop header ES-1.00 plain
   vec2 c = dragPath(fk, tk - fi * uTrail);
   // per-copy glitch nudge — stale copies sit a pixel or two out
   c += uJitter * vec2(hash11(fi * 12.7 + fk * 3.1 + floor(t * 3.0)) - 0.5,
        hash11(fi * 31.3 + fk * 7.7 + floor(t * 3.0)) - 0.5) * 9.0;
   vec2 q = p - (c - WSZ * 0.5);
   if (q.x < -14.0 || q.y < -14.0 || q.x > WSZ.x + 14.0 || q.y > WSZ.y + 14.0) continue;
   // torn copies: some stale ones never finished repainting, so one
   // scanline is stretched across the whole body
   float torn = step(0.80, hash11(fi * 5.9 + fk * 2.3 + floor(t * 1.7))) * uSmear;
   if (torn > 0.001 && fi > 0.5){
    float yTear = 30.0 + hash11(fi * 8.1 + fk) * (WSZ.y - 40.0);
    float band  = step(abs(q.y - yTear) - 26.0, 0.0);
    q.y = mix(q.y, yTear, band * torn);
   }
   vec4 w = dialog(q, aa, uGlass, fi * 1.37 + fk * 23.9);
   // the oldest copies have been sitting there longest
   w.a *= 1.0 - uFade * (fi / max(N - 1.0, 1.0));
   col = mix(col, w.rgb, w.a);
  }
 }
 // ---- taskbar -----------------------------------------------------
 if (uTaskbar > 0.5){
  float tby = SCREEN.y - 34.0;
  float inBar = step(tby, p.y);
  if (inBar > 0.5){
   float g = (p.y - tby) / 34.0;
   // XP's taskbar stays blue all the way down — the old ramp fell
   // to near-black, which is what made it look muddy
   vec3 bar = mix(vec3(0.27, 0.49, 0.93), vec3(0.14, 0.32, 0.80), g);
   bar = mix(bar, vec3(0.34, 0.57, 0.96), pow(max(1.0 - abs(g - 0.30) * 3.4, 0.0), 3.0) * 0.55);
   bar += vec3(1.0) * pow(max(1.0 - abs(g - 0.10) * 7.0, 0.0), 5.0) * 0.38;      // top gloss
   bar = mix(bar, vec3(0.20, 0.40, 0.86), smoothstep(0.86, 1.0, g) * 0.5);
   bar = mix(bar, vec3(0.98, 0.72, 0.20), step(p.y, tby + 2.2) * 0.85);  // the amber rule
   // ---- Start button ----
   // XP's is a lozenge rounded hard on the right and squared off at
   // the left where it meets the screen edge, with the flag inset.
   vec2 sp = p - vec2(48.0, tby + 17.0);
   float sd0 = rbox(sp, vec2(52.0, 15.0), 14.0);
   sd0 = max(sd0, -(sp.x + 52.0) - 4.0);           // square it off at the screen edge
   float sb = fill(sd0, aa);
   vec3 sc = mix(vec3(0.56, 0.85, 0.28), vec3(0.20, 0.52, 0.09), g);
   sc += vec3(1.0) * pow(max(1.0 - abs(g - 0.17) * 4.2, 0.0), 4.0) * 0.42;      // top gloss
   sc = mix(sc, vec3(0.09, 0.30, 0.04), smoothstep(0.72, 1.0, g) * 0.55);
   bar = mix(bar, sc, sb);
   bar = mix(bar, vec3(0.72, 0.94, 0.50), fill(abs(sd0) - 0.9, aa) * 0.45);   // lit rim
   // the four-pane flag, sheared into its wave
   for (int fq = 0; fq < 4; fq++){
    float ff = float(fq);
    vec2 cell = vec2(mod(ff, 2.0), floor(ff / 2.0)) - 0.5;
    vec2 fp = sp - vec2(-32.0, 0.0) - cell * vec2(8.6, 8.2);
    fp.y += fp.x * 0.22;                          // the flag's lean
    vec3 pane = (ff < 0.5)  ? vec3(0.92, 0.30, 0.22)
      : (ff < 1.5)  ? vec3(0.45, 0.76, 0.98)
      : (ff < 2.5)  ? vec3(0.99, 0.80, 0.22)
          : vec3(0.52, 0.82, 0.35);
    float fa = fill(rbox(fp, vec2(3.6, 3.4), 1.1), aa) * sb;
    bar = mix(bar, pane, fa);
    bar = mix(bar, min(pane + 0.45, vec3(1.0)),
      fill(rbox(fp - vec2(0.0, -2.0), vec2(3.0, 1.2), 0.8), aa) * fa * 0.7);
   }
   drawText(bar, p, vec2(37.0, tby + 18.0), 0.0, 4.8, vec3(0.06, 0.22, 0.05), 0.55 * sb);
   drawText(bar, p, vec2(36.0, tby + 17.0), 0.0, 4.8, vec3(1.0), 0.98 * sb);
   // task buttons — one per cascade, plus a couple of idle ones.
   // Each carries a small app icon and a run of label text.
   for (int i = 0; i < 4; i++){
    float fi = float(i);
    // start at 204 — the Start button reaches x=116, and the old
    // 168 put the first button's left edge under it
    float bx = 184.0 + fi * 158.0;
    float bdd = rbox(p - vec2(bx, tby + 17.0), vec2(76.0, 12.0), 3.0);
    float tbn = fill(bdd, aa);
    float hung = step(fi, clamp(floor(uCascades), 1.0, 4.0) - 0.5);
    // the hung ones sit pressed in, darker and with the gloss inverted
    // both states stay blue — the pressed one used to bottom out
    // near black, which read as a hole in the bar
    vec3 tc = mix(mix(vec3(0.31, 0.55, 0.95), vec3(0.17, 0.37, 0.85), g),
       mix(vec3(0.13, 0.29, 0.72), vec3(0.20, 0.41, 0.86), 1.0 - g), hung);
    bar = mix(bar, tc, tbn);
    bar = mix(bar, vec3(0.55, 0.74, 1.0), fill(abs(bdd) - 0.8, aa) * 0.35);   // rim
    // icon and label always agree, and match the desktop shortcuts
    float tkind = (hung > 0.5) ? 0.0 : (fi < 1.5) ? 10.0 : (fi < 2.5) ? 11.0 : 9.0;
    miniIcon(bar, p - vec2(bx - 62.0, tby + 17.0), tkind, 7.0, aa);
    // hung copies say NOT RESPONDING; the rest name their program
    float tlab = (fi < 1.5) ? 28.0 : (fi < 2.5) ? 29.0 : 30.0;
    tlab = mix(tlab, 23.0, hung);
    drawText(bar, p, vec2(bx - 50.0, tby + 17.0), tlab, 3.4,
      mix(vec3(1.0), vec3(1.0, 0.86, 0.78), hung), 0.97 * tbn);
   }
   // ---- system tray ----
   float trX = gSW - 132.0;
   vec2  trC = vec2(trX + 62.0, tby + 17.0);
   float trd = rbox(p - trC, vec2(64.0, 13.0), 2.0);
   float tr  = fill(trd, aa);
   bar = mix(bar, mix(vec3(0.16, 0.42, 0.90), vec3(0.09, 0.28, 0.78), g), tr);
   bar = mix(bar, vec3(0.15, 0.33, 0.76), fill(abs(trd) - 0.8, aa) * 0.8);   // sunken edge
   bar = mix(bar, vec3(0.42, 0.64, 0.99), fill(abs(trd + 1.6) - 0.6, aa) * 0.45);
   miniIcon(bar, p - vec2(trX + 16.0, tby + 17.0), 13.0, 6.0, aa);   // messenger
   miniIcon(bar, p - vec2(trX + 34.0, tby + 17.0), 3.0,  6.0, aa);   // network
   miniIcon(bar, p - vec2(trX + 52.0, tby + 17.0), 5.0,  6.0, aa);   // volume
   // the clock, right-aligned in its well
   drawText(bar, p, vec2(trX + 74.0, tby + 18.0), 21.0, 4.0, vec3(0.02, 0.10, 0.34), 0.45);
   drawText(bar, p, vec2(trX + 74.0, tby + 17.0), 21.0, 4.0, vec3(1.0), 0.98);
   col = mix(col, bar, inBar);
  }
 }
 // ---- start menu, over the taskbar --------------------------------
 startMenu(col, p, aa, uStart ? 1.0 : 0.0);
 // ---- the pointer, still holding the live window -------------------
 if (uCursor > 0.001){
  vec2 grab = dragPath(0.0, t) - WSZ * 0.5 + vec2(WSZ.x * 0.42, 15.0);
  vec2 cq = p - grab;
  if (dot(cq, cq) < 900.0){
   // Head and tail, both convex, unioned. Proportions off the real
   // pointer: a narrow blade, and the tail kicked out to the right.
   // A longer, narrower blade than before, with a 1px keyline. The
   // outline used to be 1.35 desktop px on a 16px arrow, which ate
   // most of the white and left a black blob.
   float head = sdTriangle(cq, vec2(0.0, 0.0), vec2(0.5, 18.4), vec2(12.6, 13.1));
   // The tail kicks DOWN-RIGHT off the blade's bottom edge. The old
   // matrix was the transpose of what it needed to be, so the leg
   // leaned left and poked out past the bottom-left corner - the arrow
   // read as a broken Y. Local +y must map to (0.43, 0.90) in desktop
   // space (y is down here), i.e. mat2(0.90, 0.43, -0.43, 0.90).
   // Half-width has to beat the keyline: the white body is eroded by
   // 0.95 desktop px per side, so a 1.7 half-width left a 0.75 white
   // core and the leg read as a thin spur instead of the real one.
   vec2 tq = cq - vec2(6.9, 16.6);
   tq = vec2(tq.x * 0.92 - tq.y * 0.40, tq.x * 0.40 + tq.y * 0.92);   // ~24 deg
   float tail = rbox(tq, vec2(1.95, 5.4), 0.3);
   float d = min(head, tail);
   col = mix(col, vec3(0.0), fill(d - 1.6, aa) * 0.20);   // soft drop shadow
   col = mix(col, vec3(0.02), fill(d, aa));               // keyline
   col = mix(col, vec3(1.0),  fill(d + 0.95, aa));        // white body
  }
 }
 // ---- grade: the era's taste, dialled in --------------------------
 if (uBloom > 0.001){
  float lum = dot(col, vec3(0.299, 0.587, 0.114));
  col += col * smoothstep(0.72, 1.0, lum) * uBloom * 0.55;
 }
 float l = dot(col, vec3(0.299, 0.587, 0.114));
 col = mix(vec3(l), col, uSat);                        // vivid
 col = pow(clamp(col, 0.0, 1.0), vec3(0.94, 0.97, 1.03));
 col *= 1.0 - uScan * 0.35 * (0.5 + 0.5 * sin(fragCoord.y * PI));
 vec2 vq = fragCoord / R;
 float vig = pow(16.0 * vq.x * vq.y * (1.0 - vq.x) * (1.0 - vq.y), 0.22);
 col *= mix(1.0, vig, uVignette);
 gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}

