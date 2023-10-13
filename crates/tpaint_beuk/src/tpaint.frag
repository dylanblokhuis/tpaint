#version 460
#extension GL_EXT_nonuniform_qualifier : enable
#extension GL_EXT_scalar_block_layout : enable

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec4 in_color;

layout (location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform texture2D u_textures[];
layout(set = 0, binding = 1) uniform sampler sampler_llc;

layout(push_constant, scalar) uniform PushConstants {
    uint texture_index;
    vec2 screen_size;
    uint _pad;
} pc;

vec3 gamma_from_linear_rgb(vec3 linear_rgb) {
    bvec3 cutoff = lessThan(linear_rgb, vec3(0.0031308));
    vec3 lower = linear_rgb * 12.92;
    vec3 higher = (1.055 * pow(linear_rgb, vec3(1.0 / 2.4))) - 0.055;

    return mix(lower, higher, cutoff);
}
vec4 gamma_from_linear_rgba(vec4 linear_rgba) {
    return vec4(gamma_from_linear_rgb(linear_rgba.rgb), linear_rgba.a);
}

vec3 linear_from_gamma_rgb(vec3 gamma_rgb) {
    bvec3 cutoff = lessThan(gamma_rgb, vec3(0.04045));
    vec3 lower = gamma_rgb / 12.92;
    vec3 higher = pow((gamma_rgb + 0.055) / 1.055, vec3(2.4));

    return mix(lower, higher, cutoff);
}

vec4 linear_from_gamma_rgba(vec4 gamma_rgba) {
    return vec4(linear_from_gamma_rgb(gamma_rgba.rgb), gamma_rgba.a);
}

void main() {
  vec4 tex_linear = linear_from_gamma_rgba(texture(sampler2D(u_textures[pc.texture_index], sampler_llc), in_uv));
  vec4 blended_color = in_color * tex_linear;
  out_color = gamma_from_linear_rgba(blended_color);
}
