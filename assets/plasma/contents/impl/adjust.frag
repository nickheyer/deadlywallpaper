#version 440
// Colour adjustments for media wallpapers: saturation, hue, brightness, contrast, gamma.
// Every control is -1..1 with 0 meaning "unchanged", matching libmpv's -100..100 ranges.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float saturation;
    float hue;
    float brightness;
    float contrast;
    float gamma;
};
layout(binding = 1) uniform sampler2D source;

vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

void main() {
    vec4 t = texture(source, qt_TexCoord0);
    vec3 c = t.rgb / max(t.a, 1.0e-4);
    vec3 hsv = rgb2hsv(c);
    hsv.x = fract(hsv.x + hue * 0.5);
    hsv.y = clamp(hsv.y * (1.0 + saturation), 0.0, 1.0);
    c = hsv2rgb(hsv);
    c = (c - 0.5) * (1.0 + contrast) + 0.5 + brightness * 0.5;
    float g = gamma >= 0.0 ? 1.0 / (1.0 + gamma * 3.0) : (1.0 - gamma * 3.0);
    c = pow(clamp(c, 0.0, 1.0), vec3(g));
    fragColor = vec4(c * t.a, t.a) * qt_Opacity;
}
