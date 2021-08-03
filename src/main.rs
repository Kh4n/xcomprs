mod gl;
mod glx;
mod xlib;

use std::error::Error;
use std::ffi::{c_void, CString};
use std::fmt::Debug;
use std::mem::{size_of, size_of_val};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent, EventMask,
    Window,
};
use x11rb::protocol::Event::*;

const PIXMAP_ATTRIBS: [i32; 5] = [
    glx::TEXTURE_TARGET_EXT as i32,
    glx::TEXTURE_2D_EXT as i32,
    glx::TEXTURE_FORMAT_EXT as i32,
    glx::TEXTURE_FORMAT_RGBA_EXT as i32,
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
struct Win {
    handle: Window,
    rect: Rect,
    border_width: u16,

    override_redirect: bool,
    mapped: bool,

    // free pixmap each time it changes (i think)
    pixmap: x11rb::protocol::xproto::Pixmap,
    vao: gl::types::GLuint,
    texture: gl::types::GLuint,
}

#[derive(Debug)]
struct GLRenderer {
    rect_vao: gl::types::GLuint,
}

impl GLRenderer {
    fn new() -> GLRenderer {
        let mut ret = GLRenderer { rect_vao: 0 };
        let mut vbo: gl::types::GLuint = 0;
        let mut ebo: gl::types::GLuint = 0;

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

        return ret;
    }
}

impl Win {
    fn new(
        display: *mut glx::types::Display,
        config: *const c_void,
        evt: CreateNotifyEvent,
        conn: &impl x11rb::connection::Connection,
        render: &GLRenderer,
    ) -> Result<Win, Box<dyn Error>> {
        let mut ret = Win {
            handle: evt.window,
            rect: Rect::new(evt.x, evt.y, evt.width, evt.height),
            border_width: evt.border_width,
            override_redirect: evt.override_redirect,
            mapped: false,

            pixmap: 0,
            vao: render.rect_vao,
            texture: 0,
        };

        ret.pixmap = conn.generate_id()?;
        conn.composite_name_window_pixmap(evt.window, ret.pixmap)?
            .check()?;
        let glx_pixmap = unsafe {
            glx::CreatePixmap(
                display,
                config,
                ret.pixmap as u64,
                &PIXMAP_ATTRIBS as *const i32,
            )
        };

        Ok(ret)
    }
}
pub fn main() {
    gl::load_with(|s| unsafe {
        let c_str = CString::new(s).unwrap();
        glx::GetProcAddress(c_str.as_ptr() as *const u8) as *const _
    });

    let (conn, screen_num) = x11rb::connect(None).expect("Can't connect to x server: ");
    let root = conn.setup().roots[screen_num].root;
    let extensions = vec!["RENDER", "Composite", "DAMAGE", "XFIXES", "SHAPE", "GLX"];
    for ext in extensions.iter() {
        match conn.extension_information(ext).unwrap() {
            Some(_) => (),
            None => panic!("Missing extension '{}'", ext),
        };
    }
    let version_reply = conn
        .composite_query_version(0, 5)
        .expect("Composite version not compatible")
        .reply()
        .unwrap();
    println!(
        "Composite V{}.{}",
        version_reply.major_version, version_reply.minor_version
    );
    match conn
        .composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::MANUAL)
        .unwrap()
        .check()
    {
        Ok(_) => (),
        Err(e) => panic!("Failed to redirect, is another compositor running?: {}", e),
    };
    match conn
        .change_window_attributes(
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
    {
        Ok(_) => (),
        Err(e) => panic!("Unable to register event masks: {}", e),
    };
    let children = match conn.query_tree(root).unwrap().reply() {
        Ok(c) => c.children,
        Err(e) => panic!("Unable to query children: {}", e),
    };
    for child in children {
        //TODO: track children early?
        println!("Window id: {}", child);
    }
    let mut wins: Vec<Win> = vec![];
    loop {
        let event = conn.poll_for_event().unwrap();
        match event {
            Some(e) => match e {
                CreateNotify(create) => {
                    wins.push(Win::new(create));
                    println!("hello");
                }
                MapNotify(map) => {
                    for w in &mut wins {
                        if w.handle == map.window {
                            w.mapped = true;
                            w.override_redirect = map.override_redirect;
                        }
                    }
                }
                _ => println!("Unhandled event: {:?}", e),
            },
            None => (),
        }
    }
}
