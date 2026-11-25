/*{
    "CATEGORIES": [
        "Masking",
        "Utility"
    ],
    "CREDIT": "VmodV",
    "DESCRIPTION": "Colour key: pixels close to Key Colour become transparent.",
    "ISFVSN": "2",
    "INPUTS": [
        {
            "NAME": "inputImage",
            "TYPE": "image"
        },
        {
            "DEFAULT": [
                0,
                1,
                0,
                1
            ],
            "LABEL": "Key Colour",
            "NAME": "keyColor",
            "TYPE": "color"
        },
        {
            "DEFAULT": 0.28,
            "LABEL": "Tolerance",
            "MAX": 1,
            "MIN": 0,
            "NAME": "tolerance",
            "TYPE": "float"
        },
        {
            "DEFAULT": 0.08,
            "LABEL": "Softness",
            "MAX": 0.5,
            "MIN": 0,
            "NAME": "softness",
            "TYPE": "float"
        },
        {
            "DEFAULT": 0.5,
            "LABEL": "Spill Suppression",
            "MAX": 1,
            "MIN": 0,
            "NAME": "spill",
            "TYPE": "float"
        }
    ]
}
*/

void main() {
    vec4 src = IMG_THIS_NORM_PIXEL(inputImage);

    // Internal canvases are premultiplied; key against straight colour.
    float srcAlpha = max(src.a, 0.0001);
    vec3 straight = src.rgb / srcAlpha;

    // 0 = exact key colour, 1 = far enough away to keep.
    float distanceFromKey = distance(straight, keyColor.rgb);
    float matte = smoothstep(
        tolerance,
        tolerance + max(softness, 0.0001),
        distanceFromKey
    );

    // Desaturate soft edges towards luma so the key colour leaves no fringe.
    float luma = dot(straight, vec3(0.2126, 0.7152, 0.0722));
    vec3 rgb = mix(straight, vec3(luma), (1.0 - matte) * spill);

    float outAlpha = srcAlpha * matte;
    gl_FragColor = vec4(rgb * outAlpha, outAlpha);
}
