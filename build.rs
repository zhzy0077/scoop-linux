fn main() {
    println!(
        "cargo:rustc-env=SCOOP_BUILD_TARGET={}",
        std::env::var("TARGET").expect("Cargo TARGET")
    );
}
