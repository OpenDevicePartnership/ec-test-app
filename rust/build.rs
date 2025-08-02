// build.rs
use std::env;
use std::fs;

fn main() {
    // Don't do a fancy build if we're just testing our TUI
    if env::var("CARGO_FEATURE_MOCK").is_ok() {
        println!("cargo:warning=Skipping build.rs logic because 'mock' feature is enabled.");
        return;
    }

    // Make sure all the EWDK environment variables are set
    let sdk_root = env::var("WindowsSdkDir").unwrap_or_else(|_| panic!("Please run SetupBuildEnv.cmd from EWDK"));
    let version = env::var("Version_Number").unwrap_or_else(|_| panic!("Please run SetupBuildEnv.cmd from EWDK"));
    let platform = env::var("Platform").unwrap_or_else(|_| panic!("Please run SetupBuildEnv.cmd from EWDK"));

    println!("cargo:rustc-link-search=native=../lib/{platform}/release");
    println!("cargo:rustc-link-search=native={sdk_root}/Lib/{version}/um/{platform}");
    println!("cargo:rustc-link-search=native={sdk_root}/Lib/{version}/ucrt/{platform}");
    println!("cargo:rustc-link-lib=static=eclib");

    // Copy dll to output folder which is needed to run
    // Get the output directory where Cargo builds artifacts
    let out_target = env::var("TARGET").unwrap_or_else(|_| panic!("--target not specified"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| panic!("Could not determine release or debug"));

    // Define source and destination paths
    let src = format!("../lib/{platform}/release/eclib.dll");
    let dest = format!("target/{out_target}/{profile}/eclib.dll");

    // Copy the file
    match fs::copy(&src, &dest) {
        Ok(_) => println!("Copied DLL to output {dest}"),
        Err(e) => println!("Copy failed: {e}"),
    }

    // Optional: tell Cargo to rerun build.rs if the source file changes
    println!("cargo:rerun-if-changed={src}");
}
