#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedRng {
    state: u64,
}

impl FixedRng {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub(crate) fn index(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        (self.next_u64() % upper as u64) as usize
    }

    pub(crate) fn range(&mut self, minimum: usize, maximum_inclusive: usize) -> usize {
        minimum + self.index(maximum_inclusive - minimum + 1)
    }

    pub(crate) fn chance(&mut self, probability: f32) -> bool {
        let sample = (self.next_u64() >> 40) as f32 / (1_u32 << 24) as f32;
        sample < probability.clamp(0.0, 1.0)
    }

    pub(crate) fn signed(&mut self, magnitude: i16) -> i16 {
        self.range(0, magnitude.unsigned_abs() as usize * 2) as i16 - magnitude.abs()
    }
}

pub(crate) fn mix_seed(seed: u64, label: &str, variant: u64) -> u64 {
    let mut value = seed ^ variant.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for byte in label.bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
        value ^= value >> 29;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_rng_replays_exactly() {
        let mut first = FixedRng::new(19);
        let mut second = FixedRng::new(19);
        for _ in 0..128 {
            assert_eq!(first.next_u64(), second.next_u64());
        }
    }

    #[test]
    fn mixed_streams_are_distinct() {
        assert_ne!(mix_seed(4, "stone", 0), mix_seed(4, "stone", 1));
        assert_ne!(mix_seed(4, "stone", 0), mix_seed(4, "dirt", 0));
    }
}
