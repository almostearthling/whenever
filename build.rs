// build script

use std::env;
use winresource;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_DESC: &str = env!("CARGO_PKG_DESCRIPTION");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_LICENSE: &str = env!("CARGO_PKG_LICENSE");


// provide version info as a 64 bit unsigned: use the provided version string
// to (somewhat arbitrarily) extract the information, although it should be
// consistent when following the versioning guideline: see
// https://github.com/almostearthling/whenever/discussions/88#discussion-9422899
fn version_info_64bit() -> u64 {
    let mut ver: Vec<u64> = Vec::new();
    for s in APP_VERSION.split(".") {
        ver.push(if s.contains("-") {
            u64::from_str_radix(s.split("-").next().unwrap_or("0"), 10).unwrap_or(0)
        } else {
            u64::from_str_radix(s, 10).unwrap_or(0)
        });
    };

    ver[0] << 48 + ver[1] << 32 + ver[2] << 16 + if ver.len() > 3 { ver[3] } else { 0 }
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
                .set_version_info(winresource::VersionInfo::PRODUCTVERSION, version_info_64bit())
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
