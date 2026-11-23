/*
{
  "DESCRIPTION": "Polyhedral Grid Fractal with morphing structured symmetry.",
  "CATEGORIES": [
    "Sacred Geometry",
    "Fractal",
    "Visual"
  ],
  "ISFVSN": "2.0",
  "INPUTS": [
    {
      "NAME": "Speed",
      "TYPE": "float",
      "DEFAULT": 1.0,
      "MIN": 0.1,
      "MAX": 5.0
    },
    {
      "NAME": "MorphStrength",
      "TYPE": "float",
      "DEFAULT": 0.5,
      "MIN": 0.0,
      "MAX": 2.0
    },
    {
      "NAME": "ColorPulse",
      "TYPE": "float",
      "DEFAULT": 0.5,
      "MIN": 0.0,
      "MAX": 1.0
    },
    {
      "NAME": "GridTiling",
      "TYPE": "float",
      "DEFAULT": 2.0,
      "MIN": 1.0,
      "MAX": 4.0
    }
  ],
  "CREDIT": "Port of https://github.com/Ethereios/ISF-shaders by Ethereios (Sleepless Monk)"
}
*/

// polyhedral — imported from https://github.com/Ethereios/ISF-shaders
// Upstream archive: polyhedral.zip.
// Upstream source: not stated in the upstream file.
// Original author(s): not stated in the upstream file.
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


void main() {
    vec2 uv = (gl_FragCoord.xy - 0.5 * RENDERSIZE.xy) / RENDERSIZE.y;
    float time = TIME * Speed;
    vec3 color = vec3(0.0);

    for (float i = 0.0; i < 5.0; i++) {
        uv = fract(uv * GridTiling) - 0.5;
        float d = length(uv) * (1.5 + MorphStrength * sin(time * 0.8));

        color += vec3(
            sin(time + d * ColorPulse),
            cos(time + d * 1.5),
            sin(time + d * 2.5)
        ) * exp(-d * 2.0);
    }

    gl_FragColor = vec4(color, 1.0);
}
