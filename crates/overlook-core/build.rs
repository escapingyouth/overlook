fn main() {
    // macOS only: Windows/Linux cross checks have no C toolchain for the target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/nstat.c");
        cc::Build::new().file("src/nstat.c").flag("-fblocks").compile("ovnstat");
    }
}
