#version 440

// Hue and gamma stage of AdjustLayer.qml (compiled to adjust.frag.qsb by ../../../build.sh).
// hueAngle:   chroma rotation in radians (mpv hue -100..100 -> -pi..pi)
// gammaPower: exponent applied to the clamped colour (mpv gamma g -> 1 / 8^(g/100))

layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float hueAngle;
    float gammaPower;
};

layout(binding = 1) uniform sampler2D source;

void main()
{
    vec4 texel = texture(source, qt_TexCoord0);
    vec3 rgb = texel.a > 0.0 ? texel.rgb / texel.a : vec3(0.0);

    // Row-major RGB <-> YIQ; "vector * matrix" applies the rows written below.
    mat3 toYiq = mat3(0.299,  0.587,  0.114,
                      0.596, -0.274, -0.322,
                      0.211, -0.523,  0.312);
    mat3 toRgb = mat3(1.0,  0.956,  0.621,
                      1.0, -0.272, -0.647,
                      1.0, -1.106,  1.703);

    vec3 yiq = rgb * toYiq;
    float c = cos(hueAngle);
    float s = sin(hueAngle);
    yiq.yz = vec2(c * yiq.y - s * yiq.z, s * yiq.y + c * yiq.z);
    rgb = clamp(yiq * toRgb, 0.0, 1.0);

    rgb = pow(rgb, vec3(gammaPower));

    fragColor = vec4(rgb * texel.a, texel.a) * qt_Opacity;
}
