use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/qt5_helper.cpp");
    println!("cargo:rustc-check-cfg=cfg(lv2_host_qt5)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let qt = pkg_config::Config::new().probe("Qt5Widgets");
    let suil = pkg_config::Config::new().probe("suil-0");
    match (qt, suil) {
        (Ok(qt), Ok(suil)) => {
            compile_qt5_helper(&qt, &suil);
            println!("cargo:rustc-cfg=lv2_host_qt5");
        }
        _ => println!(
            "cargo:warning=Qt5Widgets or Suil development files not found; Qt5 LV2 UIs disabled"
        ),
    }
}

fn compile_qt5_helper(qt: &pkg_config::Library, suil: &pkg_config::Library) {
    let compiler = cc::Build::new().cpp(true).get_compiler();
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("lv2-host-qt5-helper");
    let mut command: Command = compiler.to_command();
    command
        .arg("-std=c++17")
        .arg("-fPIC")
        .arg("src/qt5_helper.cpp")
        .arg("-o")
        .arg(&output);

    for include in qt.include_paths.iter().chain(&suil.include_paths) {
        command.arg("-I").arg(include);
    }
    for (name, value) in qt.defines.iter().chain(&suil.defines) {
        command.arg(match value {
            Some(value) => format!("-D{name}={value}"),
            None => format!("-D{name}"),
        });
    }
    for path in qt.link_paths.iter().chain(&suil.link_paths) {
        command.arg("-L").arg(path);
    }
    for arguments in qt.ld_args.iter().chain(&suil.ld_args) {
        command.args(arguments);
    }
    for library in qt.libs.iter().chain(&suil.libs) {
        command.arg(format!("-l{library}"));
    }

    let status = command
        .status()
        .expect("could not run the C++ compiler for the Qt5 UI helper");
    assert!(status.success(), "could not compile the Qt5 UI helper");
}
