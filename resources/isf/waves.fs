/*
{
  "DESCRIPTION": "Kaleidoscopic Waves",
  "CATEGORIES": [
    "Kaleidoscope",
    "Morphing"
  ],
  "INPUTS": [
    {
      "NAME": "warp_intensity",
      "TYPE": "float",
      "DEFAULT": 1.0,
      "MIN": 0.5,
      "MAX": 3.0
    },
    {
      "NAME": "twist_speed",
      "TYPE": "float",
      "DEFAULT": 0.8,
      "MIN": 0.1,
      "MAX": 2.0
    }
  ],
  "CREDIT": "Port of https://github.com/Ethereios/ISF-shaders by Ethereios (Sleepless Monk)"
}
*/

// waves — imported from https://github.com/Ethereios/ISF-shaders
// Upstream archive: waves (1).zip.
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


#define PI 3.1415926538

void main() {
    vec2 uv = (gl_FragCoord.xy * 2.0 - RENDERSIZE.xy) / RENDERSIZE.y;
    vec3 finalColor = vec3(0.0);
    
    float time = TIME * twist_speed;
    
    uv *= mat2(cos(time), -sin(time), sin(time), cos(time));
    uv = fract(uv * warp_intensity) - 0.5;
    
    float dist = length(uv);
    float wave = sin(PI * 4.0 * dist + time * 2.0);
    vec3 color = vec3(0.5 * wave + 0.5, 0.3 * wave + 0.7, 0.8 * wave + 0.2);
    
    gl_FragColor = vec4(color, 1.0);
}
