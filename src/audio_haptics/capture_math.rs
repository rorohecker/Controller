//! Format and downmix rules shared by the capture backends.
pub fn validate(channels: u16, rate: u32, native: bool) -> Result<(), String> {
    if !(1..=32).contains(&channels) || !(8_000..=192_000).contains(&rate) {
        return Err("Audio capture requires 1-32 channels at 8-192 kHz".into());
    }
    if native && (channels != 4 || rate != 48_000) {
        return Err("Native relay requires a four-channel, 48 kHz source".into());
    }
    Ok(())
}
pub fn stereo(frame: &[f32], native: bool) -> [f32; 2] {
    fn sample(v: f32) -> f32 {
        if v.is_finite() {
            v.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
    if native {
        return [
            frame.get(2).copied().map_or(0.0, sample),
            frame.get(3).copied().map_or(0.0, sample),
        ];
    }
    let Some(left) = frame.first().copied() else {
        return [0.; 2];
    };
    let left = sample(left);
    let right = frame.get(1).copied().map_or(left, sample);
    if frame.len() <= 2 {
        return [left, right];
    }
    // CPAL does not expose a portable channel map. Preserve the front pair and
    // fold remaining channels equally into both grips, without assuming LFE order.
    let surround = frame[2..].iter().copied().map(sample).sum::<f32>() / (frame.len() - 2) as f32;
    [
        (left + 0.5 * surround) / 1.5,
        (right + 0.5 * surround) / 1.5,
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_are_finite_and_surround_channels_participate() {
        assert_eq!(stereo(&[0.4], false), [0.4; 2]);
        assert_eq!(stereo(&[0.2, -0.2], false), [0.2, -0.2]);
        assert_eq!(stereo(&[f32::NAN, f32::INFINITY], false), [0.; 2]);
        assert!(
            stereo(&[0., 0., 1., 1., 1., 1.], false)
                .iter()
                .all(|v| *v > 0.3 && *v < 0.34)
        );
        assert_eq!(stereo(&[1., 1., 0.3, -0.3], true), [0.3, -0.3]);
        for channels in 1..=32 {
            assert!(validate(channels, 48000, false).is_ok());
            assert!(
                stereo(&vec![1.; channels as usize], false)
                    .iter()
                    .all(|v| *v <= 1.)
            );
        }
        assert!(validate(0, 48000, false).is_err());
        assert!(validate(2, 0, false).is_err());
        assert!(validate(2, 48000, true).is_err());
    }
}
