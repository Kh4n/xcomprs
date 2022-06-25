#version 330 core
layout(location = 0) in vec2 pos;
layout(location = 1) in vec2 tex_coord_in;

uniform vec2 screen_rect;
uniform vec4 win_rect;

out vec2 tex_coord;

void main() {
  float w = screen_rect.x;
  float h = screen_rect.y;
  // ndc_t = normalized device coordinate transform
  // the translation and scale for our transform matrix
  vec4 ndc_t = vec4(
    2.0*win_rect.x/w - 1.0, - 2.0*win_rect.y/h,
    2.0*win_rect.z/w, 2.0*win_rect.w/h
  );

  vec2 translate = vec2(2*win_rect.x/w - 1, 1 - 2*win_rect.y/h);
  mat4 transform = mat4(
    2*win_rect.z/w, 0.0, 0.0, 0,
    0.0, 2*win_rect.w/h, 0.0, 0,
    0.0, 0.0, 1, 0,
    0.0, 0.0, 0.0, 1
  ) * mat4(
    1, 0.0, 0.0, translate.x,
    0.0, 1, 0.0, translate.y,
    0.0, 0.0, 1, 0,
    0.0, 0.0, 0.0, 1
  );

  gl_Position = vec4(pos, 0.0, 1.0)*transform;
  tex_coord = tex_coord_in;
}
