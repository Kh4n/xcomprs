#version 330 core
out vec4 frag_color;

in vec2 tex_coord;

// texture samplers
uniform sampler2D win_texture;
uniform sampler2D bg_texture;

void main() {
  frag_color = texture(win_texture, tex_coord) +
               .000001 * texture(bg_texture, tex_coord);
}