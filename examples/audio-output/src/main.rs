use cpal::{
    FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use lv2_host::{Host, HostedInstance, ProcessingConfig};

const PLUGIN_URI: &str = "https://joebutton.co.uk/lv2/furelise";
const BLOCK: usize = 256;

struct Renderer {
    instance: HostedInstance,
    audio_output: usize,
    position: usize,
    failed: bool,
}

impl Renderer {
    fn next_sample(&mut self) -> f32 {
        if self.failed {
            return 0.0;
        }
        if self.position == BLOCK {
            self.instance.clear_inputs();
            if let Err(error) = self.instance.run(BLOCK) {
                eprintln!("LV2 processing stopped: {error}");
                self.failed = true;
                return 0.0;
            }
            self.position = 0;
        }
        let sample = self.instance.audio_output(self.audio_output).unwrap()[self.position];
        self.position += 1;
        sample
    }

    fn write<T>(&mut self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        for frame in output.chunks_mut(channels) {
            let value = T::from_sample(self.next_sample());
            frame.fill(value);
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no default audio output device is available".to_owned())?;
    let supported = device
        .default_output_config()
        .map_err(|error| format!("could not read the default output configuration: {error}"))?;
    let config: StreamConfig = supported.into();
    let sample_rate = f64::from(config.sample_rate);
    let channels = usize::from(config.channels);

    let host = Host::new();
    let context = host
        .processing_context(ProcessingConfig {
            sample_rate,
            block_capacity: BLOCK,
            ..ProcessingConfig::default()
        })
        .map_err(|error| error.to_string())?;
    let instance = host
        .plugin(PLUGIN_URI)
        .and_then(|plugin| plugin.instantiate(&context))
        .map_err(|error| error.to_string())?;
    let plugin_name = instance.descriptor().name.clone();
    let audio_output = instance.audio_output_slot("audio_out").ok_or_else(|| {
        format!(
            "{} has no audio output port named {:?}",
            plugin_name, "audio_out"
        )
    })?;

    println!(
        "Playing {} through {} at {sample_rate} Hz",
        plugin_name,
        device
            .id()
            .map_err(|error| format!("could not read output device ID: {error}"))?
    );
    let renderer = Renderer {
        instance,
        audio_output,
        position: BLOCK,
        failed: false,
    };
    let stream = build_stream(
        &device,
        supported.sample_format(),
        config,
        renderer,
        channels,
    )?;
    stream
        .play()
        .map_err(|error| format!("could not start audio output: {error}"))?;

    println!("Press Enter to stop.");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("could not read standard input: {error}"))?;
    drop(stream);
    Ok(())
}

fn build_stream(
    device: &cpal::Device,
    format: SampleFormat,
    config: StreamConfig,
    renderer: Renderer,
    channels: usize,
) -> Result<Stream, String> {
    match format {
        SampleFormat::I8 => output_stream::<i8>(device, config, renderer, channels),
        SampleFormat::I16 => output_stream::<i16>(device, config, renderer, channels),
        SampleFormat::I24 => output_stream::<cpal::I24>(device, config, renderer, channels),
        SampleFormat::I32 => output_stream::<i32>(device, config, renderer, channels),
        SampleFormat::I64 => output_stream::<i64>(device, config, renderer, channels),
        SampleFormat::U8 => output_stream::<u8>(device, config, renderer, channels),
        SampleFormat::U16 => output_stream::<u16>(device, config, renderer, channels),
        SampleFormat::U24 => output_stream::<cpal::U24>(device, config, renderer, channels),
        SampleFormat::U32 => output_stream::<u32>(device, config, renderer, channels),
        SampleFormat::U64 => output_stream::<u64>(device, config, renderer, channels),
        SampleFormat::F32 => output_stream::<f32>(device, config, renderer, channels),
        SampleFormat::F64 => output_stream::<f64>(device, config, renderer, channels),
        other => Err(format!("unsupported output sample format: {other}")),
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
    channels: usize,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| renderer.write(output, channels),
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
        .map_err(|error| format!("could not create audio output stream: {error}"))
}
