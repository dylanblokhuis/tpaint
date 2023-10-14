#version 460
#extension GL_EXT_nonuniform_qualifier : enable
#extension GL_EXT_scalar_block_layout : enable

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in uint in_color;

layout(location = 0) out vec2 out_uv;
layout(location = 1) out vec4 out_color;

layout(push_constant, scalar) uniform PushConstants {
    uint texture_index;
    vec2 screen_size;
    uint _pad;
} pc;

vec4 position_from_screen(vec2 screen_pos) {
    return vec4(
        2.0 * screen_pos.x / pc.screen_size.x - 1.0,
        2.0 * screen_pos.y / pc.screen_size.y - 1.0,
        0.0,
        1.0
    );
}

vec4 unpack_color(uint color) {
    return vec4(
        (color & 255),
        ((color >> 8) & 255),
        ((color >> 16) & 255),
        ((color >> 24) & 255)
    ) / 255.0;
}

void main() {
    out_uv = in_uv;
    out_color = unpack_color(in_color);
    gl_Position = position_from_screen(in_pos);
}