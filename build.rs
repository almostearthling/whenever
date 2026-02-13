// build script

use std::env;
use winresource;


// information used during the build process in pre-build actions
const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_DESC: &str = env!("CARGO_PKG_DESCRIPTION");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_LICENSE: &str = env!("CARGO_PKG_LICENSE");

const APP_VER_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");
const APP_VER_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");
const APP_VER_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");


// provide version info as a 64 bit unsigned: none of the required values
// should actually be forcibly set to zero (except for `pre`), as the version
// always follows the guidelines specified in the dedicated discussion: see
// https://github.com/almostearthling/whenever/discussions/88#discussion-9422899
fn version_info_as_u64() -> u64 {
    let app_ver_pre: &str = option_env!("CARGO_PKG_VERSION_PRE").unwrap_or("0");

    u64::from_str_radix(APP_VER_MAJOR, 10).unwrap_or(0) << 48
    | u64::from_str_radix(APP_VER_MINOR, 10).unwrap_or(0) << 32
    | u64::from_str_radix(APP_VER_PATCH, 10).unwrap_or(0) << 16
    | u64::from_str_radix(app_ver_pre, 10).unwrap_or(0) << 0
}


// pre-build actions
fn main() {
    // 1. platform dependent actions
    if let Ok(platform) = env::var("CARGO_CFG_TARGET_OS") {
        match platform.as_str() {
            "windows" => {
                // 1.a: attach an icon and version information
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

                // ...
                // ^^^ other Windows specific actions should be added above here
            }
            "linux" => {
            
            }
            // ...
            // ^^^ other supported platforms should be added above here
            _ => {
                // panic for unsupported platforms
                panic!("unsupported platform: {platform}");
            }
        }

        // ...
        // ^^^ other common pre-build actions should be added above here
    }
}

//end.
