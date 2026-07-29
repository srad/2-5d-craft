pub const TICKS_PER_SECOND: u64 = 20;
pub const TICKS_PER_DAY: u64 = 24_000;
pub const SUNRISE_TICKS: u64 = 0;
pub const NOON_TICKS: u64 = 6_000;
pub const SUNSET_TICKS: u64 = 12_000;
pub const MIDNIGHT_TICKS: u64 = 18_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonPhase {
    Full,
    WaningGibbous,
    ThirdQuarter,
    WaningCrescent,
    New,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
}

impl MoonPhase {
    pub const fn index(self) -> usize {
        match self {
            Self::Full => 0,
            Self::WaningGibbous => 1,
            Self::ThirdQuarter => 2,
            Self::WaningCrescent => 3,
            Self::New => 4,
            Self::WaxingCrescent => 5,
            Self::FirstQuarter => 6,
            Self::WaxingGibbous => 7,
        }
    }

    const fn from_day(day: u64) -> Self {
        match day % 8 {
            0 => Self::Full,
            1 => Self::WaningGibbous,
            2 => Self::ThirdQuarter,
            3 => Self::WaningCrescent,
            4 => Self::New,
            5 => Self::WaxingCrescent,
            6 => Self::FirstQuarter,
            _ => Self::WaxingGibbous,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DayCycle {
    day_time_ticks: u64,
    fractional_ticks: f64,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self::from_ticks(SUNRISE_TICKS)
    }
}

impl DayCycle {
    pub const fn from_ticks(day_time_ticks: u64) -> Self {
        Self {
            day_time_ticks,
            fractional_ticks: 0.0,
        }
    }

    pub const fn day_time_ticks(&self) -> u64 {
        self.day_time_ticks
    }

    pub const fn time_of_day_ticks(&self) -> u64 {
        self.day_time_ticks % TICKS_PER_DAY
    }

    pub const fn day(&self) -> u64 {
        self.day_time_ticks / TICKS_PER_DAY
    }

    pub fn phase(&self) -> f32 {
        self.time_of_day_ticks() as f32 / TICKS_PER_DAY as f32
    }

    pub const fn moon_phase(&self) -> MoonPhase {
        MoonPhase::from_day(self.day())
    }

    pub fn sun_elevation(&self) -> f32 {
        let angle = std::f32::consts::TAU * (self.time_of_day_ticks() as f32 - NOON_TICKS as f32)
            / TICKS_PER_DAY as f32;
        let elevation = angle.cos();
        if elevation.abs() < 0.000_001 {
            0.0
        } else {
            elevation
        }
    }

    pub fn daylight(&self) -> f32 {
        smoothstep(-0.10, 0.10, self.sun_elevation())
    }

    pub fn twilight(&self) -> f32 {
        (1.0 - (self.sun_elevation().abs() / 0.22).clamp(0.0, 1.0)) * 0.85
    }

    pub fn night(&self) -> f32 {
        1.0 - smoothstep(-0.18, 0.02, self.sun_elevation())
    }

    pub fn sky_light_level(&self) -> u8 {
        (4.0 + self.daylight() * 11.0).round() as u8
    }

    pub fn advance_seconds(&mut self, delta_seconds: f64) -> bool {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 || self.day_time_ticks == u64::MAX {
            return false;
        }
        let added = delta_seconds * TICKS_PER_SECOND as f64;
        if !added.is_finite() {
            self.day_time_ticks = u64::MAX;
            self.fractional_ticks = 0.0;
            return true;
        }
        let total = self.fractional_ticks + added;
        let whole = total.floor();
        self.fractional_ticks = total - whole;
        if whole < 1.0 {
            return false;
        }
        let increment = if whole >= u64::MAX as f64 {
            u64::MAX
        } else {
            whole as u64
        };
        let previous = self.day_time_ticks;
        self.day_time_ticks = self.day_time_ticks.saturating_add(increment);
        if self.day_time_ticks == u64::MAX {
            self.fractional_ticks = 0.0;
        }
        self.day_time_ticks != previous
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let amount = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_cycle_timing_survives_frame_partitioning() {
        let mut one_step = DayCycle::default();
        let mut many_steps = DayCycle::default();
        assert!(one_step.advance_seconds(1_200.0));
        for _ in 0..72_000 {
            many_steps.advance_seconds(1.0 / 60.0);
        }
        assert_eq!(one_step.day_time_ticks(), TICKS_PER_DAY);
        assert_eq!(many_steps.day_time_ticks(), TICKS_PER_DAY);
    }

    #[test]
    fn named_times_have_expected_light() {
        assert_eq!(DayCycle::from_ticks(SUNRISE_TICKS).sky_light_level(), 10);
        assert_eq!(DayCycle::from_ticks(NOON_TICKS).sky_light_level(), 15);
        assert_eq!(DayCycle::from_ticks(SUNSET_TICKS).sky_light_level(), 10);
        assert_eq!(DayCycle::from_ticks(MIDNIGHT_TICKS).sky_light_level(), 4);
    }

    #[test]
    fn moon_phase_uses_absolute_day() {
        for (day, phase) in [
            MoonPhase::Full,
            MoonPhase::WaningGibbous,
            MoonPhase::ThirdQuarter,
            MoonPhase::WaningCrescent,
            MoonPhase::New,
            MoonPhase::WaxingCrescent,
            MoonPhase::FirstQuarter,
            MoonPhase::WaxingGibbous,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                DayCycle::from_ticks(day as u64 * TICKS_PER_DAY).moon_phase(),
                phase
            );
        }
    }

    #[test]
    fn invalid_deltas_are_ignored_and_overflow_saturates() {
        let mut cycle = DayCycle::from_ticks(u64::MAX - 2);
        assert!(!cycle.advance_seconds(f64::NAN));
        assert!(!cycle.advance_seconds(-1.0));
        assert!(cycle.advance_seconds(1.0));
        assert_eq!(cycle.day_time_ticks(), u64::MAX);
        assert!(!cycle.advance_seconds(1.0));
    }
}
