mod errors;
mod ewm;
mod gl;
mod gl_renderer;
mod glx;
mod win;
mod xlib;


use std::ffi::{c_void, CStr, CString};


use std::ptr::{null_mut};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::damage::ConnectionExt as damage_ConnectionExt;
use x11rb::protocol::shape::{ConnectionExt as shape_ConnectionExt, SK};
use x11rb::protocol::xfixes::{ConnectionExt as xfixes_ConnectionExt, Region};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux,
    ConnectionExt as xproto_ConnectionExt, EventMask, Rectangle,
};

use x11rb::xcb_ffi::XCBConnection;

use crate::errors::CompError;

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
    let xdamage_ver_reply = conn
        .damage_query_version(1, 1)
        .expect("could not connect to server")
        .reply()
        .expect("could not query xdamage version");
    println!(
        "Xdamage V{}.{}",
        xdamage_ver_reply.major_version, xdamage_ver_reply.minor_version
    );
    let xshape_ver_reply = conn
        .shape_query_version()
        .expect("could not connect to server")
        .reply()
        .expect("could not query xshape version");
    println!(
        "Xshape V{}.{}",
        xshape_ver_reply.major_version, xshape_ver_reply.minor_version
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

    let region = conn.generate_id().expect("could not generate id");
    conn.xfixes_create_region(
        region,
        &[Rectangle {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }],
    )
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

    #[rustfmt::skip]
    let desc = gl_renderer::WindowDrawDesc::new_shader_paths(
        &vec![
            // x,y, u,v
            0.0,  0.0, 0.0, 1.0,
            0.0, -1.0, 0.0, 0.0,
            1.0,  0.0, 1.0, 1.0,
            1.0, -1.0, 1.0, 0.0,
        ],
        &vec![
            0, 1, 2,
            2, 1, 3
        ],
        "./shaders/default_vs.glsl",
        "./shaders/default_fs.glsl",
    ).expect("could not create window draw description");
    let renderer = gl_renderer::GLRenderer::new(desc).expect("unable to create renderer");

    conn.composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::MANUAL)
        .expect("could not connect to server")
        .check()
        .expect("failed to redirect, is another compositor running?");
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(
            EventMask::STRUCTURE_NOTIFY
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::PROPERTY_CHANGE,
        ),
    )
    .expect("could not connect to server")
    .check()
    .expect("unable to register event masks");

    let mut tracker = win::WinTracker::new(root, overlay, &conn, &renderer)
        .expect("could not create window tracker");
    loop {
        let event = conn.poll_for_event().unwrap();
        match tracker.process_and_render(
            &event,
            display as *mut glx::types::Display,
            fb_config as *mut c_void,
            &conn,
            width,
            height,
            overlay,
            &renderer,
        ) {
            Err(e) => {
                match &e {
                    CompError::Reply(r) => match r {
                        x11rb::rust_connection::ReplyError::X11Error(_x_err) => {
                            println!("error info (TODO: xcb version of XgetError)");
                        }
                        _ => {}
                    },
                    _ => {}
                }
                panic!("could not process and render: {:?}", e);
            }
            _ => {}
        };
    }
}
