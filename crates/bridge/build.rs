// Re-embed the web app whenever a new `dx bundle` lands in the output folder.
fn main() {
    println!("cargo:rerun-if-changed=../../target/dx/system4dispatch/release/web/public");
}
