#version 450

layout(location = 0) out vec2 v_uv;

vec2 positions[3] = vec2[](
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0)
);

void main() {
    vec2 p = positions[gl_VertexIndex];
    v_uv = p * 0.5 + 0.5;
    gl_Position = vec4(p, 0.0, 1.0);
}
