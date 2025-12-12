use dirs_next as dirs;
use gettextrs::{
    bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory,
};
use std::{env, fs, path::{Path, PathBuf}};

const APP_ID: &str = "io.github.noobping.listenmoe";

fn find_locale_dir() -> PathBuf {
    // Developer directory (cargo run)
    let dev_dir = Path::new("data").join("locale");
    if dev_dir.is_dir() {
        return dev_dir;
    }

    // <exe>/data/locale or <exe>/locale (AppImage, portable build, etc.)
    if let Ok(exe) = env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join("data").join("locale");
            if candidate.is_dir() {
                return candidate;
            }

            let candidate = exe_dir.join("locale");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }

    // Flatpak / app prefix
    let app_share_locale = Path::new("/app/share/locale");
    if app_share_locale.is_dir() {
        return app_share_locale.to_path_buf();
    }

    // User-level data dir
    if let Some(base) = dirs::data_local_dir() {
        let candidate = base.join(APP_ID).join("locale");
        if candidate.is_dir() {
            return candidate;
        }
    }

    // System locale directory
    let sys_dir = Path::new("/usr/share/locale");
    if sys_dir.is_dir() {
        return sys_dir.to_path_buf();
    }

    // Fallback: dev dir
    let _ = fs::create_dir_all(&dev_dir);
    dev_dir.to_path_buf()
}

pub fn init_i18n() {
    setlocale(LocaleCategory::LcAll, "");

    let dir = find_locale_dir();
    println!("Using locale dir: {}", dir.display());

    let dir_str = dir
        .to_str()
        .expect("Locale path must be UTF-8 for gettext");

    bindtextdomain(APP_ID, dir_str).expect("bindtextdomain failed");
    bind_textdomain_codeset(APP_ID, "UTF-8").expect("bind codeset failed");
    textdomain(APP_ID).expect("textdomain failed");
}
