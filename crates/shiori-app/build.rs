//! Puts the app icon into the Windows exe as a resource; other systems take
//! the icon from the window at run time.

fn main() {
    println!("cargo:rerun-if-changed=shiori.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("shiori.rc", embed_resource::NONE)
            .manifest_required()
            .expect("could not compile the Windows resources");
    }
}
