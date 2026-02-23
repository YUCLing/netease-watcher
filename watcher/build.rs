use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let mut crates_to_build: Vec<(String, String)> = Vec::new();

    // Always build the wndhok
    crates_to_build.push(("wndhok".to_string(), "hook".to_string()));

    if cfg!(unix) {
        crates_to_build.push(("winehooker".to_string(), "hooker".to_string()));
    }

    let out_dir_str = env::var("OUT_DIR").unwrap();
    let out_dir = PathBuf::from(&out_dir_str);
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let is_release = profile == "release";

    // 1. Detect target (MinGW or MSVC)
    let target_triple_opt = detect_target_via_sysroot();

    if let Some(target_triple) = target_triple_opt {
        // Define where we will build these artifacts (nested to avoid lock conflicts)
        let nested_target_dir = out_dir.join("win_builds");

        // 2. Identify the final installation directory (target/debug or target/release)
        // We go up 3 levels from OUT_DIR to find the root target dir.
        let install_dir = out_dir.ancestors().nth(3);

        if install_dir.is_none() {
            println!(
                "cargo:warning=Could not determine install directory. Artifacts won't be copied."
            );
        }

        // 3. Loop through each crate and attempt to build
        for (pkg_name, dir_name) in &crates_to_build {
            // Setup Command
            let mut cmd = Command::new("cargo");
            cmd.arg("build")
                .arg("--package")
                .arg(pkg_name)
                .arg("--target")
                .arg(&target_triple);

            if is_release {
                cmd.arg("--release");
            }

            cmd.env("CARGO_TARGET_DIR", &nested_target_dir);

            let build_success = match cmd.status() {
                Ok(status) => status.success(),
                Err(_) => false,
            };

            if build_success {
                // The artifacts are located in: nested_target_dir/<target_triple>/<profile>/
                let build_artifact_dir = nested_target_dir
                    .join(&target_triple)
                    .join(if is_release { "release" } else { "debug" });

                if let Some(dest_dir) = install_dir {
                    // Helper function to copy dll/exe/pdb
                    copy_artifacts(&build_artifact_dir, dest_dir);
                }
            } else {
                println!("cargo:warning=Failed to build package '{}'.", pkg_name);
            }

            // Ensure we watch the source directory of this crate for changes
            let crate_path = Path::new(&manifest_dir).parent().unwrap().join(dir_name);
            println!("cargo:rerun-if-changed={}", crate_path.display());
        }
    } else {
        println!(
            "cargo:warning=No valid Windows target (gnu/msvc) found. Skipping Windows builds. CBT hook will not be available. If you are on Windows, please ensure you have the appropriate Rust toolchain installed (e.g., x86_64-pc-windows-msvc or x86_64-pc-windows-gnu)."
        );
    }
}

/// Scans the source directory for .dll, .exe, and .pdb files and copies them to dest.
fn copy_artifacts(src: &Path, dest: &Path) {
    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Check extension
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_str().unwrap_or("").to_lowercase();

                if ["dll", "exe", "pdb"].contains(&ext_str.as_str()) {
                    let file_name = path.file_name().unwrap();
                    let dest_path = dest.join(file_name);

                    if let Err(e) = fs::copy(&path, &dest_path) {
                        println!(
                            "cargo:warning=Failed to copy {}: {}",
                            file_name.to_string_lossy(),
                            e
                        )
                    }
                }
            }
        }
    } else {
        println!(
            "cargo:warning=Could not read artifact directory: {}",
            src.display()
        );
    }
}

fn detect_target_via_sysroot() -> Option<String> {
    let candidates = if cfg!(windows) {
        vec!["x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu"]
    } else {
        vec!["x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"]
    };

    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());

    // Wrap command in Result to prevent panic
    let output = Command::new(rustc)
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sysroot_str = String::from_utf8(output.stdout).ok()?;
    let sysroot = PathBuf::from(sysroot_str.trim());

    for target in &candidates {
        if sysroot
            .join("lib/rustlib")
            .join(target)
            .join("lib")
            .exists()
        {
            return Some(target.to_string());
        }
    }
    None
}
