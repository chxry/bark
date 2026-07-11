fn main() {
    let assets_dir = env!("CARGO_MANIFEST_DIR").to_owned() + "/assets";
    bark_build::build_default(assets_dir);
}
