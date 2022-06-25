use crate::errors;
use crate::gl;
use crate::glx;
use crate::win;
use crate::xlib;

use std::convert::TryInto;
use std::ffi::{c_void, CString};
use std::fmt::Debug;
use std::mem::size_of;
use std::ptr::{null, null_mut};

use x11rb::protocol::xproto::Window;

const PIXMAP_ATTRS: [i32; 5] = [
    glx::TEXTURE_TARGET_EXT as i32,
    glx::TEXTURE_2D_EXT as i32,
    glx::TEXTURE_FORMAT_EXT as i32,
    glx::TEXTURE_FORMAT_RGBA_EXT as i32,
    xlib::None as i32,
];

const WIN_RECT_UNIFORM_NAME: &'static str = "win_rect";
const SCREEN_RECT_UNIFORM_NAME: &'static str = "screen_rect";
const WIN_TEXTURE_UNIFORM_NAME: &'static str = "win_texture";
const BG_TEXTURE_UNIFORM_NAME: &'static str = "bg_texture";
const SCREEN_TEXTURE_UNIFORM_NAME: &'static str = "screen_texture";

#[derive(Debug)]
struct FboTexture {
    fbo: gl::types::GLuint,
    texture: gl::types::GLuint,
}

#[derive(Debug)]
pub struct WindowDrawDesc {
    vao: gl::types::GLuint,
    win_shader: gl::types::GLuint,
    screen_shader: gl::types::GLuint,

    target: FboTexture,
    background: FboTexture,

    win_rect_uniform_handle: gl::types::GLint,
    screen_rect_uniform_handle: gl::types::GLint,
    win_texture_uniform_handle: gl::types::GLint,
    bg_texture_uniform_handle: gl::types::GLint,

    screen_texture_uniform_handle: gl::types::GLint,
}

impl WindowDrawDesc {
    pub fn new_shader_paths(
        verts: &Vec<f32>,
        indices: &Vec<u32>,

        win_vs_path: &str,
        win_fs_path: &str,
        screen_vs_path: &str,
        screen_fs_path: &str,

        screen_width: u16,
        screen_height: u16,
    ) -> Result<WindowDrawDesc, errors::CompError> {
        WindowDrawDesc::new(
            verts,
            indices,
            &std::fs::read_to_string(win_vs_path)?,
            &std::fs::read_to_string(win_fs_path)?,
            &std::fs::read_to_string(screen_vs_path)?,
            &std::fs::read_to_string(screen_fs_path)?,
            screen_width,
            screen_height,
        )
    }
    pub fn new(
        verts: &Vec<f32>,
        indices: &Vec<u32>,

        win_vs_source: &String,
        win_fs_source: &String,
        screen_vs_source: &String,
        screen_fs_source: &String,

        screen_width: u16,
        screen_height: u16,
    ) -> Result<WindowDrawDesc, errors::CompError> {
        let mut ret = WindowDrawDesc {
            vao: 0,
            win_shader: 0,
            screen_shader: 0,

            target: FboTexture { fbo: 0, texture: 0 },
            background: FboTexture { fbo: 0, texture: 0 },

            win_rect_uniform_handle: 0,
            screen_rect_uniform_handle: 0,
            win_texture_uniform_handle: 0,
            bg_texture_uniform_handle: 0,

            screen_texture_uniform_handle: 0,
        };

        let mut vbo: gl::types::GLuint = 0;
        let mut ebo: gl::types::GLuint = 0;

        unsafe {
            ret.win_shader = create_shader(
                CString::new(win_vs_source.as_bytes())?,
                CString::new(win_fs_source.as_bytes())?,
            )?;

            // test if shader has uniforms we need
            gl::UseProgram(ret.win_shader);
            ret.win_rect_uniform_handle = gl::GetUniformLocation(
                ret.win_shader,
                CString::new(WIN_RECT_UNIFORM_NAME)?.as_ptr(),
            );
            ret.screen_rect_uniform_handle = gl::GetUniformLocation(
                ret.win_shader,
                CString::new(SCREEN_RECT_UNIFORM_NAME)?.as_ptr(),
            );
            ret.win_texture_uniform_handle = gl::GetUniformLocation(
                ret.win_shader,
                CString::new(WIN_TEXTURE_UNIFORM_NAME)?.as_ptr(),
            );
            ret.bg_texture_uniform_handle = gl::GetUniformLocation(
                ret.win_shader,
                CString::new(BG_TEXTURE_UNIFORM_NAME)?.as_ptr(),
            );
            if ret.win_rect_uniform_handle < 0 {
                Err(format!(
                    "the window shader does not define or does not use '{}'",
                    WIN_RECT_UNIFORM_NAME
                ))?
            }
            if ret.screen_rect_uniform_handle < 0 {
                Err(format!(
                    "the window shader does not define or does not use '{}'",
                    SCREEN_RECT_UNIFORM_NAME
                ))?
            }
            if ret.win_texture_uniform_handle < 0 {
                Err(format!(
                    "the window shader does not define or does not use '{}'",
                    WIN_TEXTURE_UNIFORM_NAME
                ))?
            }
            if ret.bg_texture_uniform_handle < 0 {
                Err(format!(
                    "the window shader does not define or does not use '{}'",
                    BG_TEXTURE_UNIFORM_NAME
                ))?
            }
        }

        unsafe {
            ret.screen_shader = create_shader(
                CString::new(screen_vs_source.as_bytes())?,
                CString::new(screen_fs_source.as_bytes())?,
            )?;
            ret.screen_texture_uniform_handle = gl::GetUniformLocation(
                ret.screen_shader,
                CString::new(SCREEN_TEXTURE_UNIFORM_NAME)?.as_ptr(),
            );
            if ret.screen_texture_uniform_handle < 0 {
                Err(format!(
                    "the shader does not define or does not use '{}'",
                    SCREEN_TEXTURE_UNIFORM_NAME
                ))?
            }
        }

        if let Some(i) = indices.iter().find(|&&i| i >= verts.len() as u32) {
            Err(format!("indices contain out of range vertex: {}", i))?
        }
        match (verts.len(), indices.len()) {
            (v, _) if v % 4 != 0 => Err(format!(
                "vertices not a multiple of 4 (must be x,y,u,v where u,v are texture coords): {}",
                v
            ))?,
            (_, i) if i % 3 != 0 => Err(format!(
                "indices not a multiple of 3 (must be triangles): {}",
                i
            ))?,
            (v, i) if v < 3 * 4 || i < 3 => Err(format!(
                "must specify at least one triangle: verts:{} indices{}",
                v, i
            ))?,
            _ => (),
        }

        unsafe {
            gl::GenVertexArrays(1, &mut ret.vao as *mut gl::types::GLuint);
            gl::GenBuffers(1, &mut vbo as *mut gl::types::GLuint);
            gl::GenBuffers(1, &mut ebo as *mut gl::types::GLuint);

            gl::BindVertexArray(ret.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (verts.len() * size_of::<f32>()) as gl::types::GLsizeiptr,
                verts.as_ptr() as *const c_void,
                gl::STATIC_DRAW,
            );
            // position
            gl::VertexAttribPointer(
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * size_of::<f32>()) as gl::types::GLsizei,
                0 as *const c_void,
            );
            gl::EnableVertexAttribArray(0);
            // texture coords
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * size_of::<f32>()) as gl::types::GLsizei,
                (2 * size_of::<f32>()) as *const c_void,
            );
            gl::EnableVertexAttribArray(1);

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (indices.len() * size_of::<u32>()) as isize,
                indices.as_ptr() as *const c_void,
                gl::STATIC_DRAW,
            );
        }

        unsafe {
            ret.target = gen_framebuffer(screen_width, screen_height)?;
            ret.background = gen_framebuffer(screen_width, screen_height)?;
        }

        Ok(ret)
    }
}

unsafe fn create_shader(
    vs_source: CString,
    fs_source: CString,
) -> Result<gl::types::GLuint, errors::CompError> {
    let vertex_shader = compile_shader(vs_source, gl::VERTEX_SHADER)?;
    let frag_shader = compile_shader(fs_source, gl::FRAGMENT_SHADER)?;
    let ret = link_shaders(vertex_shader, frag_shader)?;

    gl::DetachShader(ret, vertex_shader);
    gl::DetachShader(ret, frag_shader);
    gl::DeleteShader(vertex_shader);
    gl::DeleteShader(frag_shader);

    Ok(ret)
}

unsafe fn link_shaders(
    vertex_shader: u32,
    frag_shader: u32,
) -> Result<gl::types::GLuint, errors::CompError> {
    let ret = gl::CreateProgram();
    gl::AttachShader(ret, vertex_shader);
    gl::AttachShader(ret, frag_shader);
    gl::LinkProgram(ret);

    let mut success: gl::types::GLint = 0;
    let mut log: [u8; 512] = [0; 512];
    gl::GetProgramiv(ret, gl::LINK_STATUS, &mut success);
    if success == gl::FALSE as i32 {
        gl::GetShaderInfoLog(ret, 512, null_mut(), &mut log as *mut _ as *mut i8);
        Err(format!(
            "unable to link shader: {:?}",
            String::from_utf8_lossy(&log)
        ))?;
    };
    Ok(ret)
}

unsafe fn compile_shader(
    source: CString,
    shader_type: gl::types::GLenum,
) -> Result<u32, errors::CompError> {
    let shader = gl::CreateShader(shader_type);
    gl::ShaderSource(shader, 1, &source.as_ptr(), null());
    gl::CompileShader(shader);
    let mut success: gl::types::GLint = 0;
    let mut log: [u8; 512] = [0; 512];
    gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
    if success == gl::FALSE as gl::types::GLint {
        gl::GetShaderInfoLog(shader, 512, null_mut(), &mut log as *mut _ as *mut i8);
        Err(format!(
            "unable to compile shader: {:?}",
            String::from_utf8_lossy(&log)
        ))?;
    }
    Ok(shader)
}

unsafe fn gen_framebuffer(
    screen_width: u16,
    screen_height: u16,
) -> Result<FboTexture, errors::CompError> {
    let mut fbo: gl::types::GLuint = 0;
    gl::GenFramebuffers(1, &mut fbo as *mut gl::types::GLuint);
    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
    let mut texture: gl::types::GLuint = 0;
    gl::GenTextures(1, &mut texture as *mut gl::types::GLuint);
    gl::BindTexture(gl::TEXTURE_2D, texture);
    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        gl::RGB.try_into()?,
        screen_width as i32,
        screen_height as i32,
        0,
        gl::RGB,
        gl::UNSIGNED_BYTE,
        null(),
    );
    gl::TexParameteri(
        gl::TEXTURE_2D,
        gl::TEXTURE_MIN_FILTER,
        gl::NEAREST as gl::types::GLint,
    );
    gl::TexParameteri(
        gl::TEXTURE_2D,
        gl::TEXTURE_MAG_FILTER,
        gl::NEAREST as gl::types::GLint,
    );
    gl::FramebufferTexture2D(
        gl::FRAMEBUFFER,
        gl::COLOR_ATTACHMENT0,
        gl::TEXTURE_2D,
        texture,
        0,
    );
    Ok(FboTexture {
        fbo: fbo,
        texture: texture,
    })
}

#[derive(Debug)]
pub struct GLRenderer {
    // TODO: allow different descs for different windows
    desc: WindowDrawDesc,
}

// TODO: draw borders
// TODO: find out what i meant by "draw borders"
impl GLRenderer {
    pub fn new(desc: WindowDrawDesc) -> Result<GLRenderer, errors::CompError> {
        Ok(GLRenderer { desc: desc })
    }

    pub fn reacquire_glx_pixmap(
        &self,
        win: &mut win::Win,
        display: *mut glx::types::Display,
        config: *const c_void,
    ) {
        unsafe {
            if win.glx_pixmap != 0 {
                glx::DestroyGLXPixmap(display, win.glx_pixmap);
            }
            if win.texture != 0 {
                gl::DeleteTextures(1, &win.texture);
                win.texture = 0;
            }
            win.glx_pixmap = glx::CreatePixmap(
                display,
                config,
                win.pixmap as u64,
                &PIXMAP_ATTRS as *const i32,
            );
            gl::GenTextures(1, &mut win.texture);
            gl::BindTexture(gl::TEXTURE_2D, win.texture);
            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_WRAP_S,
                gl::REPEAT as gl::types::GLint,
            );
            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_WRAP_T,
                gl::REPEAT as gl::types::GLint,
            );
            // nearest, as the windows should be a 1:1 match
            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_MIN_FILTER,
                gl::NEAREST as gl::types::GLint,
            );
            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_MAG_FILTER,
                gl::NEAREST as gl::types::GLint,
            );
            // TODO: find out why using linear makes it blurry (it really shouldn't)
            // gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            // gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        }
    }
    pub fn release_glx_pixmap(&self, win: &mut win::Win, display: *mut glx::types::Display) {
        unsafe {
            glx::DestroyGLXPixmap(display, win.glx_pixmap);
            gl::DeleteTextures(1, &win.texture);
            win.texture = 0;
        }
    }

    pub fn render(
        &self,
        width: u16,
        height: u16,
        wins: &win::WinTracker,
        display: *mut glx::types::Display,
        overlay: Window,
        _conn: &impl x11rb::connection::Connection,
    ) -> Result<(), errors::CompError> {
        unsafe {
            gl::BindVertexArray(self.desc.vao);
            clear_fbo(self.desc.target.fbo);
            clear_fbo(self.desc.background.fbo);
            let (mut target, mut background) = (&self.desc.target, &self.desc.background);

            for w in wins.mapped_wins() {
                if !w.track_damage {
                    continue;
                }
                (target, background) = (background, target);
                gl::UseProgram(self.desc.screen_shader);
                gl::Uniform1i(self.desc.screen_texture_uniform_handle, 0);
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, background.texture);
                gl::BindFramebuffer(gl::FRAMEBUFFER, target.fbo);
                gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, null());

                gl::UseProgram(self.desc.win_shader);
                gl::Uniform1i(self.desc.win_texture_uniform_handle, 0);
                gl::Uniform1i(self.desc.bg_texture_uniform_handle, 1);
                gl::Uniform2f(
                    self.desc.screen_rect_uniform_handle,
                    width as f32,
                    height as f32,
                );
                self.render_win(w, display, target, background);
            }

            gl::UseProgram(self.desc.screen_shader);
            gl::Uniform1i(self.desc.screen_texture_uniform_handle, 0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, target.texture);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, null());

            glx::SwapBuffers(display, overlay as u64);
        }
        Ok(())
    }

    unsafe fn render_win(
        &self,
        w: &win::Win,
        display: *mut glx::types::Display,
        target: &FboTexture,
        background: &FboTexture,
    ) {
        gl::Uniform4f(
            self.desc.win_rect_uniform_handle,
            w.rect.x as f32,
            w.rect.y as f32,
            w.rect.width as f32,
            w.rect.height as f32,
        );
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, w.texture);
        glx::BindTexImageEXT(
            display as *mut glx::types::Display,
            w.glx_pixmap,
            glx::FRONT_EXT as i32,
            null(),
        );

        gl::ActiveTexture(gl::TEXTURE1);
        gl::BindTexture(gl::TEXTURE_2D, background.texture);
        gl::BindFramebuffer(gl::FRAMEBUFFER, target.fbo);

        gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, null());
        glx::ReleaseTexImageEXT(display, w.glx_pixmap, glx::FRONT_EXT as i32);
    }
}

unsafe fn clear_fbo(fbo: u32) {
    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
    gl::ClearColor(0.0, 0.0, 0.0, 1.0);
    gl::Clear(gl::COLOR_BUFFER_BIT);
    gl::Disable(gl::DEPTH_TEST);
}
