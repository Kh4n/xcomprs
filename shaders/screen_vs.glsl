#version 330 core
layout(location = 0) in vec2 pos;
layout(location = 1) in vec2 tex_coord_in;

out vec2 tex_coord;

void main() {
  gl_Position = vec4(pos.x * 2 - 1, pos.y * 2 + 1, 0.0, 1.0);
  // gl_Position = vec4(pos, 0.0, 1.0);
  tex_coord = tex_coord_in;
}