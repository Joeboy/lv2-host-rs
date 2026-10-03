//! LV2 plugin discovery, processing, and fuss-free native UI hosting.

pub use livi;

mod context;
mod error;
mod instance;

pub use context::{ProcessingConfig, ProcessingContext};
pub use error::Error;
pub use instance::{AtomEvent, HostedInstance, UiInstance};

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PortKind {
    Audio,
    Control,
    Cv,
    AtomSequence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortDescriptor {
    pub index: u32,
    pub symbol: String,
    pub name: String,
    pub input: bool,
    pub kind: PortKind,
    pub default: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub logarithmic: bool,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub uri: String,
    pub name: String,
    pub classes: Vec<String>,
    pub ports: Vec<PortDescriptor>,
    pub uis: Vec<UiDescriptor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDescriptor {
    pub uri: String,
    pub classes: Vec<String>,
    pub bundle_path: String,
    pub binary_path: String,
}

pub fn discover_uis(plugin: &livi::Plugin) -> Vec<UiDescriptor> {
    let Some(uis) = plugin.raw().uis() else {
        return Vec::new();
    };

    uis.iter()
        .filter_map(|ui| {
            let uri = ui.uri().as_uri()?.to_owned();
            let bundle_path = ui.bundle_uri()?.path()?.1;
            let binary_path = ui.binary_uri()?.path()?.1;
            let mut classes = ui
                .classes()
                .iter()
                .filter_map(|class| class.as_uri().map(str::to_owned))
                .collect::<Vec<_>>();
            classes.sort();
            classes.dedup();
            Some(UiDescriptor {
                uri,
                classes,
                bundle_path,
                binary_path,
            })
        })
        .collect()
}

pub fn describe_plugin(world: &livi::World, plugin: &livi::Plugin) -> PluginDescriptor {
    use livi::PortType as T;

    let rdf_type = world.raw().new_uri(RDF_TYPE);
    let logarithmic = world
        .raw()
        .new_uri("http://lv2plug.in/ns/ext/port-props#logarithmic");
    let unit_property = world
        .raw()
        .new_uri("http://lv2plug.in/ns/extensions/units#unit");
    let unit_symbol = world
        .raw()
        .new_uri("http://lv2plug.in/ns/extensions/units#symbol");
    let mut classes = plugin
        .raw()
        .value(&rdf_type)
        .into_iter()
        .filter_map(|node| node.as_uri().map(str::to_owned))
        .collect::<Vec<_>>();
    classes.sort();
    classes.dedup();
    let ports = plugin
        .ports()
        .map(|port| {
            let (input, kind) = match port.port_type {
                T::AudioInput => (true, PortKind::Audio),
                T::AudioOutput => (false, PortKind::Audio),
                T::CVInput => (true, PortKind::Cv),
                T::CVOutput => (false, PortKind::Cv),
                T::ControlInput => (true, PortKind::Control),
                T::ControlOutput => (false, PortKind::Control),
                T::AtomSequenceInput => (true, PortKind::AtomSequence),
                T::AtomSequenceOutput => (false, PortKind::AtomSequence),
            };
            let raw_port = plugin
                .raw()
                .iter_ports()
                .find(|candidate| candidate.index() == port.index.0);
            let unit = raw_port.as_ref().and_then(|raw_port| {
                raw_port
                    .value(&unit_property)
                    .iter()
                    .next()
                    .and_then(|unit| {
                        let symbols = world.raw().find_nodes(Some(&unit), &unit_symbol, None);
                        symbols
                            .iter()
                            .next()
                            .and_then(|symbol| symbol.as_str().map(str::to_owned))
                            .or_else(|| {
                                world
                                    .raw()
                                    .symbol(&unit)
                                    .and_then(|symbol| symbol.as_str().map(str::to_owned))
                            })
                    })
            });
            PortDescriptor {
                index: port.index.0 as u32,
                symbol: port.symbol,
                name: port.name,
                input,
                kind,
                default: port.default_value,
                minimum: port.min_value.unwrap_or(-f32::MAX),
                maximum: port.max_value.unwrap_or(f32::MAX),
                logarithmic: raw_port.is_some_and(|port| port.has_property(&logarithmic)),
                unit,
            }
        })
        .collect();
    PluginDescriptor {
        uri: plugin.uri(),
        name: plugin.name(),
        classes,
        ports,
        uis: discover_uis(plugin),
    }
}

pub fn discover_plugins(world: &livi::World) -> BTreeMap<String, PluginDescriptor> {
    world
        .iter_plugins()
        .map(|plugin| {
            let descriptor = describe_plugin(world, &plugin);
            (descriptor.uri.clone(), descriptor)
        })
        .collect()
}

/// Owns an LV2 world and the metadata discovered from it.
pub struct Host {
    world: livi::World,
    plugins: BTreeMap<String, Arc<PluginDescriptor>>,
}

impl Host {
    pub fn new() -> Self {
        Self::from_world(livi::World::new())
    }

    pub fn with_load_bundle(bundle_uri: &str) -> Self {
        Self::from_world(livi::World::with_load_bundle(bundle_uri))
    }

    fn from_world(world: livi::World) -> Self {
        let plugins = discover_plugins(&world)
            .into_iter()
            .map(|(uri, descriptor)| (uri, Arc::new(descriptor)))
            .collect();
        Self { world, plugins }
    }

    pub fn plugins(&self) -> &BTreeMap<String, Arc<PluginDescriptor>> {
        &self.plugins
    }

    /// Looks up a plugin which can inspect metadata and create instances.
    pub fn plugin(&self, uri: &str) -> Result<Plugin, Error> {
        let unavailable = || Error::PluginUnavailable {
            uri: uri.to_owned(),
        };
        Ok(Plugin {
            native: self.world.plugin_by_uri(uri).ok_or_else(unavailable)?,
            descriptor: self.plugins.get(uri).cloned().ok_or_else(unavailable)?,
        })
    }

    pub fn processing_context(&self, config: ProcessingConfig) -> Result<ProcessingContext, Error> {
        ProcessingContext::new(&self.world, config)
    }
}

/// A discovered plugin. Clones share metadata and retain its Lilv world.
#[derive(Clone)]
pub struct Plugin {
    native: livi::Plugin,
    descriptor: Arc<PluginDescriptor>,
}

impl Plugin {
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    /// Creates an instance that may outlive this plugin and its host.
    pub fn instantiate(&self, context: &ProcessingContext) -> Result<HostedInstance, Error> {
        HostedInstance::instantiate(
            self.native.clone(),
            self.descriptor.clone(),
            context.clone(),
        )
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct PortUpdate {
    pub port: String,
    pub value: f32,
}

pub type PortUpdateCallback = Arc<dyn Fn(PortUpdate) + Send + Sync>;

/// Application-specific options for a native plugin UI window.
pub struct UiOptions {
    /// Title for the native top-level window.
    pub window_title: String,
    /// Receives control-port writes made by the plugin UI.
    pub port_update: PortUpdateCallback,
}

struct UiOpenRequest {
    id: u64,
    alive: Arc<std::sync::atomic::AtomicBool>,
    instance: UiInstance,
    options: UiOptions,
}

#[cfg(all(target_os = "linux", lv2_host_gtk2))]
mod gtk2;
#[cfg(all(target_os = "linux", lv2_host_qt5))]
mod qt5;

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use gdk::prelude::DisplayExtManual;
    use gtk::{gdk, glib, prelude::*};
    use libloading::Library;
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        ffi::{CStr, CString, c_char, c_void},
        ptr,
        rc::Rc,
        sync::{
            OnceLock,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
        time::Duration,
    };

    const GTK3_UI: &CStr = c"http://lv2plug.in/ns/extensions/ui#Gtk3UI";
    #[cfg(lv2_host_gtk2)]
    const GTK2_UI: &CStr = c"http://lv2plug.in/ns/extensions/ui#GtkUI";
    #[cfg(lv2_host_qt5)]
    const QT5_UI: &CStr = c"http://lv2plug.in/ns/extensions/ui#Qt5UI";
    const INSTANCE_ACCESS: &CStr = c"http://lv2plug.in/ns/ext/instance-access";
    const UI_PARENT: &CStr = c"http://lv2plug.in/ns/extensions/ui#parent";
    const UI_IDLE: &CStr = c"http://lv2plug.in/ns/extensions/ui#idleInterface";
    const EXTERNAL_UI: &CStr = c"http://kxstudio.sf.net/ns/lv2ext/external-ui#Widget";
    const EXTERNAL_HOST: &CStr = c"http://kxstudio.sf.net/ns/lv2ext/external-ui#Host";

    type PortWrite = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void);
    type PortIndex = unsafe extern "C" fn(*mut c_void, *const c_char) -> u32;
    type PortSubscribe =
        unsafe extern "C" fn(*mut c_void, u32, u32, *const *const Lv2Feature) -> u32;
    type PortUnsubscribe = unsafe extern "C" fn(*mut c_void, u32, u32) -> u32;

    #[repr(C)]
    struct Lv2Feature {
        uri: *const c_char,
        data: *mut c_void,
    }

    #[repr(C)]
    struct IdleInterface {
        idle: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
    }

    #[repr(C)]
    struct ExternalWidget {
        run: Option<unsafe extern "C" fn(*mut ExternalWidget)>,
        show: Option<unsafe extern "C" fn(*mut ExternalWidget)>,
        hide: Option<unsafe extern "C" fn(*mut ExternalWidget)>,
    }

    #[repr(C)]
    struct ExternalHost {
        ui_closed: Option<unsafe extern "C" fn(*mut c_void)>,
        plugin_human_id: *const c_char,
    }

    struct SuilApi {
        _library: Library,
        host_new: unsafe extern "C" fn(
            Option<PortWrite>,
            Option<PortIndex>,
            Option<PortSubscribe>,
            Option<PortUnsubscribe>,
        ) -> *mut c_void,
        host_free: unsafe extern "C" fn(*mut c_void),
        ui_supported: unsafe extern "C" fn(*const c_char, *const c_char) -> u32,
        instance_new: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            *const *const Lv2Feature,
        ) -> *mut c_void,
        instance_free: unsafe extern "C" fn(*mut c_void),
        instance_get_widget: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        instance_get_handle: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        instance_extension_data: unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_void,
        instance_port_event: unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void),
    }

    impl SuilApi {
        fn load() -> Result<Self, String> {
            let library = ["libsuil-0.so.0", "libsuil-0.so"]
                .into_iter()
                .find_map(|name| unsafe { Library::new(name).ok() })
                .ok_or_else(|| "Suil is not installed (could not load libsuil-0)".to_owned())?;
            unsafe {
                macro_rules! symbol {
                    ($name:literal) => {
                        *library
                            .get(concat!($name, "\0").as_bytes())
                            .map_err(|error| format!("Suil is missing {}: {error}", $name))?
                    };
                }
                Ok(Self {
                    host_new: symbol!("suil_host_new"),
                    host_free: symbol!("suil_host_free"),
                    ui_supported: symbol!("suil_ui_supported"),
                    instance_new: symbol!("suil_instance_new"),
                    instance_free: symbol!("suil_instance_free"),
                    instance_get_widget: symbol!("suil_instance_get_widget"),
                    instance_get_handle: symbol!("suil_instance_get_handle"),
                    instance_extension_data: symbol!("suil_instance_extension_data"),
                    instance_port_event: symbol!("suil_instance_port_event"),
                    _library: library,
                })
            }
        }
    }

    struct Controller {
        by_index: BTreeMap<u32, String>,
        by_symbol: BTreeMap<String, u32>,
        port_update: PortUpdateCallback,
        alive: Arc<AtomicBool>,
    }

    unsafe extern "C" fn external_ui_closed(controller: *mut c_void) {
        if let Some(controller) = unsafe { (controller as *const Controller).as_ref() } {
            controller.alive.store(false, Ordering::Release);
        }
    }

    unsafe extern "C" fn write_port(
        controller: *mut c_void,
        index: u32,
        size: u32,
        protocol: u32,
        buffer: *const c_void,
    ) {
        if controller.is_null() || buffer.is_null() || protocol != 0 || size != 4 {
            return;
        }
        let controller = unsafe { &*(controller as *const Controller) };
        let Some(port) = controller.by_index.get(&index) else {
            return;
        };
        let value = unsafe { *(buffer as *const f32) };
        (controller.port_update)(PortUpdate {
            port: port.clone(),
            value,
        });
    }

    unsafe extern "C" fn port_index(controller: *mut c_void, symbol: *const c_char) -> u32 {
        if controller.is_null() || symbol.is_null() {
            return u32::MAX;
        }
        let controller = unsafe { &*(controller as *const Controller) };
        let symbol = unsafe { CStr::from_ptr(symbol) }.to_string_lossy();
        controller
            .by_symbol
            .get(symbol.as_ref())
            .copied()
            .unwrap_or(u32::MAX)
    }

    struct Window {
        alive: Arc<AtomicBool>,
        api: Rc<SuilApi>,
        _ui_instance: UiInstance,
        window: gtk::Window,
        container: gtk::Box,
        widget: gtk::Widget,
        host: *mut c_void,
        instance: *mut c_void,
        controller: *mut Controller,
    }

    impl Window {
        fn update_control(&self, index: u32, value: f32) {
            unsafe {
                (self.api.instance_port_event)(
                    self.instance,
                    index,
                    4,
                    0,
                    (&value as *const f32).cast(),
                );
            }
        }
        fn idle(&self) {
            let interface = unsafe {
                (self.api.instance_extension_data)(self.instance, UI_IDLE.as_ptr())
                    as *const IdleInterface
            };
            if let Some(idle) = unsafe { interface.as_ref() }.and_then(|value| value.idle) {
                let handle = unsafe { (self.api.instance_get_handle)(self.instance) };
                unsafe { idle(handle) };
            }
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::Release);
            self.container.remove(&self.widget);
            unsafe {
                (self.api.instance_free)(self.instance);
                (self.api.host_free)(self.host);
                drop(Box::from_raw(self.controller));
            }
            unsafe { self.window.destroy() };
        }
    }

    struct ExternalWindow {
        alive: Arc<AtomicBool>,
        api: Rc<SuilApi>,
        _ui_instance: UiInstance,
        host: *mut c_void,
        instance: *mut c_void,
        widget: *mut ExternalWidget,
        controller: *mut Controller,
        _external_host: Box<ExternalHost>,
        _title: CString,
    }

    impl ExternalWindow {
        fn show(&self) {
            if let Some(show) = unsafe { (*self.widget).show } {
                unsafe { show(self.widget) };
            }
        }

        fn idle(&self) {
            if let Some(run) = unsafe { (*self.widget).run } {
                unsafe { run(self.widget) };
            }
        }

        fn update_control(&self, index: u32, value: f32) {
            unsafe {
                (self.api.instance_port_event)(
                    self.instance,
                    index,
                    4,
                    0,
                    (&value as *const f32).cast(),
                );
            }
        }
    }

    impl Drop for ExternalWindow {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::Release);
            unsafe {
                (self.api.instance_free)(self.instance);
                (self.api.host_free)(self.host);
                drop(Box::from_raw(self.controller));
            }
        }
    }

    enum HostedUi {
        Gtk(Window),
        External(ExternalWindow),
        #[cfg(lv2_host_qt5)]
        Qt5(super::qt5::Window),
        #[cfg(lv2_host_gtk2)]
        Gtk2(super::gtk2::Window),
    }

    impl HostedUi {
        fn present(&mut self) -> bool {
            match self {
                Self::Gtk(window) => {
                    window.window.present();
                    true
                }
                Self::External(window) => {
                    window.show();
                    true
                }
                #[cfg(lv2_host_qt5)]
                Self::Qt5(window) => window.present().is_ok(),
                #[cfg(lv2_host_gtk2)]
                Self::Gtk2(window) => window.present().is_ok(),
            }
        }

        fn is_running(&mut self) -> bool {
            match self {
                Self::Gtk(window) => window.alive.load(Ordering::Acquire),
                Self::External(window) => window.alive.load(Ordering::Acquire),
                #[cfg(lv2_host_qt5)]
                Self::Qt5(window) => window.is_running(),
                #[cfg(lv2_host_gtk2)]
                Self::Gtk2(window) => window.is_running(),
            }
        }

        fn update_control(&mut self, index: u32, value: f32) -> bool {
            match self {
                Self::Gtk(window) => {
                    window.update_control(index, value);
                    true
                }
                Self::External(window) => {
                    window.update_control(index, value);
                    true
                }
                #[cfg(lv2_host_qt5)]
                Self::Qt5(window) => window.update_control(index, value).is_ok(),
                #[cfg(lv2_host_gtk2)]
                Self::Gtk2(window) => window.update_control(index, value).is_ok(),
            }
        }

        fn idle(&self) {
            if let Self::Gtk(window) = self {
                window.idle();
            }
            if let Self::External(window) = self {
                window.idle();
            }
        }
    }

    enum Command {
        Open(UiOpenRequest, mpsc::SyncSender<Result<(), String>>),
        Close(u64),
        Present(u64),
        UpdateControl(u64, u32, f32),
    }

    static COMMANDS: OnceLock<mpsc::Sender<Command>> = OnceLock::new();
    static NEXT_WINDOW: AtomicU64 = AtomicU64::new(1);

    /// Owns one native UI. Dropping it queues destruction of that window.
    ///
    /// Methods enqueue work on the UI runtime and must be called outside the
    /// audio callback. Updates affect the UI only, not the DSP instance.
    #[must_use = "retain this handle while the UI should remain open"]
    pub struct UiWindow {
        id: u64,
        commands: mpsc::Sender<Command>,
        alive: Arc<AtomicBool>,
        controls: BTreeMap<String, u32>,
    }

    impl UiWindow {
        pub fn is_open(&self) -> bool {
            self.alive.load(Ordering::Acquire)
        }

        /// Queues bringing this window to the foreground.
        pub fn present(&self) -> Result<(), String> {
            self.send(Command::Present(self.id))
        }

        /// Queues a control value notification to the UI, by LV2 port symbol.
        pub fn update_control(&self, symbol: &str, value: f32) -> Result<(), String> {
            let index = self
                .controls
                .get(symbol)
                .ok_or_else(|| format!("control port {symbol:?} is unavailable"))?;
            self.send(Command::UpdateControl(self.id, *index, value))
        }

        /// Queues closing this window and consumes its handle.
        pub fn close(self) {
            drop(self);
        }

        fn send(&self, command: Command) -> Result<(), String> {
            if !self.is_open() {
                return Err("plugin UI is closed".to_owned());
            }
            self.commands
                .send(command)
                .map_err(|_| "plugin UI thread stopped".to_owned())
        }
    }

    impl Drop for UiWindow {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::Release);
            let _ = self.commands.send(Command::Close(self.id));
        }
    }

    #[cfg(test)]
    #[test]
    fn window_commands_are_scoped_and_host_drop_does_not_close_windows() {
        let (commands, receiver) = mpsc::channel();
        let host = UiHost {
            commands: commands.clone(),
        };
        let window = |id| UiWindow {
            id,
            commands: commands.clone(),
            alive: Arc::new(AtomicBool::new(true)),
            controls: BTreeMap::from([("gain".to_owned(), 3)]),
        };
        let first = window(1);
        let second = window(2);
        drop(host);
        assert!(receiver.try_recv().is_err());
        first.update_control("gain", 0.25).unwrap();
        assert!(matches!(
            receiver.recv().unwrap(),
            Command::UpdateControl(1, 3, 0.25)
        ));
        assert!(first.update_control("missing", 1.0).is_err());
        first.close();
        assert!(matches!(receiver.recv().unwrap(), Command::Close(1)));
        assert!(second.is_open());
        second.present().unwrap();
        assert!(matches!(receiver.recv().unwrap(), Command::Present(2)));
        second.alive.store(false, Ordering::Release);
        assert!(second.present().is_err());
        assert!(second.update_control("gain", 0.5).is_err());
    }
    pub struct UiHost {
        commands: mpsc::Sender<Command>,
    }

    impl UiHost {
        pub fn new() -> Self {
            let commands = COMMANDS.get_or_init(|| {
                let (commands, receiver) = mpsc::channel();
                thread::Builder::new()
                    .name("lv2-host-ui".into())
                    .spawn(move || run(receiver))
                    .expect("could not start plugin UI thread");
                commands
            });
            Self {
                commands: commands.clone(),
            }
        }

        /// Opens a native UI for an existing plugin instance.
        pub fn open(&self, instance: UiInstance, options: UiOptions) -> Result<UiWindow, String> {
            let id = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
            let window = UiWindow {
                id,
                commands: self.commands.clone(),
                alive: Arc::new(AtomicBool::new(true)),
                controls: instance
                    .controls
                    .iter()
                    .filter_map(|(index, _)| {
                        instance
                            .ports
                            .get(index)
                            .map(|symbol| (symbol.clone(), *index))
                    })
                    .collect(),
            };
            let request = UiOpenRequest {
                id,
                alive: window.alive.clone(),
                instance,
                options,
            };
            let (reply, result) = mpsc::sync_channel(1);
            self.commands
                .send(Command::Open(request, reply))
                .map_err(|_| "plugin UI thread stopped".to_owned())?;
            result
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "plugin UI thread did not respond".to_owned())??;
            Ok(window)
        }
    }

    fn run(commands: mpsc::Receiver<Command>) {
        let setup = (|| {
            if gtk::is_initialized() {
                return Err("GTK was already initialised on another thread".to_owned());
            }
            // Suil embeds X11 plugin UIs in a GtkPlug, which requires GDK's X11 backend.
            gdk::set_allowed_backends("x11");
            gtk::init()
                .map_err(|error| format!("could not initialise X11 GTK for plugin UIs: {error}"))?;
            let display = gdk::Display::default()
                .ok_or_else(|| "GTK did not open a display for plugin UIs".to_owned())?;
            if !display.backend().is_x11() {
                return Err("native LV2 UIs require an X11 GTK display".to_owned());
            }
            SuilApi::load().map(Rc::new)
        })();
        let Ok(api) = setup else {
            let error = setup.err().unwrap();
            while let Ok(command) = commands.recv() {
                if let Command::Open(_, reply) = command {
                    let _ = reply.send(Err(error.clone()));
                }
            }
            return;
        };

        let windows = Rc::new(RefCell::new(
            BTreeMap::<u64, (Arc<AtomicBool>, HostedUi)>::new(),
        ));
        let loop_windows = windows.clone();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            while let Ok(command) = commands.try_recv() {
                match command {
                    Command::Open(request, reply) => {
                        let alive = request.alive.clone();
                        let result = open_ui(api.clone(), request).map(|(id, window)| {
                            loop_windows.borrow_mut().insert(id, (alive, window));
                        });
                        let _ = reply.send(result);
                    }
                    Command::Close(id) => {
                        loop_windows.borrow_mut().remove(&id);
                    }
                    Command::Present(id) => {
                        if let Some((alive, window)) = loop_windows.borrow_mut().get_mut(&id)
                            && !window.present()
                        {
                            alive.store(false, Ordering::Release);
                        }
                    }
                    Command::UpdateControl(id, index, value) => {
                        if let Some((alive, window)) = loop_windows.borrow_mut().get_mut(&id)
                            && !window.update_control(index, value)
                        {
                            alive.store(false, Ordering::Release);
                        }
                    }
                }
            }
            loop_windows.borrow_mut().retain(|_, (alive, window)| {
                let running = alive.load(Ordering::Acquire) && window.is_running();
                if running {
                    window.idle();
                } else {
                    alive.store(false, Ordering::Release);
                }
                running
            });
            glib::ControlFlow::Continue
        });
        gtk::main();
        windows.borrow_mut().clear();
    }

    fn select_ui(
        api: &SuilApi,
        host_type: &CStr,
        request: &UiOpenRequest,
    ) -> Option<(u32, UiDescriptor, String)> {
        request
            .instance
            .uis
            .iter()
            .flat_map(|ui| ui.classes.iter().map(move |class| (ui, class)))
            .filter_map(|(ui, class)| {
                let class_uri = CString::new(class.as_str()).ok()?;
                let quality = unsafe { (api.ui_supported)(host_type.as_ptr(), class_uri.as_ptr()) };
                (quality > 0).then_some((quality, ui.clone(), class.clone()))
            })
            .min_by_key(|(quality, _, _)| *quality)
    }

    fn open_ui(api: Rc<SuilApi>, request: UiOpenRequest) -> Result<(u64, HostedUi), String> {
        if let Some(selected) = select_ui(&api, GTK3_UI, &request) {
            return open_gtk_window(api, request, selected)
                .map(|(instance_id, window)| (instance_id, HostedUi::Gtk(window)));
        }
        #[cfg(lv2_host_qt5)]
        if let Some((_, ui, ui_type)) = select_ui(&api, QT5_UI, &request) {
            let instance_id = request.id;
            return super::qt5::open(request, ui, ui_type)
                .map(|window| (instance_id, HostedUi::Qt5(window)));
        }
        #[cfg(lv2_host_gtk2)]
        if let Some((_, ui, ui_type)) = select_ui(&api, GTK2_UI, &request) {
            let instance_id = request.id;
            return super::gtk2::open(request, ui, ui_type)
                .map(|window| (instance_id, HostedUi::Gtk2(window)));
        }
        if let Some((_, ui, ui_type)) = select_ui(&api, EXTERNAL_UI, &request) {
            let instance_id = request.id;
            return open_external_window(api, request, ui, ui_type)
                .map(|window| (instance_id, HostedUi::External(window)));
        }
        Err("this plugin has no UI type supported by the available Suil hosts".to_owned())
    }

    fn open_gtk_window(
        api: Rc<SuilApi>,
        request: UiOpenRequest,
        selected: (u32, UiDescriptor, String),
    ) -> Result<(u64, Window), String> {
        let instance_id = request.id;
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&request.options.window_title);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&container);
        window.realize();
        let alive = request.alive.clone();
        window.connect_delete_event(move |window, _| {
            alive.store(false, Ordering::Release);
            window.hide();
            glib::Propagation::Stop
        });

        let controller = Box::new(Controller {
            by_symbol: request
                .instance
                .ports
                .iter()
                .map(|(index, symbol)| (symbol.clone(), *index))
                .collect(),
            by_index: request.instance.ports.clone(),
            port_update: request.options.port_update,
            alive: request.alive.clone(),
        });
        let controller = Box::into_raw(controller);
        let host = unsafe { (api.host_new)(Some(write_port), Some(port_index), None, None) };
        if host.is_null() {
            unsafe { drop(Box::from_raw(controller)) };
            return Err("Suil could not create a UI host".to_owned());
        }

        let parent = Lv2Feature {
            uri: UI_PARENT.as_ptr(),
            data: container.as_ptr() as *mut c_void,
        };
        let instance_access = Lv2Feature {
            uri: INSTANCE_ACCESS.as_ptr(),
            data: request.instance.handle() as *mut c_void,
        };
        let idle = Lv2Feature {
            uri: UI_IDLE.as_ptr(),
            data: ptr::null_mut(),
        };
        let features = [
            &parent as *const Lv2Feature,
            &instance_access,
            &idle,
            ptr::null(),
        ];
        let plugin_uri =
            CString::new(request.instance.plugin_uri.as_str()).map_err(|_| "invalid plugin URI")?;
        let ui_uri = CString::new(selected.1.uri.as_str()).map_err(|_| "invalid UI URI")?;
        let bundle =
            CString::new(selected.1.bundle_path.as_str()).map_err(|_| "invalid UI path")?;
        let binary =
            CString::new(selected.1.binary_path.as_str()).map_err(|_| "invalid UI path")?;
        let ui_type = CString::new(selected.2).map_err(|_| "invalid UI type")?;
        let instance = unsafe {
            (api.instance_new)(
                host,
                controller.cast(),
                GTK3_UI.as_ptr(),
                plugin_uri.as_ptr(),
                ui_uri.as_ptr(),
                ui_type.as_ptr(),
                bundle.as_ptr(),
                binary.as_ptr(),
                features.as_ptr(),
            )
        };
        if instance.is_null() {
            unsafe {
                (api.host_free)(host);
                drop(Box::from_raw(controller));
            }
            return Err("Suil could not instantiate the selected plugin UI".to_owned());
        }
        let widget_ptr = unsafe { (api.instance_get_widget)(instance) };
        if widget_ptr.is_null() {
            unsafe {
                (api.instance_free)(instance);
                (api.host_free)(host);
                drop(Box::from_raw(controller));
            }
            return Err("the plugin UI did not provide a widget".to_owned());
        }
        let widget: gtk::Widget =
            unsafe { glib::translate::from_glib_none(widget_ptr as *mut gtk::ffi::GtkWidget) };
        container.pack_start(&widget, true, true, 0);
        window.show_all();
        window.present();

        for (index, value) in &request.instance.controls {
            unsafe {
                (api.instance_port_event)(
                    instance,
                    *index,
                    std::mem::size_of::<f32>() as u32,
                    0,
                    (value as *const f32).cast(),
                );
            }
        }

        Ok((
            instance_id,
            Window {
                alive: request.alive,
                api,
                _ui_instance: request.instance,
                window,
                container,
                widget,
                host,
                instance,
                controller,
            },
        ))
    }

    fn open_external_window(
        api: Rc<SuilApi>,
        request: UiOpenRequest,
        ui: UiDescriptor,
        ui_type: String,
    ) -> Result<ExternalWindow, String> {
        let plugin_uri =
            CString::new(request.instance.plugin_uri.as_str()).map_err(|_| "invalid plugin URI")?;
        let ui_uri = CString::new(ui.uri.as_str()).map_err(|_| "invalid UI URI")?;
        let bundle = CString::new(ui.bundle_path.as_str()).map_err(|_| "invalid UI path")?;
        let binary = CString::new(ui.binary_path.as_str()).map_err(|_| "invalid UI path")?;
        let ui_type = CString::new(ui_type).map_err(|_| "invalid UI type")?;
        let title = CString::new(request.options.window_title.as_str())
            .map_err(|_| "invalid window title")?;
        let mut external_host = Box::new(ExternalHost {
            ui_closed: Some(external_ui_closed),
            plugin_human_id: title.as_ptr(),
        });
        let controller = Box::into_raw(Box::new(Controller {
            by_symbol: request
                .instance
                .ports
                .iter()
                .map(|(index, symbol)| (symbol.clone(), *index))
                .collect(),
            by_index: request.instance.ports.clone(),
            port_update: request.options.port_update,
            alive: request.alive.clone(),
        }));
        let host = unsafe { (api.host_new)(Some(write_port), Some(port_index), None, None) };
        if host.is_null() {
            unsafe { drop(Box::from_raw(controller)) };
            return Err("Suil could not create an external UI host".to_owned());
        }
        let external_feature = Lv2Feature {
            uri: EXTERNAL_HOST.as_ptr(),
            data: (&mut *external_host as *mut ExternalHost).cast(),
        };
        let instance_access = Lv2Feature {
            uri: INSTANCE_ACCESS.as_ptr(),
            data: request.instance.handle() as *mut c_void,
        };
        let features = [
            &external_feature as *const Lv2Feature,
            &instance_access,
            ptr::null(),
        ];
        let instance = unsafe {
            (api.instance_new)(
                host,
                controller.cast(),
                EXTERNAL_UI.as_ptr(),
                plugin_uri.as_ptr(),
                ui_uri.as_ptr(),
                ui_type.as_ptr(),
                bundle.as_ptr(),
                binary.as_ptr(),
                features.as_ptr(),
            )
        };
        if instance.is_null() {
            unsafe {
                (api.host_free)(host);
                drop(Box::from_raw(controller));
            }
            return Err("Suil could not instantiate the selected external plugin UI".to_owned());
        }
        let widget = unsafe { (api.instance_get_widget)(instance) as *mut ExternalWidget };
        if widget.is_null() {
            unsafe {
                (api.instance_free)(instance);
                (api.host_free)(host);
                drop(Box::from_raw(controller));
            }
            return Err("the external plugin UI did not provide a widget".to_owned());
        }
        let window = ExternalWindow {
            alive: request.alive,
            api,
            _ui_instance: request.instance,
            host,
            instance,
            widget,
            controller,
            _external_host: external_host,
            _title: title,
        };
        for (index, value) in &window._ui_instance.controls {
            window.update_control(*index, *value);
        }
        window.show();
        Ok(window)
    }
}

#[cfg(target_os = "linux")]
pub use platform::{UiHost, UiWindow};

#[cfg(not(target_os = "linux"))]
pub struct UiHost;

#[cfg(not(target_os = "linux"))]
impl UiHost {
    pub fn new() -> Self {
        Self
    }

    pub fn open(&self, _instance: UiInstance, _options: UiOptions) -> Result<UiWindow, String> {
        Err("native LV2 plugin UIs are not implemented on this platform yet".to_owned())
    }
}

#[cfg(not(target_os = "linux"))]
pub struct UiWindow {
    _private: (),
}

#[cfg(not(target_os = "linux"))]
impl UiWindow {
    pub fn is_open(&self) -> bool {
        false
    }
    pub fn present(&self) -> Result<(), String> {
        Err("plugin UI is closed".to_owned())
    }
    pub fn update_control(&self, _symbol: &str, _value: f32) -> Result<(), String> {
        self.present()
    }
    pub fn close(self) {}
}

impl Default for UiHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
