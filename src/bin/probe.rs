use aftertouch_protocol as protocol;
use hidapi::{BusType, HidApi};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let smoke_output = std::env::args().any(|arg| arg == "--smoke-output");
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
                        Ok(length) if length > 0 => println!(
                            "  input: {} bytes, report {:02x}, parsed {}",
                            length,
                            bytes[0],
                            protocol::parse_input(&bytes[..length]).is_some()
                        ),
                        Ok(_) => println!("  input: no report within 300 ms"),
                        Err(error) => println!("  input: read error: {error}"),
                    }
                    if smoke_output && matches!(info.bus_type(), BusType::Usb) {
                        let off = protocol::encode_trigger(&protocol::TriggerSetting {
                            mode: "off".into(),
                            strength: 0.0,
                            curve: vec![0.0; 11],
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
                }
                Err(error) => println!("  open: failed: {error}"),
            }
        }
    }
    let _ = BusType::Usb; // Keep the transport enum explicit in probe builds.
    Ok(())
}
