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

        // TODO: maybe make this cleaner somehow (standardized somehow as well)
        #[rustfmt::skip]
        let verts: [f32; (2 + 2) * 4] = [
            //  x,y, u,v
             0.0,  0.0,  0.0,  1.0,
             0.0, -1.0,  0.0,  0.0,
             1.0,  0.0,  1.0,  1.0,
             1.0, -1.0,  1.0,  0.0,
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

    fn reacquire_glx_pixmap(
        &self,
        win: &mut Win,
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
    fn release_glx_pixmap(&self, win: &mut Win, display: *mut glx::types::Display) {
        unsafe {
            glx::DestroyGLXPixmap(display, win.glx_pixmap);
            gl::DeleteTextures(1, &win.texture);
            win.texture = 0;
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

    // TODO: decouple this more
    glx_pixmap: glx::types::GLXPixmap,
    vao: gl::types::GLuint,
    /// the gl texture of the window backing pixmap
    texture: gl::types::GLuint,
    // renderer: &'a GLRenderer,
}

impl Win {
    fn new_raw(
        handle: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        override_redirect: bool,
        mapped: bool,
        renderer: &GLRenderer,
    ) -> Result<Win, Box<dyn Error>> {
        let mut ret = Win {
            handle: handle,
            rect: Rect::new(x, y, width, height),
            border_width: border_width,
            override_redirect: override_redirect,
            mapped: mapped,

            pixmap: 0,
            glx_pixmap: 0,
            vao: 0,
            texture: 0,
            // renderer: renderer,
        };
        renderer.initialize(&mut ret);
        Ok(ret)
    }
    fn new(evt: &CreateNotifyEvent, renderer: &GLRenderer) -> Result<Win, Box<dyn Error>> {
        Win::new_raw(
            evt.window,
            evt.x,
            evt.y,
            evt.width,
            evt.height,
            evt.border_width,
            evt.override_redirect,
            false,
            &renderer,
        )
    }

    fn map(
        &mut self,
        evt: &MapNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        self.mapped = true;
        self.override_redirect = evt.override_redirect;
        self.reacquire_pixmap(evt.window, display, config, conn, renderer)?;
        Ok(())
    }
    fn unmap(&mut self, evt: &UnmapNotifyEvent) -> Result<(), Box<dyn Error>> {
        self.mapped = false;
        Ok(())
    }

    fn destroy(
        &mut self,
        evt: &DestroyNotifyEvent,
        display: *mut glx::types::Display,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        self.release_pixmap(display, renderer)?;
        Ok(())
    }

    fn reacquire_pixmap(
        &mut self,
        window: Window,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        if !self.mapped {
            // if we're not mapped, no pixmap to reacquire
            return Ok(());
        }
        self.pixmap = conn.generate_id().expect("could not gen id");
        conn.composite_name_window_pixmap(window, self.pixmap)?
            .check()?;
        renderer.reacquire_glx_pixmap(self, display, config);
        Ok(())
    }
    fn release_pixmap(
        &mut self,
        display: *mut glx::types::Display,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        if !self.mapped {
            // if not mapped, we already released the pixmaps/textures
            return Ok(());
        }
        renderer.release_glx_pixmap(self, display);
        Ok(())
    }

    // TODO: handle all configure notify possibilities (stacking order, etc.)
    fn configure(
        &mut self,
        evt: &ConfigureNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        self.rect.x = evt.x;
        self.rect.y = evt.y;
        if self.rect.width != evt.width || self.rect.height != evt.height {
            self.reacquire_pixmap(evt.window, display, config, conn, renderer)?;
            self.rect.width = evt.width;
            self.rect.height = evt.height;
        }
        Ok(())
    }
}
impl Drop for Win {
    fn drop(&mut self) {}
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
    let width = conn.setup().roots[screen_num].width_in_pixels;
    let height = conn.setup().roots[screen_num].height_in_pixels;
    println!("root: {}", root);
    println!("width, height: {}, {}", width, height);
    let extensions = vec!["RENDER", "Composite", "DAMAGE", "XFIXES", "SHAPE", "GLX"];
    for ext in extensions.iter() {
        match conn.extension_information(ext).unwrap() {
            Some(_) => (),
            None => panic!("missing extension '{}'", ext),
        };
    }
    // MUST QUERY VERSIONS BEFORE USE!!
    let version_reply = conn
        .composite_query_version(0, 5)
        .expect("could not connect to server")
        .reply()
        .expect("composite version not compatible");
    println!(
        "Composite V{}.{}",
        version_reply.major_version, version_reply.minor_version
    );
    let xfixes_ver_reply = conn
        .xfixes_query_version(5, 0)
        .expect("could not connect to server")
        .reply()
        .expect("could not query xfixes version");
    println!(
        "XFixes V{}.{}",
        xfixes_ver_reply.major_version, xfixes_ver_reply.minor_version
    );

    let mut maj: i32 = 0;
    let mut min: i32 = 0;
    let has_glx =
        unsafe { glx::QueryVersion(display as *mut glx::types::Display, &mut maj, &mut min) };
    if has_glx == gl::FALSE as i32 {
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
        .expect("could not connect to server")
        .reply()
        .expect("unable to get overlay window")
        .overlay_win;
    println!("overlay: {}", overlay);

    // currently not using this method to let input pass through overlay
    // it works, but I have no clue how
    // TODO: figure out wtf these functions do?
    // conn.shape_mask(SO::SET, SK::BOUNDING, overlay, 0, 0, 0 as Pixmap)
    //     .expect("unable to mask shape (?)")
    //     .check()
    //     .expect("error checking reply");
    // conn.shape_rectangles(
    //     SO::SET,
    //     SK::INPUT,
    //     ClipOrdering::UNSORTED,
    //     overlay,
    //     0,
    //     0,
    //     &[],
    // )
    // .expect("unable to set shape rectangles (?)")
    // .check()
    // .expect("error getting reply");

    let mut rect = [Rectangle {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }];
    let region = conn.generate_id().expect("could not generate id");
    conn.xfixes_create_region(region, &mut rect)
        .expect("could not connect to server")
        .check()
        .expect("could not create region");
    conn.xfixes_set_window_shape_region(overlay, SK::INPUT, 0, 0, region as Region)
        .expect("could not connect to server")
        .check()
        .expect("unable to set overlay shape region");

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
        .expect("could not connect to server")
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
    .expect("could not connect to server")
    .check()
    .expect("unable to register event masks");
    let children = conn
        .query_tree(root)
        .expect("could not connect to server")
        .reply()
        .expect("unable to query children")
        .children;
    for child in children {
        //TODO: track children early
        println!("Untracked window id: {}", child);
    }
    let mut wins: Vec<Win> = vec![];
    let root_geom = conn
        .get_geometry(root)
        .expect("could not connect to server")
        .reply()
        .expect("could not get geometry");
    let root_attrs = conn
        .get_window_attributes(root)
        .expect("could not connect to server")
        .reply()
        .expect("could not get root window attributes");
    let root_mapped = match root_attrs.map_state {
        MapState::UNMAPPED => false,
        MapState::UNVIEWABLE | MapState::VIEWABLE => true,
        _ => panic!("invalid map state"),
    };
    wins.push(
        Win::new_raw(
            root,
            root_geom.x,
            root_geom.y,
            root_geom.width,
            root_geom.height,
            root_geom.border_width,
            root_attrs.override_redirect,
            root_mapped,
            &renderer,
        )
        .expect("could not construct root window"),
    );
    loop {
        let event = conn.poll_for_event().unwrap();
        match event {
            Some(e) => {
                println!("event: {:?}", e);
                match e {
                    CreateNotify(create) => {
                        wins.push(Win::new(&create, &renderer).expect("could not track window"));
                    }
                    MapNotify(map) => {
                        let w = wins
                            .iter_mut()
                            .find(|w| w.handle == map.window)
                            .expect("map notified with untracked window!");
                        w.map(
                            &map,
                            display as *mut glx::types::Display,
                            fb_config as *const c_void,
                            &conn,
                            &renderer,
                        )
                        .expect("could not map window");
                    }
                    ConfigureNotify(conf) => {
                        let w = wins
                            .iter_mut()
                            .find(|w| w.handle == conf.window)
                            .expect("configure notified with untracked window!");
                        w.configure(
                            &conf,
                            display as *mut glx::types::Display,
                            fb_config,
                            &conn,
                            &renderer,
                        )
                        .expect("could not configure window");
                    }
                    UnmapNotify(unmap) => {
                        let w = wins
                            .iter_mut()
                            .find(|w| w.handle == unmap.window)
                            .expect("unmap notified with untracked window!");
                        w.unmap(&unmap).expect("could not unmap window");
                    }
                    DestroyNotify(destroy) => {
                        wins.remove(
                            wins.iter()
                                .position(|w| w.handle == destroy.window)
                                .expect("destroy notify for untracked window"),
                        )
                        .destroy(&destroy, display as *mut glx::types::Display, &renderer)
                        .expect("could not destroy window");
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

            let screen_rect_name = CString::new("screen_rect").expect("unable to create cstring");
            gl::Uniform2f(
                gl::GetUniformLocation(renderer.shader, screen_rect_name.as_ptr()),
                width as f32,
                height as f32,
            );
            let win_rect_name = CString::new("win_rect").expect("unable to create cstring");
            for w in &mut wins {
                if !w.mapped {
                    continue;
                }
                gl::Uniform4f(
                    gl::GetUniformLocation(renderer.shader, win_rect_name.as_ptr()),
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
                glx::ReleaseTexImageEXT(
                    display as *mut glx::types::Display,
                    w.glx_pixmap,
                    glx::FRONT_EXT as i32,
                );
            }
            glx::SwapBuffers(display as *mut glx::types::Display, overlay as u64);
        }
    }
}
