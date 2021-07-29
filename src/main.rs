use std::error::Error;

mod xlib;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::xproto::{
    Atom, ChangeWindowAttributesAux, ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent,
    EventMask, Window,
};
use x11rb::protocol::Event::*;

struct AtomsNeeded {
    opacity_atom: Atom,
    win_type_atom: Atom,
    win_desktop_atom: Atom,
    win_dock_atom: Atom,
    win_toolbar_atom: Atom,
    win_menu_atom: Atom,
    win_util_atom: Atom,
    win_splash_atom: Atom,
    win_dialog_atom: Atom,
    win_normal_atom: Atom,
}

impl AtomsNeeded {
    fn new(conn: &impl Connection) -> Result<AtomsNeeded, Box<dyn Error>> {
        return Ok(AtomsNeeded {
            opacity_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_OPACITY")?
                .reply()?
                .atom,
            win_type_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE")?
                .reply()?
                .atom,
            win_desktop_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP")?
                .reply()?
                .atom,
            win_dock_atom: conn.intern_atom(false, b"_NET_WM_TYPE_DOCK")?.reply()?.atom,
            win_toolbar_atom: conn
                .intern_atom(false, b"_NET_WM_TYPE_TOOLBAR")?
                .reply()?
                .atom,
            win_menu_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_MENU")?
                .reply()?
                .atom,
            win_util_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY")?
                .reply()?
                .atom,
            win_splash_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_SPLASH")?
                .reply()?
                .atom,
            win_dialog_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_DIALOG")?
                .reply()?
                .atom,
            win_normal_atom: conn
                .intern_atom(false, b"_NET_WM_WINDOW_TYPE_NORMAL")?
                .reply()?
                .atom,
        });
    }
}

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
