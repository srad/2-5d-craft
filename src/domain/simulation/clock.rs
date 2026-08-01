/// Logical simulation rate.
///
/// This is deliberately separate from [`crate::domain::TICKS_PER_SECOND`], which paces the
/// day/night presentation clock. The two happen to share a value; changing simulation speed must
/// never change the day cycle, Avian's fixed physics schedule, or player movement.
pub const SIMULATION_TICKS_PER_SECOND: u64 = 20;

/// Logical steps a single rendered frame may execute.
pub const MAX_STEPS_PER_FRAME: u32 = 4;

/// Wall-time the accumulator may hold.
///
/// Equal to `MAX_STEPS_PER_FRAME` step durations, so sustained overload slows game time instead
/// of skipping logical ticks or attempting unbounded catch-up.
pub const MAX_ACCUMULATED_SECONDS: f64 = 0.200;

pub const SECONDS_PER_STEP: f64 = 1.0 / SIMULATION_TICKS_PER_SECOND as f64;

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const NANOS_PER_STEP: u64 = NANOS_PER_SECOND / SIMULATION_TICKS_PER_SECOND;
const MAX_ACCUMULATED_NANOS: u64 = MAX_STEPS_PER_FRAME as u64 * NANOS_PER_STEP;

/// Converts rendered frame time into whole logical simulation steps.
///
/// The clock owns no world state: it only decides how many steps a frame may run. `world_tick`
/// itself belongs to the authoritative world, because it must be saved and reloaded.
///
/// Time accumulates in whole nanoseconds. Repeatedly adding and subtracting `f64` seconds drifts —
/// three exact 50 ms frames can floor to two steps — and a clock that silently loses ticks is not
/// a deterministic reference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SimulationClock {
    accumulated_nanos: u64,
}

impl SimulationClock {
    /// Adds one frame of virtual time. Non-finite and non-positive deltas are ignored, and the
    /// total is clamped so a stalled frame cannot buy a catch-up burst.
    pub fn accumulate(&mut self, delta_seconds: f64) {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return;
        }
        let nanos = (delta_seconds * NANOS_PER_SECOND as f64).round();
        let nanos = if nanos >= MAX_ACCUMULATED_NANOS as f64 {
            MAX_ACCUMULATED_NANOS
        } else {
            nanos as u64
        };
        self.accumulated_nanos = self
            .accumulated_nanos
            .saturating_add(nanos)
            .min(MAX_ACCUMULATED_NANOS);
    }

    /// Removes and returns the whole steps this frame may execute.
    pub fn take_steps(&mut self) -> u32 {
        let steps = (self.accumulated_nanos / NANOS_PER_STEP).min(u64::from(MAX_STEPS_PER_FRAME));
        self.accumulated_nanos -= steps * NANOS_PER_STEP;
        steps as u32
    }

    /// Time held back for the next frame, in nanoseconds.
    pub fn accumulated_nanos(&self) -> u64 {
        self.accumulated_nanos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(deltas: impl IntoIterator<Item = f64>) -> (SimulationClock, u32) {
        let mut clock = SimulationClock::default();
        let mut total = 0;
        for delta in deltas {
            clock.accumulate(delta);
            total += clock.take_steps();
        }
        (clock, total)
    }

    #[test]
    fn frame_partitioning_does_not_change_logical_tick_totals() {
        let (_, many) = run(std::iter::repeat_n(1.0 / 60.0, 60));
        let (_, few) = run(std::iter::repeat_n(0.05, 20));
        assert_eq!(many, SIMULATION_TICKS_PER_SECOND as u32);
        assert_eq!(few, SIMULATION_TICKS_PER_SECOND as u32);
    }

    #[test]
    fn exact_step_multiples_never_lose_a_tick_to_rounding() {
        for steps in 1..=MAX_STEPS_PER_FRAME {
            let (clock, taken) = run([SECONDS_PER_STEP * f64::from(steps)]);
            assert_eq!(taken, steps);
            assert_eq!(clock.accumulated_nanos(), 0);
        }
    }

    #[test]
    fn a_frame_never_runs_more_than_the_step_budget() {
        let (clock, steps) = run([5.0]);
        assert_eq!(steps, MAX_STEPS_PER_FRAME);
        // The clamp discards the rest: game time slows rather than catching up.
        assert_eq!(clock.accumulated_nanos(), 0);
    }

    #[test]
    fn sustained_overload_never_accumulates_a_backlog() {
        let (clock, steps) = run(std::iter::repeat_n(1.0, 10));
        assert_eq!(steps, MAX_STEPS_PER_FRAME * 10);
        assert_eq!(clock.accumulated_nanos(), 0);
    }

    #[test]
    fn invalid_and_partial_deltas_advance_nothing() {
        let mut clock = SimulationClock::default();
        for delta in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            clock.accumulate(delta);
        }
        assert_eq!(clock.accumulated_nanos(), 0);
        clock.accumulate(0.049);
        assert_eq!(clock.take_steps(), 0);
        clock.accumulate(0.001);
        assert_eq!(clock.take_steps(), 1);
        assert_eq!(clock.take_steps(), 0);
    }

    #[test]
    fn the_accumulator_never_exceeds_its_cap() {
        let mut clock = SimulationClock::default();
        clock.accumulate(f64::MAX);
        assert_eq!(clock.accumulated_nanos(), MAX_ACCUMULATED_NANOS);
        assert_eq!(
            MAX_ACCUMULATED_NANOS as f64 / NANOS_PER_SECOND as f64,
            MAX_ACCUMULATED_SECONDS
        );
    }
}
