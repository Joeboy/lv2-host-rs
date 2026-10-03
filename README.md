# lv2-host

The aspiration for this project is that it should wrap the considerable amount
of fuss involved in hosting LV2s with GUIs, so developers get to use them with a
straightforward API.

As of now it's more like a dumping ground for LV2 hosting stuff I extracted
while tidying another project. I'll hopefully give it a proper API later.

It's based around [livi](https://github.com/wmedrano/livi-rs). If you don't care
about LV2 UIs you should probably just use that.

Still definitely a Work in Progress. Not tested much, in fact not yet tested
anywhere except my Linux laptop. Windows / Mac testing and support is still
completely TODO.

## Processing API

Discover a plugin, create a context, then instantiate it:

```rust
use lv2_host::{Host, ProcessingConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let host = Host::new();
  let plugin = host.plugin("http://hippie.lt/lv2/gate")?;
  let context = host.processing_context(ProcessingConfig {
      sample_rate: 48_000.0,
      block_capacity: 256,
      ..ProcessingConfig::default()
  })?;
  let mut instance = plugin.instantiate(&context)?;

  instance.set_control_input_by_symbol("switch", 0.0)?;
  instance.audio_input_buffer_mut("input").unwrap()[..128].fill(0.25);
  instance.run(128)?;
  let output = instance.audio_output_buffer("output").unwrap();
  println!("First output sample: {}", output[0]);
  Ok(())
}
```

## UI ownership

`UiHost::open(instance.ui_instance(), options)` returns a `UiWindow`. Keep this
handle while the window should remain open. Dropping it (or calling `close()`)
queues destruction of that window. Dropping a `UiHost` has no effect on windows
it opened. Each call to `open()` creates a separate window; applications can
retain handles by node ID and call `present()` when a user opens one again.

`UiOptions` contains a window title and a `port_update` callback. Capture any
application/node ID in that callback. It receives control-port symbols and
values; send these to your processing thread through your own message queue.
Callbacks run on toolkit/helper threads and should return promptly.

Use `window.update_control("gain", value)` to notify a native UI of a value
changed by your application or obtained from a control output. This updates the
UI only; the application must also send edits to the processing instance.
`present()` and `update_control()` report whether work was queued, not whether
the toolkit has finished it. `is_open()` reflects closure observed by the UI
runtime. Calls belong outside the audio callback. GTK2, GTK3, X11, Qt5, and
KXStudio external UIs support these
operations; toolkit selection remains automatic.

## Build requirements

On Linux, a full build with GTK2, GTK3/X11, and Qt5 UI support requires:

- a Rust toolchain with Cargo;
- `pkg-config`;
- a C compiler and a C++17 compiler such as `g++`;
- Lilv and LV2 development headers;
- Suil development headers and library;
- GTK3 development headers and library;
- GTK2 development headers and library;
- Qt5 Widgets development headers and libraries.

On Debian or Ubuntu these can be installed with:

```sh
sudo apt install \
  build-essential \
  pkg-config \
  liblilv-dev \
  lv2-dev \
  libsuil-dev \
  libgtk-3-dev \
  libgtk2.0-dev \
  qtbase5-dev
```

The resulting application also needs the corresponding Lilv, Suil, GTK3, and Qt5
shared libraries at runtime.

GTK2 and Qt5 UI support are detected by the build script using `pkg-config`.
If their respective development files are unavailable, those UI types are
disabled. GTK3 development files are currently required for Linux builds.
GTK2 and Qt5 UIs run in separate helper processes to keep their toolkit
runtimes isolated from the GTK3 host thread.

## Example projects

[`examples/native-ui`](./examples/native-ui/) lists discovered plugins and can
instantiate a selected plugin and open its native UI. UI windows receive an
opaque `UiInstance` that retains the native instance, plugin metadata, and
connected buffers for as long as the window needs them. No raw native handles or
caller-managed unsafe lifetime contract are required.

[`examples/audio-output`](./examples/audio-output/) runs the bundled Für Elise
LV2 plugin and sends its audio output to the default CPAL output device.

`example-plugins/Makefile` builds the UI integration fixtures:

| LV2 UI type | Example |
| --- | --- |
| GTK2 (`GtkUI`) | AMS LV2 Moog LPF |
| GTK3 (`Gtk3UI`) | Local GTK3 gain fixture |
| Qt5 (`Qt5UI`) | abGate |
| X11 (`X11UI`) | LibreArp |
| KXStudio external UI | LibreArp |

Run `make -C example-plugins test` to build these bundles and exercise their
windows under Xvfb. LibreArp declares both X11 and external UIs; separate tests
exercise both interfaces.
Building the AMS example additionally requires GTKMM 2.4 and FFTW3 development
packages (`libgtkmm-2.4-dev` and `libfftw3-dev` on Debian or Ubuntu).

## AI declaration

Kinda vibecoded.
