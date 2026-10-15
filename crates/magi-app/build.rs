//! Gives the Windows executable its icon and version information, so
//! Explorer, the taskbar and shortcuts show the MacroMagi icon.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/magi.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../../assets/icon/magi.ico")
            .set("ProductName", "MacroMagi")
            .set("FileDescription", "MacroMagi")
            .set("InternalName", "macromagi")
            .set("OriginalFilename", "macromagi.exe");
        // A missing resource compiler costs only the icon, not the build.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=the executable icon could not be embedded: {error}");
        }
    }
}
