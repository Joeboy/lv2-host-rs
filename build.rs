use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/qt5_helper.cpp");
    println!("cargo:rerun-if-changed=src/gtk2_helper.cpp");
    println!("cargo:rustc-check-cfg=cfg(lv2_host_qt5)");
    println!("cargo:rustc-check-cfg=cfg(lv2_host_gtk2)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let qt = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("Qt5Widgets");
    let suil = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("suil-0");
    match (qt, suil) {
        (Ok(qt), Ok(suil)) => {
            compile_qt5_helper(&qt, &suil);
            println!("cargo:rustc-cfg=lv2_host_qt5");
        }
        _ => println!(
            "cargo:warning=Qt5Widgets or Suil development files not found; Qt5 LV2 UIs disabled"
        ),
    }

    if let (Ok(gtk2), Ok(suil)) = (
        pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("gtk+-2.0"),
        pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("suil-0"),
    ) {
        compile_helper("src/gtk2_helper.cpp", "lv2-host-gtk2-helper", &gtk2, &suil);
        println!("cargo:rustc-cfg=lv2_host_gtk2");
    } else {
        println!("cargo:warning=GTK2 or Suil development files not found; GTK2 LV2 UIs disabled");
    }
}

fn compile_qt5_helper(qt: &pkg_config::Library, suil: &pkg_config::Library) {
    compile_helper("src/qt5_helper.cpp", "lv2-host-qt5-helper", qt, suil);
}

fn compile_helper(
    source: &str,
    executable: &str,
    toolkit: &pkg_config::Library,
    suil: &pkg_config::Library,
) {
    let compiler = cc::Build::new().cpp(true).get_compiler();
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(executable);
    let mut command: Command = compiler.to_command();
    command
        .arg("-std=c++17")
        .arg("-fPIC")
        .arg(source)
        .arg("-o")
        .arg(&output);

    for include in toolkit.include_paths.iter().chain(&suil.include_paths) {
        command.arg("-I").arg(include);
    }
    for (name, value) in toolkit.defines.iter().chain(&suil.defines) {
        command.arg(match value {
            Some(value) => format!("-D{name}={value}"),
            None => format!("-D{name}"),
        });
    }
    for path in toolkit.link_paths.iter().chain(&suil.link_paths) {
        command.arg("-L").arg(path);
    }
    for arguments in toolkit.ld_args.iter().chain(&suil.ld_args) {
        command.args(arguments);
    }
    for library in toolkit.libs.iter().chain(&suil.libs) {
        command.arg(format!("-l{library}"));
    }

    let status = command
        .status()
        .unwrap_or_else(|error| panic!("could not run the C++ compiler for {executable}: {error}"));
    assert!(status.success(), "could not compile {executable}");
}
