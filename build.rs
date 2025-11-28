#[cfg(not(feature = "setup"))]
#[cfg(debug_assertions)]
pub const APP_ID: &str = concat!("dev.noobping.", env!("CARGO_PKG_NAME"), "-dev");
#[cfg(not(feature = "setup"))]
#[cfg(not(debug_assertions))]
pub const APP_ID: &str = concat!("dev.noobping.", env!("CARGO_PKG_NAME"));

fn main() {
    glib_build_tools::compile_resources(&["icons"], "icons/resources.xml", "compiled.gresource");

    #[cfg(not(feature = "setup"))]
    desktop_file();
}

#[cfg(not(feature = "setup"))]
fn desktop_file() {
    use std::{fs, path::Path};
    let project = env!("CARGO_PKG_NAME");
    let dir = Path::new(".");
    let version = env!("CARGO_PKG_VERSION");
    let comment = option_env!("CARGO_PKG_DESCRIPTION").unwrap_or("Password manager");
    let contents = format!(
        "[Desktop Entry]
Type=Application
Version={version}
Name={project}
Comment={comment}
Exec={project} %u
Icon={APP_ID}
Terminal=false
Categories=AudioVideo;Player;GTK;
"
    );
    fs::write(&dir.join(format!("{project}.desktop")), contents)
        .expect("Can not build desktop file")
}
