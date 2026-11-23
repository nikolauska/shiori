//! Puts the app icon and version information into the Windows exe as
//! resources; other systems take the icon from the window at run time.

fn main() {
    println!("cargo:rerun-if-changed=shiori.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        write_version_header();
        embed_resource::compile("shiori.rc", embed_resource::NONE)
            .manifest_required()
            .expect("could not compile the Windows resources");
    }
}

/// Writes the version from Cargo.toml as a header the resource file includes.
/// A header in `OUT_DIR` instead of `/D` macros, because quoting a string
/// through each resource compiler's command line works differently.
fn write_version_header() {
    let var = |name: &str| std::env::var(name).expect("Cargo sets this for build scripts");
    let numbers = format!(
        "{},{},{},0",
        var("CARGO_PKG_VERSION_MAJOR"),
        var("CARGO_PKG_VERSION_MINOR"),
        var("CARGO_PKG_VERSION_PATCH"),
    );
    let header = format!(
        "#define SHIORI_VERSION_NUMBERS {numbers}\n#define SHIORI_VERSION_TEXT \"{}\"\n",
        var("CARGO_PKG_VERSION"),
    );
    let out_dir = std::path::PathBuf::from(var("OUT_DIR"));
    std::fs::write(out_dir.join("shiori_version.h"), header)
        .expect("could not write the version header");
}
