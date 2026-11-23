/*
{
  "DESCRIPTION": "Fractal Waves - Dynamic fractal with tunable inputs",
  "INPUTS": [
    {
      "NAME": "speed",
      "TYPE": "float",
      "DEFAULT": 0.5,
      "MIN": 0.1,
      "MAX": 2.0
    },
    {
      "NAME": "zoom",
      "TYPE": "float",
      "DEFAULT": 1.5,
      "MIN": 0.5,
      "MAX": 5.0
    },
    {
      "NAME": "colorIntensity",
      "TYPE": "float",
      "DEFAULT": 1.0,
      "MIN": 0.1,
      "MAX": 3.0
    }
  ],
  "CREDIT": "Port of https://github.com/Ethereios/ISF-shaders by Ethereios (Sleepless Monk)"
}
*/

// julia — imported from https://github.com/Ethereios/ISF-shaders
// Upstream archive: julia.zip.
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
    vec2 uv = (gl_FragCoord.xy * 2.0 - RENDERSIZE.xy) / RENDERSIZE.y;
    vec3 color = vec3(0.0);
    float globalDist = length(uv);
    
    for (float i = 0.0; i < 4.0; i++) {
        uv = fract(uv * zoom) * 2.0 - 1.0;
        float dist = length(uv) * globalDist;
        color = 0.1 / abs(vec3(dist) - 0.5);
        color = abs(sin(color + globalDist + dist * 2.0 + TIME * speed));
        color = pow(color, vec3(colorIntensity));
    }

    gl_FragColor = vec4(color, 1.0);
}
