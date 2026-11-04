fn main() {
    println!("cargo:rerun-if-changed=link.x");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    let script = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("link.x");
    println!("cargo:rustc-link-arg=-T{}", script.display());
}
