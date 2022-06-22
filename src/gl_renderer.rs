use crate::errors;
use crate::gl;
use crate::glx;
use crate::win;
use crate::xlib;

use std::error::Error;
use std::ffi::{c_void, CStr, CString};
use std::fmt::Debug;
use std::mem::{size_of, size_of_val};
use std::ptr::{null, null_mut};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::damage::ConnectionExt as damage_ConnectionExt;
use x11rb::protocol::shape::{ConnectionExt as shape_CompositeExt, SK, SO};
use x11rb::protocol::xfixes::{ConnectionExt, Region};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ConfigureNotifyEvent,
    ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent, DestroyNotifyEvent, EventMask,
    MapNotifyEvent, MapState, Pixmap, Rectangle, UnmapNotifyEvent, Window,
};
use x11rb::protocol::Event::*;
use x11rb::xcb_ffi::XCBConnection;

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

#[derive(Debug)]
pub struct WindowDrawDesc {
    vao: gl::types::GLuint,
    shader: gl::types::GLuint,

    win_rect_name: CString,
    screen_rect_name: CString,
    win_texture_name: CString,
}

impl WindowDrawDesc {
    pub fn new_shader_paths(
        verts: &Vec<f32>,
        indices: &Vec<u32>,
        vs_path: &str,
        fs_path: &str,
    ) -> Result<WindowDrawDesc, errors::CompError> {
        WindowDrawDesc::new(
            verts,
            indices,
            &std::fs::read_to_string(vs_path)?,
            &std::fs::read_to_string(fs_path)?,
        )
    }
    pub fn new(
        verts: &Vec<f32>,
        indices: &Vec<u32>,
        vs_source: &String,
        fs_source: &String,
    ) -> Result<WindowDrawDesc, errors::CompError> {
        let mut ret = WindowDrawDesc {
            vao: 0,
            shader: 0,
            win_rect_name: CString::new(WIN_RECT_UNIFORM_NAME)?,
            screen_rect_name: CString::new(SCREEN_RECT_UNIFORM_NAME)?,
            win_texture_name: CString::new(WIN_TEXTURE_UNIFORM_NAME)?,
        };

        let mut vbo: gl::types::GLuint = 0;
        let mut ebo: gl::types::GLuint = 0;

        let vs_source = CString::new(vs_source.as_bytes())?;
        let fs_source = CString::new(fs_source.as_bytes())?;
        unsafe {
            let vertex_shader = gl::CreateShader(gl::VERTEX_SHADER);
            gl::ShaderSource(vertex_shader, 1, &vs_source.as_ptr(), null());
            gl::CompileShader(vertex_shader);
            let mut success: i32 = 0;
            let mut log: [u8; 512] = [0; 512];
            gl::GetShaderiv(vertex_shader, gl::COMPILE_STATUS, &mut success);
            if success == gl::FALSE as i32 {
                gl::GetShaderInfoLog(
                    vertex_shader,
                    512,
                    null_mut(),
                    &mut log as *mut _ as *mut i8,
                );
                Err(format!(
                    "unable to compile shader: {:?}",
                    String::from_utf8_lossy(&log)
                ))?;
            }
            let frag_shader = gl::CreateShader(gl::FRAGMENT_SHADER);
            gl::ShaderSource(frag_shader, 1, &fs_source.as_ptr(), null());
            gl::CompileShader(frag_shader);
            gl::GetShaderiv(frag_shader, gl::COMPILE_STATUS, &mut success);
            if success == gl::FALSE as i32 {
                gl::GetShaderInfoLog(frag_shader, 512, null_mut(), &mut log as *mut _ as *mut i8);
                Err(format!(
                    "unable to compile shader: {:?}",
                    String::from_utf8_lossy(&log)
                ))?;
            }

            ret.shader = gl::CreateProgram();
            gl::AttachShader(ret.shader, vertex_shader);
            gl::AttachShader(ret.shader, frag_shader);
            gl::LinkProgram(ret.shader);
            gl::GetProgramiv(ret.shader, gl::LINK_STATUS, &mut success);
            if success == gl::FALSE as i32 {
                gl::GetShaderInfoLog(ret.shader, 512, null_mut(), &mut log as *mut _ as *mut i8);
                Err(format!(
                    "unable to link shader: {:?}",
                    String::from_utf8_lossy(&log)
                ))?;
            }
            gl::DetachShader(ret.shader, vertex_shader);
            gl::DetachShader(ret.shader, frag_shader);
            gl::DeleteShader(vertex_shader);
            gl::DeleteShader(frag_shader);

            // test if shader has uniforms we need
            // gl::UseProgram(ret.shader);
            let (w, s, t) = (
                gl::GetUniformLocation(ret.shader, ret.win_rect_name.as_ptr()),
                gl::GetUniformLocation(ret.shader, ret.screen_rect_name.as_ptr()),
                gl::GetUniformLocation(ret.shader, ret.win_texture_name.as_ptr()),
            );
            if w < 0 {
                Err(format!(
                    "the shader does not define '{}'",
                    WIN_RECT_UNIFORM_NAME
                ))?
            }
            if s < 0 {
                Err(format!(
                    "the shader does not define '{}'",
                    SCREEN_RECT_UNIFORM_NAME
                ))?
            }
            if t < 0 {
                Err(format!(
                    "the shader does not define '{}'",
                    WIN_TEXTURE_UNIFORM_NAME
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
            gl::GenVertexArrays(1, &mut ret.vao as *mut u32);
            gl::GenBuffers(1, &mut vbo as *mut u32);
            gl::GenBuffers(1, &mut ebo as *mut u32);

            gl::BindVertexArray(ret.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (verts.len() * size_of::<f32>()) as isize,
                verts.as_ptr() as *const c_void,
                gl::STATIC_DRAW,
            );
            // position
            gl::VertexAttribPointer(
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * size_of::<f32>()) as i32,
                0 as *const c_void,
            );
            gl::EnableVertexAttribArray(0);
            // texture coords
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * size_of::<f32>()) as i32,
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

        Ok(ret)
    }
}

#[derive(Debug)]
pub struct GLRenderer {
    // TODO: allow different descs for different windows
    desc: WindowDrawDesc,
}

// TODO: draw borders
impl GLRenderer {
    pub fn new(desc: WindowDrawDesc) -> Result<GLRenderer, errors::CompError> {
        Ok(GLRenderer { desc: desc })
    }

    pub fn initialize(&self, win: &mut win::Win) {
        assert!(win.vao == 0, "window vao is already set");
        win.vao = self.desc.vao;
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
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
            // nearest, as the windows should be a 1:1 match
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
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
        conn: &impl x11rb::connection::Connection,
    ) -> Result<(), errors::CompError> {
        unsafe {
            // no need to clear (well you do, but not if you want to use xdamage etc)
            // gl::ClearColor(0.2, 0.2, 0.1, 1.0);
            // gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::UseProgram(self.desc.shader);
            gl::Uniform1i(
                gl::GetUniformLocation(self.desc.shader, self.desc.win_texture_name.as_ptr()),
                0,
            );
            gl::BindVertexArray(self.desc.vao);
            gl::Uniform2f(
                gl::GetUniformLocation(self.desc.shader, self.desc.screen_rect_name.as_ptr()),
                width as f32,
                height as f32,
            );
            for w in wins.mapped_wins() {
                self.render_win(w, display);
            }
            glx::SwapBuffers(display, overlay as u64);
        }
        Ok(())
    }

    unsafe fn render_win(&self, w: &win::Win, display: *mut glx::types::Display) {
        gl::Uniform4f(
            gl::GetUniformLocation(self.desc.shader, self.desc.win_rect_name.as_ptr()),
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
        gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, null());
        glx::ReleaseTexImageEXT(display, w.glx_pixmap, glx::FRONT_EXT as i32);
        // conn.xfixes_set_region(
        //     w.region,
        //     &[Rectangle {
        //         x: 0,
        //         y: 0,
        //         width: w.rect.width,
        //         height: w.rect.height,
        //     }],
        // )?
        // .check()?;
    }
}
