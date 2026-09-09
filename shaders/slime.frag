#version 450

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

layout(std430, set = 0, binding = 0) readonly buffer PetShape {
    vec2 points[24];
} shape;

layout(push_constant) uniform PetParams {
    float time;
    float opacity;
} pet;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453123);
}

float value_noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

void main() {
    vec2 uv = v_uv;

    vec2 center = vec2(0.0);
    for (int i = 0; i < 24; i++) {
        center += shape.points[i];
    }
    center /= 24.0;

    vec2 p = uv - center;
    float dist = length(p);
    vec2 dir = dist > 0.0001 ? p / dist : vec2(1.0, 0.0);

    float radius_sum = 0.0;
    float weight_sum = 0.0;
    for (int i = 0; i < 24; i++) {
        vec2 q = shape.points[i] - center;
        float q_len = max(length(q), 0.0001);
        vec2 q_dir = q / q_len;
        float w = pow(max(dot(dir, q_dir), 0.0), 18.0) + 0.0002;
        radius_sum += q_len * w;
        weight_sum += w;
    }
    float radius = radius_sum / weight_sum;

    float d = dist - radius;
    float stain = smoothstep(0.080, -0.045, d);
    float inner = smoothstep(0.120, -0.165, d);

    float uneven = mix(0.988, 1.015, value_noise(uv * 2.4 + vec2(pet.time * 0.002, 0.0)));

    vec3 amber = vec3(0.43, 0.265, 0.120);
    vec3 warm_brown = vec3(0.245, 0.140, 0.067);
    vec3 deep_umber = vec3(0.108, 0.062, 0.036);
    vec3 color = mix(amber, warm_brown, inner * 0.54);
    color = mix(color, deep_umber, smoothstep(-0.09, -0.22, d) * 0.42);

    float boundary = exp(-abs(d) * 17.0) * 0.12;
    color += vec3(0.09, 0.022, 0.004) * boundary;

    float alpha = stain * (0.13 + inner * 0.18) * pet.opacity * uneven;
    alpha *= 1.0 + 0.006 * sin(pet.time * 0.8);

    out_color = vec4(color, clamp(alpha, 0.0, 0.32));
}
