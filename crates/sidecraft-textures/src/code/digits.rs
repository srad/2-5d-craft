//! Mixed-radix accumulator over an arbitrary-precision integer.
//!
//! Packing each field into a whole number of bits wastes space whenever a cardinality is not a
//! power of two — 19 global controls cost 76 bits that way against 68.2 packed by radix. Folding
//! them into one integer instead (`accumulator = accumulator * cardinality + index`) spends
//! exactly `log2(product of cardinalities)` bits, and the same integer is what base36 renders.

pub const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Little-endian `u32` limbs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Digits(Vec<u32>);

impl Digits {
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|limb| *limb == 0)
    }

    /// `self = self * multiplier + addend`, where both fit in a `u32`.
    pub fn mul_add(&mut self, multiplier: u64, addend: u64) {
        debug_assert!(multiplier <= u64::from(u32::MAX));
        debug_assert!(addend < multiplier.max(1));
        let mut carry = addend;
        for limb in &mut self.0 {
            let product = u64::from(*limb) * multiplier + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        while carry > 0 {
            self.0.push(carry as u32);
            carry >>= 32;
        }
    }

    /// `self /= divisor`, returning the remainder.
    pub fn div_rem(&mut self, divisor: u64) -> u64 {
        debug_assert!(divisor > 0 && divisor <= u64::from(u32::MAX));
        let mut remainder = 0u64;
        for limb in self.0.iter_mut().rev() {
            let value = (remainder << 32) | u64::from(*limb);
            *limb = (value / divisor) as u32;
            remainder = value % divisor;
        }
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
        remainder
    }

    pub fn to_base36(&self) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let mut value = self.clone();
        let mut text = Vec::new();
        while !value.is_zero() {
            text.push(ALPHABET[value.div_rem(36) as usize]);
        }
        text.reverse();
        String::from_utf8(text).expect("alphabet is ascii")
    }

    pub fn from_base36(text: &str) -> Option<Self> {
        let mut value = Self::default();
        for character in text.chars() {
            let digit = ALPHABET
                .iter()
                .position(|candidate| *candidate == character as u8)?;
            value.mul_add(36, digit as u64);
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base36_round_trips_across_limb_boundaries() {
        for seed in [0u64, 1, 35, 36, u64::from(u32::MAX), u64::MAX] {
            let mut value = Digits::default();
            for byte in seed.to_be_bytes() {
                value.mul_add(256, u64::from(byte));
            }
            let text = value.to_base36();
            assert_eq!(Digits::from_base36(&text), Some(value), "seed {seed}");
        }
    }

    #[test]
    fn mixed_radix_pushes_and_pops_in_reverse() {
        let fields = [(3u64, 5u64), (0, 2), (44, 45), (85, 86), (7, 9)];
        let mut value = Digits::default();
        for (index, cardinality) in fields {
            value.mul_add(cardinality, index);
        }
        let mut recovered = Vec::new();
        for (_, cardinality) in fields.iter().rev() {
            recovered.push(value.div_rem(*cardinality));
        }
        recovered.reverse();
        assert_eq!(
            recovered,
            fields.iter().map(|(index, _)| *index).collect::<Vec<_>>()
        );
        assert!(value.is_zero());
    }

    #[test]
    fn a_leading_field_keeps_the_value_unambiguous() {
        // Version is pushed first and is never zero, so no encoding starts with a run of zero
        // digits that base36 would drop.
        let mut value = Digits::default();
        value.mul_add(8, 1);
        value.mul_add(256, 0);
        assert!(!value.to_base36().starts_with('0'));
    }
}
