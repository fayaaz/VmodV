/*{
  "ISFVSN": "2.0",
  "CREDIT": "Port of \"Fear Inoculum\" by RagePly (https://www.shadertoy.com/view/cdSGRw)",
  "DESCRIPTION": "A raymarched helix — an homage to Tool's Fear Inoculum album cover. CC BY-NC-SA 3.0, see source header and THIRD_PARTY_NOTICES.md.",
  "MODV_CATEGORY": "3D Scenes",
  "CATEGORIES": ["GENERATOR"],
  "INPUTS": [
    { "NAME": "speed", "LABEL": "Speed", "TYPE": "float", "MIN": 0.0, "MAX": 2.0, "DEFAULT": 1.0 },
    { "NAME": "coil", "LABEL": "Helix Coils", "TYPE": "float", "MIN": 5.0, "MAX": 150.0, "DEFAULT": 60.0 },
    { "NAME": "radius", "LABEL": "Helix Radius", "TYPE": "float", "MIN": 5.0, "MAX": 40.0, "DEFAULT": 20.0 },
    { "NAME": "cameraTilt", "LABEL": "Camera Tilt", "TYPE": "float", "MIN": -1.2, "MAX": 0.6, "DEFAULT": -0.4 },
    { "NAME": "eyePulse", "LABEL": "Eye Pulse", "TYPE": "float", "MIN": 0.0, "MAX": 0.3, "DEFAULT": 0.1 }
  ],
  "PASSES": [
    { "TARGET": "helixRender", "DESCRIPTION": "Helix raymarch" },
    { "TARGET": "lensRender", "DESCRIPTION": "Chromatic lens distortion and grain" },
    { "DESCRIPTION": "Gaussian blur and radial vignette" }
  ]
}*/

// Helix Eye — port of "Fear Inoculum" by RagePly
// Source: https://www.shadertoy.com/view/cdSGRw (created 2022-11-02)
//
// This ported shader is licensed under the Creative Commons
// Attribution-NonCommercial-ShareAlike 3.0 Unported License
// (CC BY-NC-SA 3.0), because the source shader carries no licence of its
// own and the Shadertoy default licence applies:
// https://creativecommons.org/licenses/by-nc-sa/3.0/
//
// Attribution: RagePly. Modifications remain under the same licence.
// The Gaussian blur and lens passes are always enabled (the source's
// FEAR_BLUR/FEAR_DIST toggles); sin^2 replaces pow(sin, 2.0) because the
// power function is undefined for negative bases.

// [a, b] -> [0, 1], with clamping.
float lerp( in float x, in float a, in float b )
{
    return (clamp(x, a, b) - a)/(b-a);
}

// https://www.neilmendoza.com/glsl-rotation-about-an-arbitrary-axis/
mat4 rotationMatrix(vec3 axis, float angle)
{
    axis = normalize(axis);
    float s = sin(angle);
    float c = cos(angle);
    float oc = 1.0 - c;

    return mat4(oc * axis.x * axis.x + c,           oc * axis.x * axis.y - axis.z * s,  oc * axis.z * axis.x + axis.y * s,  0.0,
                oc * axis.x * axis.y + axis.z * s,  oc * axis.y * axis.y + c,           oc * axis.y * axis.z - axis.x * s,  0.0,
                oc * axis.z * axis.x - axis.y * s,  oc * axis.y * axis.z + axis.x * s,  oc * axis.z * axis.z + c,           0.0,
                0.0,                                0.0,                                0.0,                                1.0);
}

// feature flags for the eye relief, set by radial_offset()
bool eyelid;
bool iris;
bool pupil;
#define pi 3.141592653589793

// 2D heightmap with input theta: angle around the helix-arm and psi: angle around y-axle
float radial_offset( in float theta, in float psi )
{
    float eye_ridge = 0.7 + eyePulse*sin(TIME*speed);
    float eye_crater = 0.6 + eyePulse*sin(TIME*speed);
    float eye_pupil = 0.7;

    float x = theta+psi+TIME*speed/10.0;
    float y = psi;

    float sx = sin(10.0*x);
    float sy = sin(23.0*y);
    float height = (sx*sx + sy*sy)/2.0;

    if (height > eye_ridge)
    {
        eyelid = true;
        height = 2.0*eye_ridge - height;

        if (height < eye_crater)
        {
            iris = true;
            height = 2.0*eye_crater - height;

            if ( height > eye_pupil ) pupil = true;
        }
    }
    else {
        // simple roughness
        height += 0.0025*(sin(100.0*x) + sin(300.0*y));
        height += 0.0012*(sin(232.0*x+123.0) + sin(600.0*y+789.0));
    }

    return (1.0 - clamp(height, 0.1, 1.0));
}

// adapted from https://www.shadertoy.com/view/ttB3DV
float helix_sdf( in vec3 pos, in float rad, in float vert )
{
    // horizontal distance
    float h = length(pos.xz) - rad;

    // vertical distance from spiral
    float t = pos.y / vert * 2.0 * pi;
    float v = vert * asin(sin(t + 0.5*atan(pos.x, pos.z))) / 2.0 / pi;

    // distance = distance from arm with thickness given by constant + offset map
    return sqrt(h*h + v*v) - 9.0 + radial_offset( atan(v, h), atan(pos.x, pos.z));
}

// https://iquilezles.org/articles/fog/
vec3 applyFog( in vec3  rgb,       // original color of the pixel
               in float distance ) // camera to point distance
{
    float fogAmount = 1.0 - exp( -distance*0.005 );
    vec3  fogColor  = vec3(0.5,0.6,0.7);
    return mix( rgb, fogColor, fogAmount );
}

// deterministic per-pixel grain
vec2 grainSeed( vec2 p )
{
    float h  = fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
    float h2 = fract(sin(dot(p + vec2(19.19, 7.31), vec2(12.9898, 78.233))) * 43758.5453);
    return vec2(h, h2);
}

void main()
{
    if (PASSINDEX == 0)
    {
        // --- helix raymarch ---
        float ratio = RENDERSIZE.y / RENDERSIZE.x;
        vec2 uv = (gl_FragCoord.xy / RENDERSIZE.xy - vec2(0.5, 0.5)) * vec2(1.0, ratio);

        // setup ray
        vec3 o = vec3(0.0,0.0,-0.5);
        vec3 d = normalize(vec3(uv, 0.0) - o);
        o = vec3(uv,0.0);

        // rotate camera
        mat4 xrot = rotationMatrix(vec3(1.0,0.0,0.0), cameraTilt);
        o = (xrot * vec4(o,1.0)).xyz;
        d = (xrot * vec4(d,1.0)).xyz;

        // translate camera
        o += vec3(-7.0,-0.0,-25.0);

        // helix settings
        // coil and radius are uniforms (see INPUTS)

        // raymarch https://iquilezles.org/articles/raymarchingdf/
        float dist = 0.0;
        bool hit = false;
        float h = 0.0;
        int steps = 100;

        for (int iter = 0; iter < 100; iter++)
        {
            steps = iter;
            if (dist >= 100.0) break;

            // reset features
            eyelid = false;
            iris = false;
            pupil = false;

            h = helix_sdf(o,radius,coil);
            if (h < 0.001)
            {
                hit = true;
                break;
            }

            dist += 0.9*h;
            o += 0.9*h*d;
        }

        if (hit)
        {
            // approximate surface normal
            float dDdx = (helix_sdf(o + vec3(0.001, 0.0, 0.0), radius, coil) - h)/0.001;
            float dDdy = (helix_sdf(o + vec3(0.0, 0.001, 0.0), radius, coil) - h)/0.001;
            vec3 n = normalize(vec3(dDdx, dDdy, 1));

            // primary smooth-shading
            float normal = dot(n, normalize(vec3(-1.0, 0.6, 0.5)));
            float shader = lerp(clamp(normal, 0.0, 1.0), -0.02, 2.0);

            // highlight
            vec3 highlight = 1.0 * smoothstep(0.47, 0.5, shader) * vec3(0.396, 0.718, 0.996);

            // Ambient Occlusion
            float abo = exp(-float(steps)*0.05);

            // left arm-highlight
            float left_highlight = 3.0*smoothstep(0.5, 1.0, dot(n, normalize(vec3(-0.4, 1.0, 0.0)))); // direction + amp
            left_highlight *= smoothstep(0.3, 1.0, dot(normalize(o.xz), vec2(-1.0, -0.0))); // sector bias/filter
            left_highlight = 1.2*lerp(left_highlight, 0.0, 1.0); // compress and amplify
            vec3 lh = left_highlight * vec3(0.667, 0.875, 0.969);

            // underside glow
            vec3 light_source = vec3(0.0, -40.0, 30.0);
            float height = 1.0 - lerp(o.y, -40.0, -20.0); // height bias/filter
            float lightcomp = dot(n, normalize(light_source - o)); // light direction
            vec3 light = 5.0 * height * lerp(lightcomp, 0.0, 1.0) * vec3(0.8,0.75,1.0);

            // material color
            vec3 base_color = 0.5 * vec3(0.8,0.8,1.0);
            if (pupil)
            {
                base_color = vec3(-1.0,-1.0,-1.0); // make them really dark
            }
            else if (iris)
            {
                base_color = vec3(2.0, 2.0, 2.0); // have them pop out
            }
            else if (eyelid)
            {
                base_color = vec3(0.8, 0.1, 0.1);
            }

            base_color = lh + highlight + abo * 0.5*shader * base_color + light;
            gl_FragColor = vec4(applyFog(base_color, dist), 1.0);
        }
        else
        {
            gl_FragColor = vec4(vec3(0.0), 1.0);
        }
    }
    else if (PASSINDEX == 1)
    {
        // --- chromatic lens distortion and grain ---
        // https://nullprogram.com/blog/2018/07/31/ (hash adapted to float)
        vec2 uv  = isf_FragNormCoord;

        // direct distortion radially
        float ratio = RENDERSIZE.y / RENDERSIZE.x;
        vec2 of  = (uv - vec2(0.5, 0.5) - vec2(0.0, -0.2)) * vec2(1.0, ratio) / RENDERSIZE.xy;
        float rad = length(of*RENDERSIZE.xy);
        of *= -RENDERSIZE.x/190.0 * smoothstep(0.0, 0.3, rad);

        // apply more distortion towards the highlighted areas
        of *= 1.0 - smoothstep(0.5, 1.5, uv.x); // slight left-bias
        of *= 1.0 - smoothstep(0.5, 1.5, uv.y); // slight down-bias

        // apply some grain
        vec2 g = grainSeed(gl_FragCoord.xy);
        vec2 gof = RENDERSIZE.x * 0.001 * g / RENDERSIZE.xy;

        // sample texture with increasing offset
        vec4 t1  = IMG_NORM_PIXEL(helixRender, uv + 5.0 * of + gof);
        vec4 t4  = IMG_NORM_PIXEL(helixRender, uv + 10.0 * of + gof);
        vec4 t16 = IMG_NORM_PIXEL(helixRender, uv + 15.0 * of + gof);

        float red = t1.r;
        float green = t4.g;
        float blue = t16.b;

        gl_FragColor = vec4(red, green, 1.1*blue, 1.0);
    }
    else
    {
        // --- Gaussian blur and radial vignette ---
        float ratio = RENDERSIZE.y / RENDERSIZE.x;
        vec2 uv = (isf_FragNormCoord - vec2(0.5, 0.5)) * vec2(1.0, ratio);
        vec2 screen = isf_FragNormCoord;

        vec3 tex = vec3(0.0);

        // apply more blur radially, with the center around the "blue lightsource"
        float sigma_factor = smoothstep(0.0, 0.250, length(uv-vec2(0.0, -0.1)));

        // adapt sigma wrt screen resolution
        float sigma = sigma_factor * RENDERSIZE.x/350.0 + 0.1*(1.0 - sigma_factor);
        float G = 0.0;

        // Gaussian blur https://en.wikipedia.org/wiki/Gaussian_blur
        for (int i = -8; i <= 8; i++)
        {
            for (int j = -8; j <= 8; j++)
            {
                float x = float(i);
                float y = float(j);
                vec2 v = vec2(x, y) / RENDERSIZE.xy;

                float weight = exp(- (x*x + y*y)/(2.0*sigma*sigma));

                G += weight;

                tex += weight * IMG_NORM_PIXEL(lensRender, screen + v).rgb;
            }
        }

        vec3 color = tex / G;

        // slight tint around edges
        float radial_shadow = 1.0 - length(uv);
        radial_shadow *= radial_shadow;
        color *= lerp(radial_shadow, -0.5, 1.0);

        gl_FragColor = vec4(color, 1.0);
    }
}
