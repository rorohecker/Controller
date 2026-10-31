pub fn average(
    data: &[u8],
    width: i32,
    height: i32,
    bytes_per_pixel: usize,
    rgb: [usize; 3],
) -> Result<[u8; 3], String> {
    if !(1..=16384).contains(&width)
        || !(1..=16384).contains(&height)
        || !(3..=4).contains(&bytes_per_pixel)
        || rgb.iter().any(|&i| i >= bytes_per_pixel)
        || data.len() > 64 * 1024 * 1024
    {
        return Err("Invalid display frame dimensions or pixel format.".into());
    }
    let (width, height) = (width as usize, height as usize);
    let stride = data.len() / height;
    if !data.len().is_multiple_of(height) || stride < width * bytes_per_pixel {
        return Err("Incomplete display frame.".into());
    }
    let mut total = [0u64; 3];
    for y in 0..14 {
        for x in 0..24 {
            let offset =
                (height * (2 * y + 1) / 28) * stride + (width * (2 * x + 1) / 48) * bytes_per_pixel;
            for c in 0..3 {
                total[c] += u64::from(data[offset + rgb[c]]);
            }
        }
    }
    Ok(total.map(|v| (v / 336) as u8))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels_padding_and_malformed_frames() {
        assert_eq!(
            average(&[10, 20, 30, 255, 222, 222, 222, 222], 1, 1, 4, [2, 1, 0]).unwrap(),
            [30, 20, 10]
        );
        assert_eq!(average(&[9, 8, 7], 1, 1, 3, [0, 1, 2]).unwrap(), [9, 8, 7]);
        for (bytes, w, h, bpp, map) in [
            (&[][..], 0, 1, 4, [2, 1, 0]),
            (&[0][..], 2, 1, 4, [2, 1, 0]),
            (&[0; 8][..], 1, 3, 4, [2, 1, 0]),
            (&[0; 4][..], 1, 1, 4, [4, 1, 0]),
        ] {
            assert!(average(bytes, w, h, bpp, map).is_err());
        }
    }
}
