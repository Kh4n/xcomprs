use crate::errors;
use crate::ewm;
use crate::ewm::RootWindowHintCodes;
use crate::gl;
use crate::gl_renderer;
use crate::glx;

use std::convert::TryFrom;
use std::error::Error;
use std::ffi::{c_void, CStr, CString};
use std::fmt::Debug;
use std::mem::{size_of, size_of_val};
use std::ptr::{null, null_mut};

use byteorder::ByteOrder;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::ConnectionExt as composite_ConnectionExt;
use x11rb::protocol::damage::ConnectionExt as damage_ConnectionExt;
use x11rb::protocol::damage::Damage;
use x11rb::protocol::damage::ReportLevel;
use x11rb::protocol::shape::{ConnectionExt as shape_ConnectionExt, SK, SO};
use x11rb::protocol::xfixes::{ConnectionExt, Region};
use x11rb::protocol::xproto::Atom;
use x11rb::protocol::xproto::AtomEnum;
use x11rb::protocol::xproto::WindowClass;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ConfigureNotifyEvent,
    ConnectionExt as xproto_ConnectionExt, CreateNotifyEvent, DestroyNotifyEvent, EventMask,
    MapNotifyEvent, MapState, Pixmap, Rectangle, UnmapNotifyEvent, Window,
};
use x11rb::protocol::Event;
use x11rb::protocol::Event::*;
use x11rb::rust_connection::ConnectionError;
use x11rb::xcb_ffi::XCBConnection;

#[derive(Debug)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug)]
pub struct Win {
    pub rect: Rect,

    handle: Window,
    pub damage: Damage,

    border_width: u16,
    override_redirect: bool,
    mapped: bool,

    // free pixmap each time it changes (i think)
    pub pixmap: x11rb::protocol::xproto::Pixmap,

    // TODO: decouple this more
    pub glx_pixmap: glx::types::GLXPixmap,
    pub vao: gl::types::GLuint,
    /// the gl texture of the window backing pixmap
    pub texture: gl::types::GLuint,
    // renderer: &'a gl_renderer::GLRenderer,
}

impl Win {
    pub fn new_raw(
        handle: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        override_redirect: bool,
        class: WindowClass,
        mapped: bool,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
        track_damage: bool,
    ) -> Result<Win, errors::CompError> {
        let mut ret = Win {
            handle: handle,
            damage: 0,
            rect: Rect::new(x, y, width, height),
            border_width: border_width,
            override_redirect: override_redirect,
            mapped: mapped,

            pixmap: 0,
            glx_pixmap: 0,
            vao: 0,
            texture: 0,
        };
        renderer.initialize(&mut ret);

        if class != WindowClass::INPUT_ONLY && track_damage {
            ret.damage = conn.generate_id()?;
            // conn.damage_create(self.damage, self.handle, ReportLevel::RAW_RECTANGLES)?
            // conn.damage_create(ret.damage, ret.handle, ReportLevel::DELTA_RECTANGLES)?
            conn.damage_create(ret.damage, ret.handle, ReportLevel::NON_EMPTY)?
                .check()?;

            conn.change_window_attributes(
                handle,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::EXPOSURE),
            )?
            .check()?;
        }

        Ok(ret)
    }
    pub fn new_handle(
        handle: Window,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
        track_damage: bool,
    ) -> Result<Win, errors::CompError> {
        let geom = conn
            .get_geometry(handle)
            .expect("could not connect to server")
            .reply()
            .expect("could not get geometry");
        let attrs = conn
            .get_window_attributes(handle)
            .expect("could not connect to server")
            .reply()
            .expect("could not get window attributes");
        let mapped = match attrs.map_state {
            MapState::UNMAPPED => false,
            MapState::UNVIEWABLE | MapState::VIEWABLE => true,
            _ => panic!("invalid map state"),
        };
        Win::new_raw(
            handle,
            geom.x,
            geom.y,
            geom.width,
            geom.height,
            geom.border_width,
            attrs.override_redirect,
            attrs.class,
            mapped,
            conn,
            renderer,
            track_damage,
        )
    }
    pub fn new_event(
        evt: &CreateNotifyEvent,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
        track_damage: bool,
    ) -> Result<Win, errors::CompError> {
        let attrs = conn
            .get_window_attributes(evt.window)
            .expect("could not connect to server")
            .reply()
            .expect("could not get window attributes");

        Win::new_raw(
            evt.window,
            evt.x,
            evt.y,
            evt.width,
            evt.height,
            evt.border_width,
            evt.override_redirect,
            attrs.class,
            false,
            conn,
            renderer,
            track_damage,
        )
    }

    pub fn map(
        &mut self,
        evt: &MapNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        self.mapped = true;
        self.override_redirect = evt.override_redirect;
        self.reacquire_pixmap(evt.window, display, config, conn, renderer)?;
        Ok(())
    }
    pub fn unmap(
        &mut self,
        evt: &UnmapNotifyEvent,
        conn: &impl x11rb::connection::Connection,
    ) -> Result<(), errors::CompError> {
        self.mapped = false;
        Ok(())
    }

    pub fn destroy(
        &mut self,
        evt: &DestroyNotifyEvent,
        display: *mut glx::types::Display,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        if self.damage != 0 {
            // apparently when destroying damage you can get a BadDamage. idk why. both picom and xcompmgr ignore the error :/
            conn.damage_destroy(self.damage)?.ignore_error();
        }
        self.release_pixmap(display, renderer)?;
        Ok(())
    }

    pub fn reacquire_pixmap(
        &mut self,
        window: Window,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        // if we're not mapped, no pixmap to reacquire
        if !self.mapped {
            return Ok(());
        }
        self.pixmap = conn.generate_id().expect("could not gen id");
        conn.composite_name_window_pixmap(window, self.pixmap)?
            .check()?;
        renderer.reacquire_glx_pixmap(self, display, config);
        Ok(())
    }
    pub fn release_pixmap(
        &mut self,
        display: *mut glx::types::Display,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        // if not mapped, we already released the pixmaps/textures
        if !self.mapped {
            return Ok(());
        }
        renderer.release_glx_pixmap(self, display);
        Ok(())
    }
}
impl Drop for Win {
    fn drop(&mut self) {}
}

#[derive(Debug)]
pub struct WinTracker {
    root: Window,
    overlay: Window,
    wins: Vec<Win>,

    region: Region,
}

impl WinTracker {
    pub fn new(
        root: Window,
        overlay: Window,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<WinTracker, errors::CompError> {
        let mut ret = WinTracker {
            root: root,
            overlay: overlay,
            wins: vec![Win::new_handle(root, conn, renderer, false)?],

            region: conn.generate_id()?,
        };

        // reusable empty region for damage fetch requests
        conn.xfixes_create_region(ret.region, &[])?.check()?;

        let children = conn.query_tree(root)?.reply()?.children;
        for child in children {
            // don't want to track overlay damage as we will be spammed with events (they fire every frame for the whole overlay)
            ret.wins
                .push(Win::new_handle(child, conn, renderer, child != overlay)?);
        }
        Ok(ret)
    }

    pub fn process_and_render(
        &mut self,
        event: &Option<Event>,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        width: u16,
        height: u16,
        overlay: Window,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        match event {
            Some(e) => {
                println!("event: {:?}, num wins: {}", e, self.wins.len());
                match e {
                    CreateNotify(create) => {
                        self.wins
                            .push(Win::new_event(&create, conn, renderer, true)?);
                    }
                    MapNotify(map) => {
                        let w = self
                            .wins
                            .iter_mut()
                            .find(|w| w.handle == map.window)
                            .ok_or("map notified with untracked window!".to_string())?;
                        w.map(&map, display, config, conn, renderer)?;
                    }
                    ConfigureNotify(conf) => {
                        let w = self
                            .wins
                            .iter()
                            .position(|w| w.handle == conf.window)
                            .ok_or("configure notified with untracked window!".to_string())?;
                        self.configure(w, &conf, display, config, conn, renderer)?;
                    }
                    UnmapNotify(unmap) => {
                        let w = self
                            .wins
                            .iter_mut()
                            .find(|w| w.handle == unmap.window)
                            .ok_or("unmap notified with untracked window!".to_string())?;
                        w.unmap(&unmap, conn)?;
                    }
                    DestroyNotify(destroy) => {
                        self.wins
                            .remove(
                                self.wins
                                    .iter()
                                    .position(|w| w.handle == destroy.window)
                                    .ok_or("destroy notify for untracked window".to_string())?,
                            )
                            .destroy(&destroy, display, conn, renderer)?;
                    }
                    PropertyNotify(prop) => match RootWindowHintCodes::try_from(prop.atom) {
                        Ok(RootWindowHintCodes::_NET_ACTIVE_WINDOW) => {
                            if prop.window != self.get_composite_win().handle {
                                Err("root window atom's target was not root window".to_string())?;
                            }
                        }
                        Ok(RootWindowHintCodes::_NET_CLIENT_LIST_STACKING) => {
                            if prop.window != self.get_composite_win().handle {
                                Err("root window atom's target was not root window".to_string())?;
                            }
                            let res = conn
                                .get_property(
                                    false,
                                    self.get_composite_win().handle,
                                    RootWindowHintCodes::_NET_CLIENT_LIST_STACKING as u32,
                                    AtomEnum::ANY,
                                    0,
                                    self.wins.len() as u32,
                                )?
                                .reply()?;
                            for j in (4..=res.value.len()).step_by(4) {
                                let w_id = byteorder::LittleEndian::read_u32(
                                    &res.value.as_slice()[j - 4..j],
                                );
                                print!("id: {} ", w_id);
                            }
                            println!("stacking info: {:?}", res);
                        }
                        _ => println!(
                            "unhandled atom #: {}, name: {}",
                            prop.atom,
                            std::str::from_utf8(
                                conn.get_atom_name(prop.atom)?.reply()?.name.as_slice()
                            )?
                        ),
                    },
                    DamageNotify(damage) => {
                        // sometimes damage is sent after things are cleaned up (i think?) so this throws an error
                        // same thing in xcompmgr and picom src :/
                        if conn
                            .damage_subtract(damage.damage, 0 as u32, self.region)?
                            .check()
                            .is_ok()
                        {
                            let fetch = conn.xfixes_fetch_region(self.region)?.reply()?;
                            println!("fetch: {:?}", fetch);
                        }
                    }
                    _ => println!("unhandled event!"),
                }
            }
            None => (),
        }
        renderer.render(width, height, self, display, overlay, conn)?;
        Ok(())
    }

    // TODO: handle all configure notify possibilities (stacking order, etc.)
    pub fn configure(
        &mut self,
        win_pos: usize,
        evt: &ConfigureNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), errors::CompError> {
        let win = &mut self.wins[win_pos];
        win.rect.x = evt.x;
        win.rect.y = evt.y;
        if win.rect.width != evt.width || win.rect.height != evt.height {
            win.reacquire_pixmap(evt.window, display, config, conn, renderer)?;
            win.rect.width = evt.width;
            win.rect.height = evt.height;
        }

        if (win_pos == 0 && evt.above_sibling != 0)
            || (win_pos > 0 && self.wins[win_pos - 1].handle != evt.above_sibling)
        {
            let target = self.wins.remove(win_pos);
            let target_pos = match evt.above_sibling {
                0 => 0,
                _ => self
                    .wins
                    .iter()
                    .position(|w| w.handle == evt.above_sibling)
                    .ok_or("above sibling window not found".to_string())?,
            };
            if target_pos == self.wins.len() - 1 {
                self.wins.push(target);
            } else {
                self.wins.insert(target_pos + 1, target);
            }
        }

        Ok(())
    }

    pub fn get_composite_win(&self) -> &Win {
        &self.wins[0]
    }

    pub fn mapped_wins(&self) -> impl Iterator<Item = &Win> {
        self.wins.iter().filter(|w| w.mapped).into_iter()
    }
}
