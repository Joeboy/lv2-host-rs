# lv2-host examples

Each example is an independent Cargo project:

- [`native-ui`](./native-ui/) discovers plugins and opens a native plugin UI.
- [`audio-output`](./audio-output/) processes a plugin in a CPAL output callback.

Both use the plugins fetched and built by [`example-plugins`](../example-plugins/).
Run `make -C example-plugins` from the repository root before running either example.
Set `LV2_PATH` to the absolute path of `example-plugins/build` when running them.

See each directory's README for its build and run instructions.
