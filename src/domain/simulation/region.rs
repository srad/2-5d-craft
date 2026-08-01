/// A bounded horizontal band that stays simulated regardless of where the player is.
///
/// Ticking areas exist so rules never have to know that the active region currently happens to be
/// one single-player streaming window. A future multiplayer host supplies the union of its player
/// regions through the same provider without touching rule code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickingArea {
    pub center_chunk: i64,
    pub radius: u32,
}

impl TickingArea {
    pub const fn new(center_chunk: i64, radius: u32) -> Self {
        Self {
            center_chunk,
            radius,
        }
    }

    pub fn contains(self, chunk_x: i64) -> bool {
        self.center_chunk.abs_diff(chunk_x) <= u64::from(self.radius)
    }

    /// The chunks this area covers, ascending, skipping coordinates that would overflow.
    pub fn chunks(self) -> impl Iterator<Item = i64> {
        let radius = i64::from(self.radius);
        (-radius..=radius).filter_map(move |offset| self.center_chunk.checked_add(offset))
    }
}

/// Supplies the horizontal chunks the simulation may advance this tick.
///
/// Implementations return a sorted, deduplicated, bounded list so evaluation order can never
/// depend on hash iteration.
pub trait SimulationRegionProvider {
    fn active_chunks(&self) -> Vec<i64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_area_covers_exactly_its_radius() {
        let area = TickingArea::new(4, 2);
        assert_eq!(area.chunks().collect::<Vec<_>>(), vec![2, 3, 4, 5, 6]);
        assert!(area.contains(2));
        assert!(area.contains(6));
        assert!(!area.contains(1));
        assert!(!area.contains(7));
    }

    #[test]
    fn area_chunks_are_overflow_safe() {
        assert_eq!(
            TickingArea::new(i64::MAX, 2).chunks().collect::<Vec<_>>(),
            vec![i64::MAX - 2, i64::MAX - 1, i64::MAX]
        );
        assert_eq!(
            TickingArea::new(i64::MIN, 1).chunks().collect::<Vec<_>>(),
            vec![i64::MIN, i64::MIN + 1]
        );
    }
}
