use lv2_host::{Host, ProcessingConfig, UiHost, UiOptions};
use std::sync::Arc;

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_BLOCK_LENGTH: usize = 256;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let host = Host::new();
    let plugins = host.plugins();

    let Some(uri) = std::env::args().nth(1) else {
        println!("Discovered {} LV2 plugins:", plugins.len());
        for plugin in plugins.values() {
            println!(
                "- {}\n  {}\n  {} ports, {} UIs",
                plugin.name,
                plugin.uri,
                plugin.ports.len(),
                plugin.uis.len()
            );
        }
        println!("\nPass a plugin URI to instantiate it and open its native UI.");
        return Ok(());
    };

    let descriptor = plugins
        .get(&uri)
        .ok_or_else(|| format!("no plugin with URI {uri:?} was discovered"))?;
    if descriptor.uis.is_empty() {
        return Err(format!("{} has no discoverable native UI", descriptor.name));
    }

    let context = host
        .processing_context(ProcessingConfig {
            sample_rate: SAMPLE_RATE,
            block_capacity: MAX_BLOCK_LENGTH,
            ..ProcessingConfig::default()
        })
        .map_err(|error| error.to_string())?;
    let instance = host
        .plugin(&uri)
        .and_then(|plugin| plugin.instantiate(&context))
        .map_err(|error| format!("could not instantiate {}: {error}", descriptor.name))?;

    let ui_host = UiHost::new();

    let window = ui_host.open(
        instance.ui_instance(),
        UiOptions {
            window_title: format!("{} — lv2-host example", descriptor.name),
            port_update: Arc::new(|update| {
                println!("UI update: port={} value={}", update.port, update.value);
            }),
        },
    )?;

    println!("Opened {}. Press Enter to close it.", descriptor.name);
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("could not read standard input: {error}"))?;

    window.close();
    drop(ui_host);
    drop(instance);
    Ok(())
}
