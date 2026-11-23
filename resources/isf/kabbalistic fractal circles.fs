/*
{
  "CATEGORIES": [
    "Psychedelic",
    "Automatically Converted",
    "Shadertoy"
  ],
  "DESCRIPTION": "Enhanced version of https://www.shadertoy.com/view/X33yRB by ekaunt. Added tunable parameters for color shift, psychedelic pulsing, speed, morphing, and more.",
  "IMPORTED": {},
  "INPUTS": [
    {
      "NAME": "Speed",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 5.0,
      "DEFAULT": 1.0
    },
    {
      "NAME": "Zoom",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 20.0,
      "DEFAULT": 10.0
    },
    {
      "NAME": "ColorShiftSpeed",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 5.0,
      "DEFAULT": 1.0
    },
    {
      "NAME": "MorphingSpeed",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 5.0,
      "DEFAULT": 1.0
    },
    {
      "NAME": "PsychedelicIntensity",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 5.0,
      "DEFAULT": 1.0
    },
    {
      "NAME": "BackgroundColor",
      "TYPE": "color",
      "DEFAULT": [
        0.0,
        0.0,
        0.0,
        1.0
      ]
    }
  ],
  "CREDIT": "Port of https://github.com/Ethereios/ISF-shaders by Ethereios (Sleepless Monk); upstream source: https://www.shadertoy.com/view/X33yRB by ekaunt"
}
*/

// kabbalistic fractal circles — imported from https://github.com/Ethereios/ISF-shaders
// Upstream archive: kabbalistic fractal circles.zip.
// Upstream source: https://www.shadertoy.com/view/X33yRB.
// Original author(s): ekaunt.
// Adapter/publisher: Ethereios (Sleepless Monk).
//
// Licensing: the upstream repository LICENSE is CC0-1.0, but its README
// says the shaders are released under a Creative Commons non-commercial
// license and that they were customized from Shadertoy (whose default is
// CC BY-NC-SA 3.0). The more restrictive reading applies: this port is
// treated as CC BY-NC-SA 3.0 (Attribution-NonCommercial-ShareAlike).
// Commercial use is not permitted and modifications stay under the same
// license. See THIRD_PARTY_NOTICES.md.
// Modification: an ISF CREDIT field and this attribution note were added.


#define Pi 3.1415926535
#define Pi2 (Pi * 2.)
#define TIME TIME
#define RENDERSIZE RENDERSIZE
#define ITERATIONS 10

// Customizable color palette
vec3 palette(float t) {
    vec3 a = vec3(0.5, 0.5, 0.5);
    vec3 b = vec3(0.5, 0.5, 0.5);
    vec3 c = vec3(1.0, 1.0, 1.0);
    vec3 d = vec3(0.0, 0.33, 0.67);
    return a + b * cos(Pi2 * (c * t + d));
}

void main() {
    // Initialize UV coordinates
    vec2 uv = (gl_FragCoord.xy / RENDERSIZE.y - RENDERSIZE.xy / RENDERSIZE.y) / Zoom;
    vec2 uv0 = uv;

    // Dynamic variables
    float depth = TIME * Speed;
    float angle = depth * MorphingSpeed;
    vec3 color = BackgroundColor.rgb;

    // Iterative fractal rendering with fixed iteration count
    for (int j = 0; j < ITERATIONS; j++) {
        float jFactor = float(j) * (Zoom / float(ITERATIONS));
        
        uv = fract(uv * 2.0) - 0.5;
        uv *= mat2(cos(angle), -sin(angle), sin(angle), cos(angle));

        // Distance-based smoothing
        float d = smoothstep(0.0, 0.15, abs(sin(length(uv) * 8.0 + TIME * Speed) / 8.0));
        d = 0.02 / d;

        // Color shifting and psychedelic pulsing
        float shiftedTime = TIME * ColorShiftSpeed;
        vec3 col = palette(length(uv * jFactor) + shiftedTime);
        col *= palette(length(uv0 * jFactor) + shiftedTime);
        col *= PsychedelicIntensity;

        // Accumulate colors
        color += col * d;
    }

    // Output final color
    gl_FragColor = vec4(color, 1.0);
}