use aftertouch_protocol as protocol;
use hidapi::{BusType, HidApi, HidDevice};
use std::{
    thread,
    time::{Duration, Instant},
};

#[allow(dead_code)]
#[path = "../audio_haptics.rs"]
mod audio_haptics;

fn gyro_spread(
    device: &HidDevice,
    duration: Duration,
    refresh_audio: bool,
) -> Result<f64, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut samples = Vec::new();
    let mut last_refresh = start;
    while start.elapsed() < duration {
        if refresh_audio && last_refresh.elapsed() >= Duration::from_millis(80) {
            device.write(&protocol::audio_haptics_enable_report())?;
            last_refresh = Instant::now();
        }
        let mut bytes = [0u8; 78];
        let length = device.read_timeout(&mut bytes, 20)?;
        if let Some(state) = protocol::parse_input(&bytes[..length]) {
            samples.push(state.gyro.map(f64::from));
        }
    }
    if samples.len() < 2 {
        return Ok(0.0);
    }
    let means = std::array::from_fn::<_, 3, _>(|axis| {
        samples.iter().map(|sample| sample[axis]).sum::<f64>() / samples.len() as f64
    });
    let variance = samples
        .iter()
        .map(|sample| {
            (0..3)
                .map(|axis| (sample[axis] - means[axis]).powi(2))
                .sum::<f64>()
        })
        .sum::<f64>()
        / samples.len() as f64;
    Ok(variance.sqrt())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let smoke_output = std::env::args().any(|arg| arg == "--smoke-output");
    let speaker_test = std::env::args().any(|arg| arg == "--speaker-test");
    let grip_test = std::env::args().any(|arg| arg == "--grip-test");
    let api = HidApi::new()?;
    for info in api
        .device_list()
        .filter(|d| d.vendor_id() == 0x054c && matches!(d.product_id(), 0x0ce6 | 0x0df2))
    {
        println!(
            "{:04x}:{:04x} {:?} usage {:04x}:{:04x} interface {} product {:?} path {}",
            info.vendor_id(),
            info.product_id(),
            info.bus_type(),
            info.usage_page(),
            info.usage(),
            info.interface_number(),
            info.product_string(),
            info.path().to_string_lossy()
        );
        if info.usage_page() == 1 && info.usage() == 5 {
            match info.open_device(&api) {
                Ok(device) => {
                    println!("  open: ok");
                    let mut bytes = [0u8; 78];
                    match device.read_timeout(&mut bytes, 300) {
                        Ok(length) if length > 0 => {
                            let parsed = protocol::parse_input(&bytes[..length]);
                            println!(
                                "  input: {} bytes, report {:02x}, parsed {}, headphone jack {}",
                                length,
                                bytes[0],
                                parsed.is_some(),
                                parsed.is_some_and(|state| state.battery_info.headphone_jack)
                            );
                        }
                        Ok(_) => println!("  input: no report within 300 ms"),
                        Err(error) => println!("  input: read error: {error}"),
                    }
                    if smoke_output && matches!(info.bus_type(), BusType::Usb) {
                        let off = protocol::encode_trigger(&protocol::TriggerSetting {
                            mode: "off".into(),
                            strength: 0.0,
                            curve: vec![0.0; 11],
                            ..protocol::TriggerSetting::default()
                        })?;
                        let report = protocol::usb_report(
                            0x0c,
                            0,
                            0,
                            Some(off),
                            Some(off),
                            None,
                            None,
                            false,
                        );
                        println!("  neutral trigger write: {} bytes", device.write(&report)?);
                    }
                    if (speaker_test || grip_test) && matches!(info.bus_type(), BusType::Usb) {
                        let mut audio = audio_haptics::AudioEngine::default();
                        println!("  audio endpoint: {}", audio.start_output()?);
                        println!(
                            "  clear audio mute: {} bytes",
                            device.write(&protocol::audio_haptics_enable_report())?
                        );
                        if speaker_test {
                            println!(
                                "  speaker route write: {} bytes",
                                device.write(&protocol::speaker_route_report(true))?
                            );
                            for _ in 0..4 {
                                audio.play_speaker_chirp()?;
                                thread::sleep(Duration::from_millis(950));
                            }
                            println!("  speaker callbacks: {}", audio.callbacks());
                            println!("  audible PCM frames: {}", audio.speaker_frames());
                            println!(
                                "  restore audio route write: {} bytes",
                                device.write(&protocol::speaker_route_report(false))?
                            );
                        }
                        if grip_test {
                            let baseline = gyro_spread(&device, Duration::from_millis(700), false)?;
                            println!("  grip gyro baseline: {baseline:.1}");
                            for index in 0..4 {
                                audio.play(audio_haptics::VoiceClip::new(
                                    "square", 90, 2, 500, 100, 0,
                                ))?;
                                let spread =
                                    gyro_spread(&device, Duration::from_millis(650), index >= 2)?;
                                println!(
                                    "  grip gyro pulse {}: {spread:.1}{}",
                                    index + 1,
                                    if index >= 2 { " (route refreshed)" } else { "" }
                                );
                            }
                            println!("  grip callbacks: {}", audio.callbacks());
                            println!("  grip PCM frames: {}", audio.grip_frames());
                        }
                        if let Some(error) = audio.take_error() {
                            return Err(error.into());
                        }
                        audio.stop();
                    }
                }
                Err(error) => println!("  open: failed: {error}"),
            }
        }
    }
    let _ = BusType::Usb; // Keep the transport enum explicit in probe builds.
    Ok(())
}
