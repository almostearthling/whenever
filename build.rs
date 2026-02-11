// build script

use std::env;
use winresource;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_DESC: &str = env!("CARGO_PKG_DESCRIPTION");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_LICENSE: &str = env!("CARGO_PKG_LICENSE");

const APP_VER_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");
const APP_VER_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");
const APP_VER_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");


// provide version info as a 64 bit unsigned: use the provided version string
// to (somewhat arbitrarily) extract the information, although it should be
// consistent when following the versioning guideline: see
// https://github.com/almostearthling/whenever/discussions/88#discussion-9422899
fn version_info_as_u64() -> u64 {
    let app_ver_pre: &str = option_env!("CARGO_PKG_VERSION_PRE").unwrap_or("0");

    u64::from_str_radix(APP_VER_MAJOR, 10).unwrap_or(0) << 48
    | u64::from_str_radix(APP_VER_MINOR, 10).unwrap_or(0) << 32
    | u64::from_str_radix(APP_VER_PATCH, 10).unwrap_or(0) << 16
    | u64::from_str_radix(app_ver_pre, 10).unwrap_or(0) << 0
}


fn main() {
    // attach an icon and version information when the target OS is Windows
    if let Ok(platform) = env::var("CARGO_CFG_TARGET_OS") {
        if platform == "windows" {
            println!("cargo::rerun-if-changed=Cargo.toml");
            println!("cargo::rerun-if-changed=resources/metronome.ico");
            let mut res = winresource::WindowsResource::new();
            res
                .set_icon("resources/metronome.ico")
                .set_version_info(
                    winresource::VersionInfo::PRODUCTVERSION, 
                    version_info_as_u64(),
                )
                .set("InternalName", APP_NAME)
                .set("FileDescription", APP_DESC)
                .set("ProductVersion", APP_VERSION)
                .set("LegalCopyright", APP_LICENSE);
            
            // panic here if something went wrong
            res.compile().expect("error attaching resources");
        }
    }
}

//end.
