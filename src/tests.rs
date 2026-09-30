use super::*;

#[test]
fn processing_context_owns_configuration_and_instances_outlive_host() {
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example-plugins/build/abGate.lv2")
        .canonicalize()
        .unwrap();
    let host = Host::with_load_bundle(&format!("file://{}/", bundle.display()));
    for sample_rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            host.processing_context(ProcessingConfig {
                sample_rate,
                ..ProcessingConfig::default()
            }),
            Err(Error::InvalidSampleRate { .. })
        ));
    }
    assert!(matches!(
        host.processing_context(ProcessingConfig {
            block_capacity: 0,
            ..ProcessingConfig::default()
        }),
        Err(Error::InvalidConfiguration { .. })
    ));
    assert!(matches!(
        host.processing_context(ProcessingConfig {
            atom_sequence_capacity: 0,
            ..ProcessingConfig::default()
        }),
        Err(Error::InvalidConfiguration { .. })
    ));
    let context = host
        .processing_context(ProcessingConfig {
            sample_rate: 44_100.0,
            block_capacity: 32,
            ..ProcessingConfig::default()
        })
        .unwrap();
    let plugin = host.plugin("http://hippie.lt/lv2/gate").unwrap();
    let mut instance = plugin.instantiate(&context).unwrap();
    let mut sibling = plugin.instantiate(&context.clone()).unwrap();
    assert!(std::ptr::eq(plugin.descriptor(), instance.descriptor()));
    let separate_context = host
        .processing_context(ProcessingConfig::default())
        .unwrap();
    let mut unrelated = plugin.instantiate(&separate_context).unwrap();
    assert!(matches!(
        instance.copy_atom_output_to(0, &mut unrelated, 0),
        Err(Error::IncompatibleContexts)
    ));
    // A clone is compatible: it reaches port validation instead.
    assert!(matches!(
        instance.copy_atom_output_to(0, &mut sibling, 0),
        Err(Error::InvalidAtomInput { .. })
    ));
    drop(plugin);
    drop(host);
    drop(context);
    assert_eq!(instance.context().config().sample_rate, 44_100.0);
    let input = instance.audio_input_slot("input").unwrap();
    let output = instance.audio_output_slot("output").unwrap();
    assert!(instance.audio_input_mut(usize::MAX).is_none());
    assert!(instance.audio_output_buffer("input").is_none());
    assert_eq!(instance.audio_input_mut(input).unwrap().len(), 32);
    instance.audio_input_mut(input).unwrap().fill(0.5);
    instance.set_control_input_by_symbol("switch", 0.0).unwrap();
    instance.run(17).unwrap();
    for sample in &instance.audio_output(output).unwrap()[..17] {
        assert!((*sample - 0.5).abs() < 1e-6);
    }
    assert!(matches!(
        instance.run(33),
        Err(Error::InvalidBlockSize { capacity: 32, .. })
    ));
    assert!(matches!(
        instance.run(0),
        Err(Error::InvalidBlockSize { .. })
    ));
}

#[test]
fn discovers_native_and_external_uis() {
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example-plugins/build/LibreArp.lv2")
        .canonicalize()
        .unwrap();
    let lv2 = Host::with_load_bundle(&format!("file://{}/", bundle.display()));
    let plugin = lv2.plugin("https://librearp.gitlab.io").unwrap();
    let descriptor = &lv2.plugins()["https://librearp.gitlab.io"];
    let uis = discover_uis(&plugin.native);

    assert_eq!(descriptor.uri, "https://librearp.gitlab.io");
    assert!(!descriptor.name.is_empty());
    assert!(!descriptor.classes.is_empty());
    assert!(!descriptor.ports.is_empty());
    assert_eq!(descriptor.uis, uis);
    assert!(uis.iter().any(|ui| {
        ui.classes
            .iter()
            .any(|class| class == "http://lv2plug.in/ns/extensions/ui#X11UI")
    }));
    assert!(uis.iter().all(|ui| !ui.binary_path.is_empty()));
}

#[cfg(target_os = "linux")]
#[test]
fn opens_librearp_ui_in_isolated_host_runtime() {
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example-plugins/build/LibreArp.lv2")
        .canonicalize()
        .unwrap();
    let instance = {
        let lv2 = Host::with_load_bundle(&format!("file://{}/", bundle.display()));
        let context = lv2.processing_context(ProcessingConfig::default()).unwrap();
        lv2.plugin("https://librearp.gitlab.io")
            .unwrap()
            .instantiate(&context)
            .unwrap()
    };
    let ui_instance = instance.ui_instance();
    drop(instance);
    let ui_host = UiHost::new();
    let second_instance = ui_instance.clone();

    let window = ui_host
        .open(
            ui_instance,
            UiOptions {
                window_title: "LibreArp".to_owned(),
                port_update: Arc::new(|_| {}),
            },
        )
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(250));
    let second_host = UiHost::new();
    let second = second_host
        .open(
            second_instance,
            UiOptions {
                window_title: "LibreArp second window".to_owned(),
                port_update: Arc::new(|_| {}),
            },
        )
        .unwrap();
    drop(ui_host);
    assert!(window.is_open());
    window.present().unwrap();
    window.close();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(second.is_open());
    second.present().unwrap();
    second.close();
    std::thread::sleep(std::time::Duration::from_millis(100));
}
