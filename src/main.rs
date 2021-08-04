mod gl;
mod glx;
mod xlib;

use std::error::Error;
use std::ffi::{c_void, CStr, CString};
use std::fmt::Debug;
use std::mem::{size_of, size_of_val};
use std::ptr::{null, null_mut};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent, EventMask,
    MapNotifyEvent, Window,
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

const CONTEXT_ATTRS: [i32; 5] = [
    glx::CONTEXT_MAJOR_VERSION_ARB as i32,
    3,
    glx::CONTEXT_MINOR_VERSION_ARB as i32,
    3,
    xlib::None as i32,
];

const NUM_FB_ATTRS: usize = 13;
#[rustfmt::skip]
const FB_ATTRS: [i32; NUM_FB_ATTRS * 2 + 1] = [
    glx::BIND_TO_TEXTURE_RGB_EXT as i32,
    true as i32,

    glx::BIND_TO_TEXTURE_TARGETS_EXT as i32,
    glx::TEXTURE_2D_EXT as i32,

    glx::Y_INVERTED_EXT as i32,
    glx::DONT_CARE as i32,

    glx::DOUBLEBUFFER as i32,
    true as i32,

    glx::DRAWABLE_TYPE as i32,
    glx::WINDOW_BIT as i32 | glx::PIXMAP_BIT as i32,

    glx::X_RENDERABLE as i32,
    true as i32,

    glx::RED_SIZE as i32,
    8,
    glx::GREEN_SIZE as i32,
    8,
    glx::BLUE_SIZE as i32,
    8,
    glx::DEPTH_SIZE as i32,
    24,
    glx::STENCIL_SIZE as i32,
    8,
    glx::BUFFER_SIZE as i32,
    32,

    glx::RENDER_TYPE as i32,
    glx::RGBA_BIT as i32,

    xlib::None as i32,
];

#[derive(Debug)]
struct Rect {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

impl Rect {
    fn new(x: i16, y: i16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug)]
struct GLRenderer {
    rect_vao: gl::types::GLuint,
    shader: gl::types::GLuint,
}

impl GLRenderer {
    fn new() -> Result<GLRenderer, Box<dyn Error>> {
        let mut ret = GLRenderer {
            rect_vao: 0,
            shader: 0,
        };
        let mut vbo: gl::types::GLuint = 0;
        let mut ebo: gl::types::GLuint = 0;

        let vs_source =
            CString::new(std::fs::read_to_string("./shaders/default_vs.glsl")?.as_bytes())?;
        let fs_source =
            CString::new(std::fs::read_to_string("./shaders/default_fs.glsl")?.as_bytes())?;
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
        }

        // TODO: maybe make this cleaner somehow
        #[rustfmt::skip]
        let verts: [f32; (2 + 2) * 4] = [
            //  x,y, u,v
            0.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 1.0, 0.0,
            0.0, 1.0, 0.0, 1.0,
            1.0, 1.0, 1.0, 1.0,
        ];
        #[rustfmt::skip]
        let indices: [u32; 3 * 2] = [
            0, 1, 2,
            2, 1, 3
        ];

        unsafe {
            gl::GenVertexArrays(1, &mut ret.rect_vao as *mut u32);
            gl::GenBuffers(1, &mut vbo as *mut u32);
            gl::GenBuffers(1, &mut ebo as *mut u32);

            gl::BindVertexArray(ret.rect_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                size_of_val(&verts) as isize,
                &verts as *const f32 as *const c_void,
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
                size_of_val(&indices) as isize,
                &indices as *const u32 as *const c_void,
                gl::STATIC_DRAW,
            );
        }
        Ok(ret)
    }

    fn initialize(&self, win: &mut Win) {
        assert!(win.vao == 0, "window vao is already set");
        win.vao = self.rect_vao;
    }

    fn map(&self, win: &mut Win, display: *mut glx::types::Display, config: *const c_void) {
        // must get pixmap when window is mapped
        // otherwise we get a bad match because the window is not viewable
        unsafe {
            win.glx_pixmap = glx::CreatePixmap(
                display,
                config,
                win.pixmap as u64,
                &PIXMAP_ATTRS as *const i32,
            );
            gl::GenTextures(1, &mut win.texture);
            gl::BindTexture(gl::TEXTURE_2D, win.texture);
            glx::BindTexImageEXT(display, win.glx_pixmap, glx::FRONT_EXT as i32, null());
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        }
    }
}

#[derive(Debug)]
struct Win {
    handle: Window,
    rect: Rect,
    border_width: u16,

    override_redirect: bool,
    mapped: bool,

    // free pixmap each time it changes (i think)
    pixmap: x11rb::protocol::xproto::Pixmap,
    glx_pixmap: glx::types::GLXPixmap,
    vao: gl::types::GLuint,
    /// the gl texture of the window backing pixmap
    texture: gl::types::GLuint,
}

impl Win {
    fn new(evt: CreateNotifyEvent, renderer: &GLRenderer) -> Result<Win, Box<dyn Error>> {
        let mut ret = Win {
            handle: evt.window,
            rect: Rect::new(evt.x, evt.y, evt.width, evt.height),
            border_width: evt.border_width,
            override_redirect: evt.override_redirect,
            mapped: false,

            pixmap: 0,
            glx_pixmap: 0,
            vao: 0,
            texture: 0,
        };
        renderer.initialize(&mut ret);
        Ok(ret)
    }

    fn map(
        &mut self,
        evt: MapNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        self.mapped = true;
        self.override_redirect = evt.override_redirect;

        self.pixmap = conn.generate_id().expect("could not gen id");
        conn.composite_name_window_pixmap(evt.window, self.pixmap)?
            .check()?;
        renderer.map(self, display, config);
        Ok(())
    }
}
pub fn main() {
    let display = unsafe { xlib::XOpenDisplay(null_mut()) };
    if display.is_null() {
        panic!("unable to open display!");
    }
    let xcb_conn_ptr = unsafe { xlib::XGetXCBConnection(display) };
    let conn = match unsafe {
        XCBConnection::from_raw_xcb_connection(xcb_conn_ptr as *mut c_void, true)
    } {
        Ok(d) => d,
        Err(e) => panic!("can't open display: {}", e),
    };
    let screen_num = unsafe { xlib::XDefaultScreen(display) } as usize;
    unsafe {
        xlib::XSetEventQueueOwner(display, xlib::XEventQueueOwner_XCBOwnsEventQueue);
    }

    let root = conn.setup().roots[screen_num].root;
    println!("root: {}", root);
    let extensions = vec!["RENDER", "Composite", "DAMAGE", "XFIXES", "SHAPE", "GLX"];
    for ext in extensions.iter() {
        match conn.extension_information(ext).unwrap() {
            Some(_) => (),
            None => panic!("missing extension '{}'", ext),
        };
    }
    let version_reply = conn
        .composite_query_version(0, 5)
        .expect("composite version not compatible")
        .reply()
        .expect("unable to get version reply");
    println!(
        "Composite V{}.{}",
        version_reply.major_version, version_reply.minor_version
    );

    let mut maj: i32 = 0;
    let mut min: i32 = 0;
    let has_glx =
        unsafe { glx::QueryVersion(display as *mut glx::types::Display, &mut maj, &mut min) };
    if has_glx == 0 {
        panic!("GLX not supported");
    }
    println!("GLX V{}.{}", maj, min);

    let mut num_configs: i32 = 0;
    let fb_configs = unsafe {
        glx::ChooseFBConfig(
            display as *mut glx::types::Display,
            screen_num as i32,
            &FB_ATTRS as *const i32,
            &mut num_configs,
        )
    };
    match (fb_configs.is_null(), num_configs) {
        (true, _) => panic!("unable to get fb_configs"),
        (_, 0) => panic!("no matching configs found"),
        (false, _) => (),
    }
    let fb_config = unsafe { *fb_configs.offset(0) };
    let mut visual_id: i32 = 0;
    unsafe {
        glx::GetFBConfigAttrib(
            display as *mut glx::types::Display,
            fb_config,
            glx::VISUAL_ID as i32,
            &mut visual_id,
        )
    };
    println!("visual_id: {}", visual_id);
    let glx_ctx = unsafe {
        glx::CreateContextAttribsARB(
            display as *mut glx::types::Display,
            fb_config as *const c_void,
            null_mut(),
            true as i32,
            &CONTEXT_ATTRS as *const i32,
        )
    };
    if glx_ctx.is_null() {
        panic!("unable to create context");
    }

    let overlay = conn
        .composite_get_overlay_window(root)
        .expect("unable to get overlay window")
        .reply()
        .expect("reply error")
        .overlay_win;
    println!("overlay: {}", overlay);
    let context_success =
        unsafe { glx::MakeCurrent(display as *mut glx::types::Display, overlay as u64, glx_ctx) };
    if context_success == 0 {
        panic!("unable to make context current");
    }

    gl::load_with(|s| unsafe {
        let c_str = CString::new(s).unwrap();
        glx::GetProcAddress(c_str.as_ptr() as *const u8) as *const _
    });
    unsafe {
        println!(
            "GL Version: {}",
            CStr::from_ptr(gl::GetString(gl::VERSION) as *const i8)
                .to_str()
                .expect("unable to parse cstring")
        );
    }
    let renderer = GLRenderer::new().expect("unable to create renderer");

    conn.composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::MANUAL)
        .unwrap()
        .check()
        .expect("failed to redirect, is another compositor running?");
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(
            EventMask::STRUCTURE_NOTIFY
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::EXPOSURE
                | EventMask::PROPERTY_CHANGE,
        ),
    )
    .unwrap()
    .check()
    .expect("unable to register event masks");
    let children = conn
        .query_tree(root)
        .unwrap()
        .reply()
        .expect("unable to query children")
        .children;
    for child in children {
        //TODO: track children early
        println!("Untracked window id: {}", child);
    }
    let mut wins: Vec<Win> = vec![];
    loop {
        let event = conn.poll_for_event().unwrap();
        match event {
            Some(e) => {
                println!("event: {:?}", e);
                match e {
                    CreateNotify(create) => {
                        wins.push(Win::new(create, &renderer).expect("could not track window"));
                    }
                    MapNotify(map) => {
                        for w in &mut wins {
                            if w.handle == map.window {
                                w.map(
                                    map,
                                    display as *mut glx::types::Display,
                                    fb_config as *const c_void,
                                    &conn,
                                    &renderer,
                                )
                                .expect("could not map window");
                            }
                        }
                    }
                    _ => println!("unhandled event!"),
                }
            }
            None => (),
        }
        unsafe {
            gl::ClearColor(0.2, 0.2, 0.1, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::UseProgram(renderer.shader);
            let texture_name = CString::new("win_texture").expect("unable to create cstring");
            gl::Uniform1i(
                gl::GetUniformLocation(renderer.shader, texture_name.as_ptr()),
                0,
            );
            gl::BindVertexArray(renderer.rect_vao);

            let uniform_name = CString::new("win_rect").expect("unable to create cstring");
            for w in &mut wins {
                gl::Uniform4f(
                    gl::GetUniformLocation(renderer.shader, uniform_name.as_ptr()),
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
            }
            glx::SwapBuffers(display as *mut glx::types::Display, overlay as u64);
        }
    }
}
