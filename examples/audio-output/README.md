# lv2-host audio output example

This example loads the
[`furelise-lv2`](https://github.com/Joeboy/furelise-lv2) plugin from
[`example-plugins`](../../example-plugins/), processes it in
the audio callback, and sends its mono output to every channel of CPAL's default
output device.

From this directory, fetch and build the example plugins:

```sh
make -C ../../example-plugins
```

Then run the host example:

```sh
LV2_PATH="$(realpath ../../example-plugins/build)" cargo run --release
```

The example uses the default CPAL host and output device at that device's
default sample rate. Press Enter to stop playback.
