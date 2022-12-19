#version 330 core
out vec4 frag_color;

in vec2 tex_coord;

// texture samplers
uniform sampler2D win_texture;
uniform sampler2D bg_texture;

uniform vec2 screen_rect;
uniform vec4 win_rect;

void main() {
  float x = win_rect.x;
  float y = win_rect.y;
  float ww = win_rect.z;
  float wh = win_rect.w;

  float sw = screen_rect.x;
  float sh = screen_rect.y;

  // normalize and convert to uv
  x = x / sw;
  y = (sh - wh - y) / sh;
  ww = ww / sw;
  wh = wh / sh;

  vec2 bg_tex_coord = vec2(
    x + tex_coord.x * ww,
    y + tex_coord.y * wh
  );

  // xorg pixmap is inverted for some reason
  vec2 tex_coord_win = vec2(tex_coord.x, 1 - tex_coord.y);
  frag_color = texture(win_texture, tex_coord_win) + 0.1 * texture(bg_texture, bg_tex_coord);
}