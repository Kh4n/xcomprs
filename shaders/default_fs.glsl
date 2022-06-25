#version 330 core
out vec4 frag_color;

in vec2 tex_coord;

// texture samplers
uniform sampler2D win_texture;
uniform sampler2D bg_texture;

void main() {
  // xorg pixmap is inverted for some reason
  vec2 tex_coord_win = vec2(tex_coord.x, 1 - tex_coord.y);
  frag_color = texture(win_texture, tex_coord_win) +
               0.1 * texture(bg_texture, tex_coord);
}