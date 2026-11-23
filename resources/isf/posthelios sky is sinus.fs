/*
{
  "DESCRIPTION": "Psychedelic kaleidoscopic pattern with tunable color, brightness, geometry, speed, morph, and zoom",
  "CATEGORIES": [
    "Automatically Converted",
    "Shadertoy",
    "Color",
    "Psychedelic"
  ],
  "INPUTS": [
    {
      "NAME": "ColorPulseFreq",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 5.0,
      "DEFAULT": 1.0,
      "LABEL": "Color Pulse Frequency"
    },
    {
      "NAME": "ColorPulseAmp",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 1.0,
      "DEFAULT": 0.5,
      "LABEL": "Color Pulse Amplitude"
    },
    {
      "NAME": "PaletteSelect",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 2.0,
      "DEFAULT": 0.0,
      "LABEL": "Color Palette",
      "VALUES": [
        "Classic",
        "Acid",
        "Chill"
      ]
    },
    {
      "NAME": "PaletteShift",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 1.0,
      "DEFAULT": 0.5,
      "LABEL": "Palette Shift"
    },
    {
      "NAME": "KaleidoStrength",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 3.0,
      "DEFAULT": 1.0,
      "LABEL": "Kaleidoscope Strength"
    },
    {
      "NAME": "PsychedelicFactor",
      "TYPE": "float",
      "MIN": 0.5,
      "MAX": 3.0,
      "DEFAULT": 1.8,
      "LABEL": "Psychedelic Enhancement"
    },
    {
      "NAME": "Brightness",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 2.0,
      "DEFAULT": 0.5,
      "LABEL": "Brightness"
    },
    {
      "NAME": "GeometryMorph",
      "TYPE": "float",
      "MIN": 0.0,
      "MAX": 1.0,
      "DEFAULT": 0.0,
      "LABEL": "Geometry Morph"
    },
    {
      "NAME": "Speed",
      "TYPE": "float",
      "MIN": 0.1,
      "MAX": 4.0,
      "DEFAULT": 0.2,
      "LABEL": "Animation Speed"
    },
    {
      "NAME": "Zoom",
      "TYPE": "float",
      "MIN": 1.0,
      "MAX": 5.0,
      "DEFAULT": 1.0,
      "LABEL": "Zoom"
    }
  ],
  "ISFVSN": "2",
  "CREDIT": "Port of https://github.com/Ethereios/ISF-shaders by Ethereios (Sleepless Monk); upstream source: https://iquilezles.org/articles/palettes/ by posthelios (named in the upstream title)"
}
*/

// posthelios sky is sinus — imported from https://github.com/Ethereios/ISF-shaders
// Upstream archive: posthelios sky is sinus.zip.
// Upstream source: https://iquilezles.org/articles/palettes/.
// Original author(s): posthelios (named in the upstream title).
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


// https://iquilezles.org/articles/palettes/ 
vec3 palette(float t, float mode) {
    vec3 a = vec3(0.5, 0.5, 0.5);
    vec3 b = vec3(0.5, 0.5, 0.5);
    vec3 c = vec3(1.0, 1.0, 1.0);
    vec3 d = vec3(0.263, 0.416, 0.557);

    if (mode == 1.0) {
        // Acid palette
        a = vec3(0.9, 0.1, 0.1);
        b = vec3(0.1, 0.8, 0.1);
        c = vec3(0.0, 0.0, 1.0);
        d = vec3(0.33, 0.66, 0.0);
    } else if (mode == 2.0) {
        // Chill palette
        a = vec3(0.1, 0.2, 0.4);
        b = vec3(0.5, 0.7, 0.9);
        c = vec3(0.0, 0.2, 0.5);
        d = vec3(0.5, 0.3, 0.1);
    }

    return a + b * sin(6.28318 * (tan(c) * sin(t) + tan(d)));
}

void main() {
    float time = TIME * Speed;

    vec2 uv = (gl_FragCoord.xy * 2.0 - RENDERSIZE.xy) / RENDERSIZE.y;
    vec2 uv0 = uv;
    uv = normalize(vec3(uv, 1.2)).xy;

    vec3 finalColor = vec3(0.0);

    for (float i = 0.0; i < 13.0; i++) {
        uv = fract(atan(uv) * 1.2) - 0.5;
        uv *= mat2(cos(GeometryMorph), -sin(GeometryMorph), sin(GeometryMorph), cos(GeometryMorph));

        float d = length(uv) * exp(-length(uv0));
        vec3 col = palette(length(uv0) + i * 0.004 + time * ColorPulseFreq, PaletteSelect);
        d = sin(d * 12.0 + time * i) / 14.0;
        d = abs(d);
        d = pow(0.002 / d, 0.9);
        finalColor += col * d;
    }

    // Enhance psychedelic colors
    finalColor = pow(finalColor, vec3(PsychedelicFactor));

    // Add zoom effect
    finalColor *= Zoom;

    // Apply brightness
    finalColor *= Brightness;

    gl_FragColor = vec4(finalColor, 1.0);
}