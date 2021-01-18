use std::env;
use std::path::PathBuf;
use std::process::Command;

const BOEHM_REPO: &str = "https://github.com/ivmai/bdwgc.git";
const BOEHM_ATOMICS_REPO: &str = "https://github.com/ivmai/libatomic_ops.git";
const BOEHM_DIR: &str = "bdwgc";

fn run<F>(name: &str, mut configure: F)
where
    F: FnMut(&mut Command) -> &mut Command,
{
    let mut command = Command::new(name);
    let configured = configure(&mut command);
    if !configured.status().is_ok() {
        let err = configured.status().unwrap_err();
        panic!("failed to execute {:?}: {}", configured, err);
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = env::var("OUT_DIR").unwrap();
    let mut boehm_src = PathBuf::from(out_dir);
    boehm_src.push(BOEHM_DIR);
    if !boehm_src.exists() {
        run("git", |cmd| {
            cmd.arg("clone").arg(BOEHM_REPO).arg(&boehm_src)
        });

        run("git", |cmd| {
            cmd.arg("clone")
                .arg(BOEHM_ATOMICS_REPO)
                .current_dir(&boehm_src)
        });

        env::set_current_dir(&boehm_src).unwrap();
        run("cmake", |cmd| cmd.arg("."));
        run("cmake", |cmd| {
            cmd.args(&["--build", ".", "--config", "Release"])
        });
    }
    let libpath = PathBuf::from(&boehm_src);
    //libpath.push(BOEHM_DIR);
    println!(
        "cargo:rustc=flags=-L{}",
        &libpath.as_path().to_str().unwrap()
    );
    //panic!();
    println!(
        "cargo:rustc-link-search=all={}",
        &libpath.as_path().to_str().unwrap()
    );
    for entry in std::fs::read_dir(&libpath).unwrap() {
        println!("-entry {}", entry.unwrap());
    }
    println!("link to {}", libpath.display());
    println!("cargo:rustc-link-lib=gc");
}
