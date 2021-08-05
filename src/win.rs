use crate::gl;
use crate::gl_renderer;
use crate::glx;

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
use x11rb::protocol::Event;
use x11rb::protocol::Event::*;
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
        mapped: bool,
        renderer: &gl_renderer::GLRenderer,
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
    pub fn new_handle(
        handle: Window,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<Win, Box<dyn Error>> {
        let geom = conn
            .get_geometry(handle)
            .expect("could not connect to server")
            .reply()
            .expect("could not get geometry");
        let attrs = conn
            .get_window_attributes(handle)
            .expect("could not connect to server")
            .reply()
            .expect("could not get root window attributes");
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
            mapped,
            &renderer,
        )
    }
    pub fn new_event(
        evt: &CreateNotifyEvent,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<Win, Box<dyn Error>> {
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

    pub fn map(
        &mut self,
        evt: &MapNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        self.mapped = true;
        self.override_redirect = evt.override_redirect;
        self.reacquire_pixmap(evt.window, display, config, conn, renderer)?;
        Ok(())
    }
    pub fn unmap(&mut self, evt: &UnmapNotifyEvent) -> Result<(), Box<dyn Error>> {
        self.mapped = false;
        Ok(())
    }

    pub fn destroy(
        &mut self,
        evt: &DestroyNotifyEvent,
        display: *mut glx::types::Display,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
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
    pub fn release_pixmap(
        &mut self,
        display: *mut glx::types::Display,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<(), Box<dyn Error>> {
        if !self.mapped {
            // if not mapped, we already released the pixmaps/textures
            return Ok(());
        }
        renderer.release_glx_pixmap(self, display);
        Ok(())
    }

    // TODO: handle all configure notify possibilities (stacking order, etc.)
    pub fn configure(
        &mut self,
        evt: &ConfigureNotifyEvent,
        display: *mut glx::types::Display,
        config: *const c_void,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
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

pub struct WinTracker {
    wins: Vec<Win>,
}

impl WinTracker {
    pub fn new(
        root: Window,
        conn: &impl x11rb::connection::Connection,
        renderer: &gl_renderer::GLRenderer,
    ) -> Result<WinTracker, Box<dyn Error>> {
        let mut ret = WinTracker {
            wins: vec![Win::new_handle(root, conn, renderer)?],
        };
        let children = conn
            .query_tree(root)
            .expect("could not connect to server")
            .reply()
            .expect("unable to query children")
            .children;
        for child in children {
            ret.wins.push(Win::new_handle(child, conn, renderer)?);
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
    ) -> Result<(), Box<dyn Error>> {
        match event {
            Some(e) => {
                println!("event: {:?}", e);
                match e {
                    CreateNotify(create) => {
                        self.wins.push(
                            Win::new_event(&create, &renderer).expect("could not track window"),
                        );
                    }
                    MapNotify(map) => {
                        let w = self
                            .wins
                            .iter_mut()
                            .find(|w| w.handle == map.window)
                            .expect("map notified with untracked window!");
                        w.map(&map, display, config, conn, renderer)
                            .expect("could not map window");
                    }
                    ConfigureNotify(conf) => {
                        let w = self
                            .wins
                            .iter_mut()
                            .find(|w| w.handle == conf.window)
                            .expect("configure notified with untracked window!");
                        w.configure(&conf, display, config, conn, renderer)
                            .expect("could not configure window");
                    }
                    UnmapNotify(unmap) => {
                        let w = self
                            .wins
                            .iter_mut()
                            .find(|w| w.handle == unmap.window)
                            .expect("unmap notified with untracked window!");
                        w.unmap(&unmap).expect("could not unmap window");
                    }
                    DestroyNotify(destroy) => {
                        self.wins
                            .remove(
                                self.wins
                                    .iter()
                                    .position(|w| w.handle == destroy.window)
                                    .expect("destroy notify for untracked window"),
                            )
                            .destroy(&destroy, display, renderer)
                            .expect("could not destroy window");
                    }
                    _ => println!("unhandled event!"),
                }
            }
            None => (),
        }
        renderer.render(width, height, self, display, overlay);
        Ok(())
    }

    pub fn mapped_wins(&self) -> impl Iterator<Item = &Win> {
        self.wins.iter().filter(|w| w.mapped).into_iter()
    }
}

// impl<'a> IntoIterator for &'a WinTracker {
//     type Item = <std::slice::Iter<'a, Win> as Iterator>::Item;
//     type IntoIter = std::slice::Iter<'a, Win>;

//     fn into_iter(self) -> Self::IntoIter {
//         self.wins.as_slice().into_iter()
//     }
// }
