//! Transparent isolated Qt5/Suil UI helper.

use super::{PortUpdate, UiDescriptor, UiInstance, UiOpenRequest};
use std::{
    fs::{OpenOptions, remove_file},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lv2-host-qt5-helper"));
const TIMEOUT: Duration = Duration::from_secs(5);
static HELPER_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct Window {
    _ui_instance: UiInstance,
    child: Child,
    input: ChildStdin,
    reader: Option<thread::JoinHandle<()>>,
}

impl Window {
    pub(super) fn present(&mut self) -> Result<(), std::io::Error> {
        self.input.write_all(&[0; 9])
    }

    pub(super) fn update_control(&mut self, index: u32, value: f32) -> Result<(), std::io::Error> {
        let mut command = [0; 9];
        command[0] = 1;
        command[1..5].copy_from_slice(&index.to_le_bytes());
        command[5..9].copy_from_slice(&value.to_le_bytes());
        self.input.write_all(&command)
    }
    pub(super) fn is_running(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|status| status.is_none())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn open(
    request: UiOpenRequest,
    ui: UiDescriptor,
    ui_type: String,
) -> Result<Window, String> {
    if !Path::new(&ui.binary_path).is_file() {
        return Err(format!("plugin UI binary is missing: {}", ui.binary_path));
    }
    let helper = write_helper()?;
    let child = Command::new(&helper)
        .env("QT_QPA_PLATFORM", "xcb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let _ = remove_file(&helper);
    let mut child = child.map_err(|error| format!("could not start Qt5 UI helper: {error}"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| "Qt5 UI helper has no input pipe".to_owned())?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| "Qt5 UI helper has no output pipe".to_owned())?;
    if let Err(error) = send_request(&mut input, &request, &ui, &ui_type) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let (startup, ready) = std::sync::mpsc::sync_channel(1);
    let instance_id = request.id;
    let ports = request.instance.ports.clone();
    let port_update = request.options.port_update.clone();
    let reader = thread::Builder::new()
        .name(format!("lv2-host-qt5-reader-{instance_id}"))
        .spawn(move || read_messages(output, startup, ports, port_update))
        .map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            format!("could not monitor Qt5 UI helper: {error}")
        })?;
    match ready.recv_timeout(TIMEOUT) {
        Ok(Ok(())) => Ok(Window {
            _ui_instance: request.instance,
            child,
            input,
            reader: Some(reader),
        }),
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            Err(error)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            Err("Qt5 UI helper did not respond".to_owned())
        }
    }
}

fn write_helper() -> Result<PathBuf, String> {
    let id = HELPER_ID.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("lv2-host-qt5-helper-{}-{id}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("could not create temporary Qt5 UI helper: {error}"))?;
    file.write_all(HELPER)
        .map_err(|error| format!("could not write temporary Qt5 UI helper: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("could not make Qt5 UI helper executable: {error}"))?;
    }
    drop(file);
    Ok(path)
}

fn send_request(
    output: &mut impl Write,
    request: &UiOpenRequest,
    ui: &UiDescriptor,
    ui_type: &str,
) -> Result<(), String> {
    output
        .write_all(b"LV2Q")
        .map_err(|error| format!("could not send Qt5 UI request: {error}"))?;
    for value in [
        request.instance.plugin_uri.as_str(),
        request.options.window_title.as_str(),
        ui.uri.as_str(),
        ui_type,
        ui.bundle_path.as_str(),
        ui.binary_path.as_str(),
    ] {
        write_string(output, value)?;
    }
    write_u32(output, request.instance.ports.len())?;
    for (index, symbol) in &request.instance.ports {
        output
            .write_all(&index.to_le_bytes())
            .map_err(|error| format!("could not send Qt5 UI request: {error}"))?;
        write_string(output, symbol)?;
    }
    write_u32(output, request.instance.controls.len())?;
    for (index, value) in &request.instance.controls {
        output
            .write_all(&index.to_le_bytes())
            .and_then(|_| output.write_all(&value.to_le_bytes()))
            .map_err(|error| format!("could not send Qt5 UI request: {error}"))?;
    }
    output
        .flush()
        .map_err(|error| format!("could not send Qt5 UI request: {error}"))
}

fn write_u32(output: &mut impl Write, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "Qt5 UI request is too large".to_owned())?;
    output
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("could not send Qt5 UI request: {error}"))
}

fn write_string(output: &mut impl Write, value: &str) -> Result<(), String> {
    write_u32(output, value.len())?;
    output
        .write_all(value.as_bytes())
        .map_err(|error| format!("could not send Qt5 UI request: {error}"))
}

fn read_messages(
    mut input: impl Read,
    startup: std::sync::mpsc::SyncSender<Result<(), String>>,
    ports: std::collections::BTreeMap<u32, String>,
    port_update: super::PortUpdateCallback,
) {
    let mut startup = Some(startup);
    loop {
        let mut kind = [0u8; 1];
        if input.read_exact(&mut kind).is_err() {
            if let Some(startup) = startup.take() {
                let _ = startup.send(Err("Qt5 UI helper exited during startup".to_owned()));
            }
            return;
        }
        match kind[0] {
            0 => {
                if let Some(startup) = startup.take() {
                    let _ = startup.send(Ok(()));
                }
            }
            1 => {
                let message = read_string(&mut input)
                    .unwrap_or_else(|error| format!("invalid Qt5 UI error: {error}"));
                if let Some(startup) = startup.take() {
                    let _ = startup.send(Err(message));
                } else {
                    eprintln!("LV2 Qt5 UI helper: {message}");
                }
                return;
            }
            2 => {
                let Ok(index) = read_u32(&mut input) else {
                    return;
                };
                let Ok(value) = read_f32(&mut input) else {
                    return;
                };
                if let Some(port) = ports.get(&index) {
                    port_update(PortUpdate {
                        port: port.clone(),
                        value,
                    });
                }
            }
            _ => return,
        }
    }
}

fn read_u32(input: &mut impl Read) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|error| format!("could not read Qt5 UI helper message: {error}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_f32(input: &mut impl Read) -> Result<f32, String> {
    let mut bytes = [0u8; 4];
    input
        .read_exact(&mut bytes)
        .map_err(|error| format!("could not read Qt5 UI helper message: {error}"))?;
    Ok(f32::from_le_bytes(bytes))
}

fn read_string(input: &mut impl Read) -> Result<String, String> {
    let length = read_u32(input)? as usize;
    let mut bytes = vec![0u8; length];
    input
        .read_exact(&mut bytes)
        .map_err(|error| format!("could not read Qt5 UI helper message: {error}"))?;
    String::from_utf8(bytes).map_err(|_| "Qt5 UI helper sent invalid UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_abgate_with_suil_qt5_host() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("example-plugins/build/abGate.lv2")
            .canonicalize()
            .unwrap();
        let lv2 = crate::Host::with_load_bundle(&format!("file://{}/", bundle.display()));
        let context = lv2
            .processing_context(crate::ProcessingConfig::default())
            .unwrap();
        let instance = lv2
            .plugin("http://hippie.lt/lv2/gate")
            .unwrap()
            .instantiate(&context)
            .unwrap();
        let ui_instance = instance.ui_instance();
        let (index, value) = ui_instance.controls[0];
        let symbol = ui_instance.ports[&index].clone();
        drop(instance);
        let ui_host = crate::UiHost::new();
        let window = ui_host
            .open(
                ui_instance,
                crate::UiOptions {
                    window_title: "abGate".to_owned(),
                    port_update: std::sync::Arc::new(|_| {}),
                },
            )
            .unwrap();
        thread::sleep(Duration::from_millis(250));
        drop(ui_host);
        assert!(window.is_open());
        window.present().unwrap();
        window.update_control(&symbol, value).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(window.is_open());
        window.close();
        thread::sleep(Duration::from_millis(100));
    }
}
