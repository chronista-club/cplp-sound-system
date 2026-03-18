use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let header_path = out_dir.join("cplp_ffi.h");

    let config = cbindgen::Config::from_file("cbindgen.toml")
        .unwrap_or_else(|_| cbindgen::Config::default());

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed")
        .write_to_file(&header_path);

    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
