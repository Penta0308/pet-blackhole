use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=shaders/slime.vert");
    println!("cargo:rerun-if-changed=shaders/slime.frag");

    compile_shader("shaders/slime.vert", "target/slime.vert.spv");
    compile_shader("shaders/slime.frag", "target/slime.frag.spv");
}

fn compile_shader(input: &str, output: &str) {
    let status = Command::new("glslc")
        .args([input, "-o", output])
        .status()
        .unwrap_or_else(|error| panic!("failed to run glslc for {input}: {error}"));

    if !status.success() {
        panic!("glslc failed for {input}");
    }
}
