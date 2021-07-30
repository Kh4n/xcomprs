use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry};
use std::env;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

const XLIB_BINDINGS_FILE: &str = "xlib_bindings.rs";
const GL_BINDINGS_FILE: &str = "gl_bindings.rs";

fn main() {
    println!("cargo:rerun-if-changed=xlib_libs.h");

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=X11-xcb");
    println!("cargo:rustc-link-lib=GL");

    let bindings = bindgen::Builder::default()
        .header("xlib_libs.h")
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join(XLIB_BINDINGS_FILE))
        .expect("Couldn't write bindings!");

    let dest = env::var("OUT_DIR").unwrap();
    let mut file = File::create(&Path::new(&dest).join(GL_BINDINGS_FILE)).unwrap();
    Registry::new(Api::Gl, (3, 3), Profile::Core, Fallbacks::All, [])
        .write_bindings(GlobalGenerator, &mut file)
        .unwrap();
}
