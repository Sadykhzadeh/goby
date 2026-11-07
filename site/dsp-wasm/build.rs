//! Names the app's version, from its Cargo.toml, for the window's About line.
fn main() {
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    let toml = std::fs::read_to_string("../../Cargo.toml").expect("the app's Cargo.toml");
    let version = toml.lines().find_map(|l| l.strip_prefix("version = \"")?.strip_suffix('"')).expect("a version");
    println!("cargo:rustc-env=GOBY_VERSION={version}");
}
