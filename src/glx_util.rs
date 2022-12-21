use std::ffi::c_void;
use std::ptr::null;

use crate::errors;
use crate::glx;
use crate::xlib;

pub struct GlxInfo {
    pub config: *const c_void,
}

const COLOR_BITS: i32 = 8;

const NUM_FB_ATTRS: usize = 11;
#[rustfmt::skip]
const FB_ATTRS: [i32; NUM_FB_ATTRS * 2 + 1] = [
    // glx::BIND_TO_TEXTURE_RGB_EXT as i32,
    // true as i32,

    // glx::BIND_TO_TEXTURE_TARGETS_EXT as i32,
    // glx::TEXTURE_2D_EXT as i32,

    // glx::Y_INVERTED_EXT as i32,
    // glx::DONT_CARE as i32,

    // glx::DOUBLEBUFFER as i32,
    // true as i32,

    glx::DRAWABLE_TYPE as i32,
    glx::PIXMAP_BIT as i32,

    glx::X_RENDERABLE as i32,
    true as i32,

    glx::X_VISUAL_TYPE as i32,
    glx::TRUE_COLOR as i32,

    glx::RED_SIZE as i32,
    COLOR_BITS,
    glx::GREEN_SIZE as i32,
    COLOR_BITS,
    glx::BLUE_SIZE as i32,
    COLOR_BITS,
    glx::ALPHA_SIZE as i32,
    COLOR_BITS,
    glx::DEPTH_SIZE as i32,
    0,
    glx::STENCIL_SIZE as i32,
    0,
    glx::BUFFER_SIZE as i32,
    8*4,

    glx::RENDER_TYPE as i32,
    glx::RGBA_BIT as i32,

    xlib::None as i32,
];

fn get_fb_config_attr(
    display: &mut glx::types::Display,
    conf: *const c_void,
    attribute: u32,
) -> Result<i32, String> {
    let ret: i32 = 0;
    let err = unsafe {
        glx::GetFBConfigAttrib(
            display as *mut glx::types::Display,
            conf,
            attribute as i32,
            ret as *mut i32,
        )
    };
    if err != 0 {
        return Err(format!("Could not get FB config attribute: {}", attribute));
    }
    Ok(ret)
}

pub fn find_fb_config(
    display: &mut glx::types::Display,
    screen_num: i32,
) -> Result<GlxInfo, String> {
    let mut num_configs: i32 = 0;
    let fb_configs = unsafe {
        glx::ChooseFBConfig(
            display as *mut glx::types::Display,
            screen_num as i32,
            &FB_ATTRS as *const i32,
            &mut num_configs,
        )
    };

    let mut conf = null::<c_void>();
    for i in 0..num_configs {
        conf = unsafe { *fb_configs.offset(i as isize) };

        let red = get_fb_config_attr(display, conf, glx::RED_SIZE)
            .expect("unable to get attribute RED_SIZE");
        let green = get_fb_config_attr(display, conf, glx::GREEN_SIZE)
            .expect("unable to get attribute GREEN_SIZE");
        let blue = get_fb_config_attr(display, conf, glx::BLUE_SIZE)
            .expect("unable to get attribute BLUE_SIZE");
        if red != COLOR_BITS || green != COLOR_BITS || blue != COLOR_BITS {
            continue;
        }
    }

    Ok(GlxInfo { config: conf })
}
