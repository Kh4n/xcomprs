mod gl;
mod glx;
mod xlib;

use std::ffi::CString;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent, EventMask,
    Window,
};
use x11rb::protocol::Event::*;

#[derive(Debug)]
struct Win {
    handle: Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    border_width: u16,
    override_redirect: bool,
}

impl Win {
    fn new(evt: CreateNotifyEvent) -> Win {
        Win {
            handle: evt.window,
            x: evt.x,
            y: evt.y,
            width: evt.width,
            height: evt.height,
            border_width: evt.border_width,
            override_redirect: evt.override_redirect,
        }
    }
}
pub fn main() {
    gl::load_with(|s| unsafe {
        let c_str = CString::new(s).unwrap();
        glx::GetProcAddressARB(c_str.as_ptr() as *const u8) as *const _
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
                _ => println!("Unhandled event: {:?}", e),
            },
            None => (),
        }
    }
}
