use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry, StaticGenerator};
use std::env;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

const GL_BINDINGS_FILE: &str = "gl_bindings.rs";
const GLX_BINDINGS_FILE: &str = "glx_bindings.rs";
const XLIB_BINDINGS_FILE: &str = "xlib_bindings.rs";

const GLX_EXTENSIONS: [&str; 2] = ["GLX_EXT_texture_from_pixmap", "GLX_ARB_get_proc_address"];

const XLIB_FUNCTIONS: [&str; 5] = [
    "XOpenDisplay",
    "XGetXCBConnection",
    "XDefaultScreen",
    "XSetEventQueueOwner",
    "XFree",
];
const XLIB_VARS: [&str; 1] = ["None"];

fn main() {
    println!("cargo:rerun-if-changed=xlib_libs.h");

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=X11-xcb");
    println!("cargo:rustc-link-lib=GL");

    let mut builder = bindgen::Builder::default().header("xlib_libs.h");
    for function in XLIB_FUNCTIONS {
        builder = builder.allowlist_function(function);
    }
    for var in XLIB_VARS {
        builder = builder.allowlist_var(var);
    }
    let bindings = builder.generate().expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join(XLIB_BINDINGS_FILE))
        .expect("Couldn't write bindings!");

    let dest = env::var("OUT_DIR").unwrap();
    let mut file = File::create(&Path::new(&dest).join(GL_BINDINGS_FILE)).unwrap();
    Registry::new(Api::Gl, (3, 3), Profile::Core, Fallbacks::All, [])
        .write_bindings(GlobalGenerator, &mut file)
        .unwrap();

    let dest = env::var("OUT_DIR").unwrap();
    let mut file = File::create(&Path::new(&dest).join(GLX_BINDINGS_FILE)).unwrap();
    Registry::new(
        Api::Glx,
        (1, 3),
        Profile::Core,
        Fallbacks::All,
        GLX_EXTENSIONS,
    )
    .write_bindings(StaticGenerator, &mut file)
    .unwrap();
}
