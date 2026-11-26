/*{
  "DESCRIPTION": "Laser dot lattice: a Menger KIFS rendered with NO surface shading at all — the ray flies through the invisible structure and only deposits light at the fold-axis hot points, leaving a glowing dotted constellation tracing the fractal in black space. Real-time, seamless 16s loop.",
  "CREDIT": "glslop agent (Claude)",
  "CATEGORIES": ["fractal", "raymarch", "neon", "kifs"],
  "INPUTS": [
    { "NAME": "speed",     "TYPE": "float", "DEFAULT": 1.0,  "MIN": 0.0, "MAX": 3.0 },
    { "NAME": "glow",      "TYPE": "float", "DEFAULT": 1.0,  "MIN": 0.3, "MAX": 3.0 },
    { "NAME": "hue",       "TYPE": "float", "DEFAULT": 0.0,  "MIN": 0.0, "MAX": 1.0 },
    { "NAME": "spin",      "TYPE": "float", "DEFAULT": 0.5,  "MIN": 0.0, "MAX": 1.5 },
    { "NAME": "zoom",      "TYPE": "float", "DEFAULT": 1.0,  "MIN": 0.5, "MAX": 2.0 },
    { "NAME": "lens",      "TYPE": "float", "DEFAULT": 0.95, "MIN": 0.3, "MAX": 1.5 },
    { "NAME": "iterCount", "TYPE": "long",  "DEFAULT": 3,    "MIN": 2,   "MAX": 9 }
  ]
}*/

#define PI  3.14159265359
#define TAU 6.28318530718

const float PERIOD = 16.0;
const int   MAXI   = 9;
const int   STEPS  = 150;

mat2 rot(float a){ float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }

vec3 pal(float t){
  return 0.5 + 0.5 * cos(TAU * (t + vec3(0.55, 0.40, 0.18)));
}

// Clean Menger KIFS. d = surface distance (for stepping only — never used to stop);
// trap -> color ; edge -> 0..1, peaks where the surface point lies on a bar EDGE
// (two of the box faces meet), so the dots string along the fractal's edges.
float map(vec3 p, mat2 Rroll, out float trap, out float edge){
  vec3 z = p;
  z.xy = Rroll * z.xy;
  float scale = 3.0;
  trap = 1.0e9;
  for (int i = 0; i < MAXI; i++){
    if (i >= iterCount) break;
    z = abs(z);
    if (z.x < z.y) z.xy = z.yx;
    if (z.x < z.z) z.xz = z.zx;
    if (z.y < z.z) z.yz = z.zy;
    z = z * scale - vec3(2.0);
    if (z.z < -1.0) z.z += 2.0;
    trap = min(trap, dot(z, z));
  }
  // edge proximity from the final cell: on a face the largest |coord| ~1; on an
  // edge the SECOND largest is ~1 too. mid = middle component of abs(z).
  vec3 q = abs(z);
  float c1 = max(q.x, max(q.y, q.z));
  float c3 = min(q.x, min(q.y, q.z));
  float mid = q.x + q.y + q.z - c1 - c3;
  edge = smoothstep(0.86, 0.99, mid);
  return length(max(q - vec3(1.0), 0.0)) * pow(scale, -float(iterCount));
}

void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5 * RENDERSIZE.xy) / RENDERSIZE.y;

  float ph = fract(TIME * speed / PERIOD);
  float th = TAU * ph;
  mat2 Rroll = rot(0.4 * spin * sin(th));

  // wandering fly-THROUGH path: a closed loop that weaves through the lattice
  // (all integer multiples of th -> seamless). See-through, so being "inside" is fine.
  float R = 1.05 * zoom;
  float a  = th;
  float a2 = th + 0.32;                       // a point ahead, for look direction
  vec3 ro   = vec3(R * (sin(a)  + 0.35 * sin(3.0 * a)),  0.65 * sin(2.0 * a),
                   R * (cos(a)  + 0.35 * cos(2.0 * a)));
  vec3 look = vec3(R * (sin(a2) + 0.35 * sin(3.0 * a2)), 0.65 * sin(2.0 * a2),
                   R * (cos(a2) + 0.35 * cos(2.0 * a2)));
  // blend travel direction with a pull toward the lattice core so it stays framed
  vec3 fwd = normalize(mix(normalize(look - ro), normalize(-ro), 0.5));
  vec3 rgt = normalize(cross(vec3(0.0, 1.0, 0.0), fwd));
  vec3 upv = cross(fwd, rgt);

  // fisheye / 360 lens: ray angle from forward grows with screen radius
  float rr = length(uv);
  float theta = rr * lens * PI;              // lens~0.7 -> ~180deg FOV; higher -> 360-ish
  vec2 dir2 = rr > 1.0e-5 ? uv / rr : vec2(0.0, 1.0);
  vec3 rd = normalize(cos(theta) * fwd + sin(theta) * (dir2.x * rgt + dir2.y * upv));

  // ---- fly-THROUGH march: deposit a dot only at the instant the ray CROSSES
  // the (invisible) shell, and only where that crossing sits on a bar edge ----
  float t = 0.02;
  float tcap = 7.0;
  float trans = 1.0;
  vec3 col = vec3(0.0);
  float trap, edge;
  float prevD = 1.0e9;
  for (int i = 0; i < STEPS; i++){
    vec3 p = ro + rd * t;
    float d = map(p, Rroll, trap, edge);
    if (d < 0.0035 && prevD >= 0.0035){            // ray just entered the shell
      // light the crossing only if it lands on a bar EDGE -> dots trace the structure
      float spark = edge * exp(-0.16 * t);         // distance fade -> recedes into black
      // electric color from orbit trap + depth; small white-hot center only
      vec3 c = mix(pal(hue + 0.85 * sqrt(trap) + 0.10 * t), vec3(1.0), 0.12);
      col += c * spark * trans * glow * 5.0;
      trans *= exp(-spark * 5.0);                  // front edges dominate -> depth
    }
    prevD = d;
    t += max(d, 0.012);                            // pass straight through the shell
    if (t > tcap || trans < 0.03) break;
  }

  // gentle depth fade so the far dots sink into black
  col *= 0.95;
  col = col / (1.0 + col);
  col = pow(col, vec3(0.78));
  col *= 1.0 - 0.26 * dot(uv, uv);

  gl_FragColor = vec4(col, 1.0);
}

