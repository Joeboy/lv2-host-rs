# lv2-host native UI example

Standalone Rust / Cargo project that demonstrates plugin metadata discovery and
native UI hosting with [lv2-host](../..).

From this directory, fetch and build the example plugins, then list them:

```sh
make -C ../../example-plugins
LV2_PATH="$(realpath ../../example-plugins/build)" cargo run
```

Pass a discovered plugin URI to instantiate that plugin and open its native UI:

```sh
LV2_PATH="$(realpath ../../example-plugins/build)" cargo run -- https://librearp.gitlab.io
```

The example retains the `UiWindow` returned by `open()` until Enter is pressed.
Closing or dropping that handle closes its window. The window retains the LV2
instance for its lifetime and the example prints control-port writes received
from the native UI. It does not process audio; applications remain responsible
for connecting ports and running instances.
