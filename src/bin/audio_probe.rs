//! Read-only endpoint inspection, plus an opt-in silent stream smoke test.

#[allow(dead_code)]
#[path = "../audio_haptics.rs"]
mod audio_haptics;

use cpal::traits::{DeviceTrait, HostTrait};
use std::{thread, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    println!(
        "Default audio: {}",
        host.default_output_device()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "none".into())
    );
    for device in host.output_devices()? {
        let name = device.to_string();
        if !name.to_ascii_lowercase().contains("dualsense") {
            continue;
        }
        println!("Controller audio: {name}");
        for config in device.supported_output_configs()? {
            if config.channels() == 4
                && config.min_sample_rate() <= 48_000
                && config.max_sample_rate() >= 48_000
            {
                println!(
                    "  haptic PCM: four channels, 48 kHz, {:?}",
                    config.sample_format()
                );
            }
        }
    }
    let args: Vec<_> = std::env::args().collect();
    if args
        .iter()
        .any(|arg| arg == "--smoke-silent" || arg == "--smoke-loopback")
    {
        let mut audio = audio_haptics::AudioEngine::default();
        println!("Opened: {}", audio.start_output()?);
        if args.iter().any(|arg| arg == "--smoke-loopback") {
            println!("{}", audio.set_bridge(true, 0.0, "gentle")?);
        }
        thread::sleep(Duration::from_secs(2));
        println!("Output callbacks: {}", audio.callbacks());
        if let Some(error) = audio.take_error() {
            return Err(error.into());
        }
        if audio.callbacks() == 0 {
            return Err("Audio device did not request any PCM frames".into());
        }
        audio.stop();
    }
    Ok(())
}
