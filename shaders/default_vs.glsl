#version 330 core
layout(location = 0) in vec2 pos;
layout(location = 1) in vec2 tex_coord_in;

uniform vec4 win_rect;

out vec2 tex_coord;

void main() {
  gl_Position = vec4(pos, 1.0, 1.0);
  tex_coord = vec2(tex_coord_in.x, 1.0 - tex_coord_in.y);
}