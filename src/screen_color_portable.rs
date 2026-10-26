pub struct ScreenSampler;

impl ScreenSampler {
    pub fn new() -> Self {
        Self
    }
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn take(&self) -> Option<Result<[u8; 3], String>> {
        None
    }
}

pub fn smooth(previous: Option<[u8; 3]>, current: [u8; 3]) -> [u8; 3] {
    let Some(previous) = previous else {
        return current;
    };
    std::array::from_fn(|index| {
        ((u16::from(previous[index]) * 3 + u16::from(current[index])) / 4) as u8
    })
}
