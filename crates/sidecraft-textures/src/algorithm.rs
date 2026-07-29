use crate::{
    GenerateOptions, OrePattern, PatternAlgorithm, PlacementAlgorithm, QualityPreset,
    config::ClusterShape, rng::FixedRng,
};

pub(crate) const SIDE: i32 = 16;
pub(crate) const PIXELS: usize = (SIDE * SIDE) as usize;
pub(crate) const CARDINALS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtifactMetrics {
    pub periodicity: f32,
    pub orientation_imbalance: f32,
    pub isolated_ratio: f32,
    pub score: f32,
}

pub(crate) fn best_pattern(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    let threshold = match options.quality {
        QualityPreset::Relaxed => 1.15,
        QualityPreset::Balanced => 0.86,
        QualityPreset::Strict => 0.68,
    };
    let mut best = [0_u8; PIXELS];
    let mut best_score = f32::INFINITY;
    for _ in 0..32 {
        let mut candidate = generate_pattern(rng, options);
        for _ in 0..options.smoothing_passes {
            candidate = cellular_cleanup(candidate);
        }
        let metrics = artifact_metrics(&candidate);
        if metrics.score < best_score {
            best = candidate;
            best_score = metrics.score;
        }
        if metrics.score <= threshold {
            return candidate;
        }
    }
    best
}

fn generate_pattern(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    match options.pattern {
        PatternAlgorithm::ClusterStamps => cluster_stamps(rng, options),
        PatternAlgorithm::CellularClumps => cellular_clumps(rng, options),
        PatternAlgorithm::BrokenStrata => broken_strata(rng, options),
        PatternAlgorithm::ShortWalks => short_walks(rng, options),
    }
}

fn seed_points(rng: &mut FixedRng, options: &GenerateOptions, count: usize) -> Vec<(i32, i32)> {
    match options.placement {
        PlacementAlgorithm::Uniform => (0..count)
            .map(|_| (rng.index(16) as i32, rng.index(16) as i32))
            .collect(),
        PlacementAlgorithm::JitteredGrid => {
            let columns = (count as f32).sqrt().ceil().max(1.0) as usize;
            let step = (16 / columns.max(1)).max(2);
            (0..count)
                .map(|index| {
                    let cell_x = index % columns;
                    let cell_y = index / columns;
                    (
                        (cell_x * step + rng.index(step)).min(15) as i32,
                        (cell_y * step + rng.index(step)).min(15) as i32,
                    )
                })
                .collect()
        }
        PlacementAlgorithm::PoissonDisc => {
            let mut points = Vec::with_capacity(count);
            for _ in 0..count * 24 {
                let candidate = (rng.index(16) as i32, rng.index(16) as i32);
                if points.iter().all(|&(x, y)| {
                    let dx = x - candidate.0;
                    let dy = y - candidate.1;
                    dx * dx + dy * dy >= 9
                }) {
                    points.push(candidate);
                    if points.len() == count {
                        break;
                    }
                }
            }
            while points.len() < count {
                points.push((rng.index(16) as i32, rng.index(16) as i32));
            }
            points
        }
    }
}

fn cluster_stamps(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    const POLYOMINOES: &[&[(i32, i32)]] = &[
        &[(0, 0), (1, 0)],
        &[(0, 0), (1, 0), (2, 0)],
        &[(0, 0), (0, 1), (1, 1)],
        &[(0, 0), (1, 0), (0, 1), (1, 1)],
        &[(0, 0), (0, 1), (0, 2), (1, 2)],
        &[(0, 0), (1, 0), (2, 0), (1, 1)],
        &[(0, 0), (1, 0), (1, 1), (2, 1)],
    ];
    let target = (options.cluster_density * PIXELS as f32) as usize;
    let count = (target / options.cluster_size.max(1)).max(4);
    let points = seed_points(rng, options, count);
    let mut output = [0_u8; PIXELS];
    for (origin_x, origin_y) in points {
        let shape = match options.cluster_shape {
            ClusterShape::Rectangular => {
                if rng.chance(0.5) {
                    POLYOMINOES[0]
                } else {
                    POLYOMINOES[3]
                }
            }
            ClusterShape::Polyomino | ClusterShape::Mixed => {
                POLYOMINOES[rng.index(POLYOMINOES.len())]
            }
        };
        let transpose = rng.chance(0.5);
        for &(shape_x, shape_y) in shape {
            let (shape_x, shape_y) = if transpose {
                (shape_y, shape_x)
            } else {
                (shape_x, shape_y)
            };
            set_index(
                &mut output,
                origin_x + shape_x,
                origin_y + shape_y,
                if rng.chance(0.22) { 2 } else { 1 },
            );
        }
    }
    output
}

fn cellular_clumps(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    let mut output = [0_u8; PIXELS];
    let seeds = ((options.cluster_density * 48.0) as usize).clamp(4, 14);
    let target = (options.cluster_density * PIXELS as f32) as usize;
    let mut frontier = Vec::new();
    for point in seed_points(rng, options, seeds) {
        set_index(&mut output, point.0, point.1, 1);
        frontier.push(point);
    }
    while nonzero_count(&output) < target && !frontier.is_empty() {
        let origin = frontier[rng.index(frontier.len())];
        let (dx, dy) = CARDINALS[rng.index(CARDINALS.len())];
        let next = (origin.0 + dx, origin.1 + dy);
        if inside(next.0, next.1) {
            set_index(
                &mut output,
                next.0,
                next.1,
                if rng.chance(0.16) { 2 } else { 1 },
            );
            frontier.push(next);
        }
        if frontier.len() > target * 3 {
            let index = rng.index(frontier.len());
            frontier.remove(index);
        }
    }
    output
}

fn broken_strata(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    let mut output = [0_u8; PIXELS];
    let bands = ((options.cluster_density * 34.0) as usize).clamp(4, 11);
    for _ in 0..bands {
        let horizontal = rng.chance(0.56);
        let (mut x, mut y) = (rng.index(16) as i32, rng.index(16) as i32);
        for _ in 0..rng.range(2, options.cluster_size.clamp(3, 8)) {
            if rng.chance(0.82) {
                set_index(&mut output, x, y, if rng.chance(0.2) { 2 } else { 1 });
            }
            if horizontal {
                x += 1;
            } else {
                y += 1;
            }
        }
    }
    output
}

fn short_walks(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    let target = (options.cluster_density * PIXELS as f32) as usize;
    let mut output = [0_u8; PIXELS];
    while nonzero_count(&output) < target {
        let (mut x, mut y) = (rng.index(16) as i32, rng.index(16) as i32);
        let length = rng.range(2, options.cluster_size.clamp(3, 9));
        for _ in 0..length {
            set_index(&mut output, x, y, if rng.chance(0.18) { 2 } else { 1 });
            let (dx, dy) = CARDINALS[rng.index(CARDINALS.len())];
            x = (x + dx).clamp(0, 15);
            y = (y + dy).clamp(0, 15);
        }
    }
    output
}

fn cellular_cleanup(input: [u8; PIXELS]) -> [u8; PIXELS] {
    let mut output = input;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let neighbors = CARDINALS
                .iter()
                .filter(|&&(dx, dy)| get_index(&input, x + dx, y + dy) != 0)
                .count();
            let index = (y * SIDE + x) as usize;
            if input[index] == 0 && neighbors >= 3 {
                output[index] = 1;
            } else if input[index] != 0 && neighbors == 0 {
                output[index] = 0;
            }
        }
    }
    output
}

pub fn artifact_metrics(indices: &[u8; PIXELS]) -> ArtifactMetrics {
    let occupied = nonzero_count(indices).max(1);
    let mut best_periodicity = 0.0_f32;
    for shift in 1..=4 {
        let mut matches = 0;
        let mut compared = 0;
        for y in 0..16 {
            for x in 0..16 - shift {
                compared += 1;
                if indices[y * 16 + x] == indices[y * 16 + x + shift] {
                    matches += 1;
                }
            }
        }
        best_periodicity = best_periodicity.max(matches as f32 / compared as f32);
    }
    let mut horizontal = 0_i32;
    let mut vertical = 0_i32;
    let mut isolated = 0;
    for y in 0..SIDE {
        for x in 0..SIDE {
            if get_index(indices, x, y) == 0 {
                continue;
            }
            horizontal += i32::from(get_index(indices, x + 1, y) != 0);
            vertical += i32::from(get_index(indices, x, y + 1) != 0);
            if CARDINALS
                .iter()
                .all(|&(dx, dy)| get_index(indices, x + dx, y + dy) == 0)
            {
                isolated += 1;
            }
        }
    }
    let orientation_imbalance =
        (horizontal - vertical).unsigned_abs() as f32 / (horizontal + vertical).max(1) as f32;
    let isolated_ratio = isolated as f32 / occupied as f32;
    let periodicity = ((best_periodicity - 0.58) / 0.42).clamp(0.0, 1.0);
    ArtifactMetrics {
        periodicity,
        orientation_imbalance,
        isolated_ratio,
        score: periodicity * 0.52 + orientation_imbalance * 0.28 + isolated_ratio * 0.8,
    }
}

pub(crate) fn ore_mask(rng: &mut FixedRng, options: &GenerateOptions) -> [bool; PIXELS] {
    let target = (options.ore_coverage * PIXELS as f32).round() as usize;
    let mut mask = [false; PIXELS];
    let start = (7 + rng.index(3) as i32, 7 + rng.index(3) as i32);
    mask[(start.1 * SIDE + start.0) as usize] = true;
    let mut frontier = vec![start];
    let mut attempts = 0;
    while mask.iter().filter(|&&value| value).count() < target && attempts < target * 64 {
        attempts += 1;
        let origin = match options.ore_pattern {
            OrePattern::CenterGrowth | OrePattern::CompactCellular => {
                frontier[rng.index(frontier.len())]
            }
            OrePattern::BranchingWalk => *frontier.last().expect("ore frontier"),
        };
        let mut choices = CARDINALS;
        if rng.chance(options.ore_center_bias) {
            choices.sort_by_key(|&(dx, dy)| {
                let x = origin.0 + dx - 8;
                let y = origin.1 + dy - 8;
                x * x + y * y
            });
        }
        let offset = if rng.chance(options.ore_center_bias) {
            choices[rng.index(2)]
        } else {
            choices[rng.index(4)]
        };
        let next = (
            (origin.0 + offset.0).clamp(2, 13),
            (origin.1 + offset.1).clamp(2, 13),
        );
        let index = (next.1 * SIDE + next.0) as usize;
        if !mask[index] {
            mask[index] = true;
            frontier.push(next);
            if options.ore_thickness > 1 && rng.chance(0.55) {
                let extra = CARDINALS[rng.index(4)];
                let extra = (
                    (next.0 + extra.0).clamp(2, 13),
                    (next.1 + extra.1).clamp(2, 13),
                );
                mask[(extra.1 * SIDE + extra.0) as usize] = true;
            }
        } else if matches!(options.ore_pattern, OrePattern::BranchingWalk) && frontier.len() > 1 {
            frontier.pop();
        } else if frontier.len() > target * 2 {
            let remove = rng.index(frontier.len());
            frontier.remove(remove);
        }
        if matches!(options.ore_pattern, OrePattern::BranchingWalk)
            && frontier.len() % (target / options.ore_branches.max(1)).max(2) == 0
        {
            frontier.truncate(frontier.len().saturating_sub(rng.range(0, 3)).max(1));
        }
    }
    fill_connected_fallback(&mut mask, target);
    recenter(mask)
}

fn fill_connected_fallback(mask: &mut [bool; PIXELS], target: usize) {
    while mask.iter().filter(|&&value| value).count() < target {
        let mut candidates = Vec::new();
        for (index, occupied) in mask.iter().enumerate() {
            if !occupied {
                continue;
            }
            let x = (index % 16) as i32;
            let y = (index / 16) as i32;
            for (dx, dy) in CARDINALS {
                let next_x = x + dx;
                let next_y = y + dy;
                if !(2..=13).contains(&next_x) || !(2..=13).contains(&next_y) {
                    continue;
                }
                let next = (next_y * SIDE + next_x) as usize;
                if !mask[next] {
                    let center_distance = (next_x - 8).abs() + (next_y - 8).abs();
                    candidates.push((center_distance, next));
                }
            }
        }
        candidates.sort_unstable();
        let Some((_, next)) = candidates.first() else {
            break;
        };
        mask[*next] = true;
    }
}

fn recenter(mask: [bool; PIXELS]) -> [bool; PIXELS] {
    let coordinates = mask
        .iter()
        .enumerate()
        .filter(|(_, occupied)| **occupied)
        .map(|(index, _)| ((index % 16) as i32, (index / 16) as i32))
        .collect::<Vec<_>>();
    let count = coordinates.len() as f32;
    let centroid_x = coordinates.iter().map(|(x, _)| *x).sum::<i32>() as f32 / count;
    let centroid_y = coordinates.iter().map(|(_, y)| *y).sum::<i32>() as f32 / count;
    let minimum_x = coordinates.iter().map(|(x, _)| *x).min().unwrap_or(0);
    let maximum_x = coordinates.iter().map(|(x, _)| *x).max().unwrap_or(15);
    let minimum_y = coordinates.iter().map(|(_, y)| *y).min().unwrap_or(0);
    let maximum_y = coordinates.iter().map(|(_, y)| *y).max().unwrap_or(15);
    let shift_x = (7.5 - centroid_x)
        .round()
        .clamp(-minimum_x as f32, (15 - maximum_x) as f32) as i32;
    let shift_y = (7.5 - centroid_y)
        .round()
        .clamp(-minimum_y as f32, (15 - maximum_y) as f32) as i32;
    let mut centered = [false; PIXELS];
    for (x, y) in coordinates {
        centered[((y + shift_y) * SIDE + x + shift_x) as usize] = true;
    }
    centered
}

pub(crate) fn inside(x: i32, y: i32) -> bool {
    (0..SIDE).contains(&x) && (0..SIDE).contains(&y)
}

fn set_index(output: &mut [u8; PIXELS], x: i32, y: i32, value: u8) {
    if inside(x, y) {
        output[(y * SIDE + x) as usize] = value;
    }
}

fn get_index(output: &[u8; PIXELS], x: i32, y: i32) -> u8 {
    if inside(x, y) {
        output[(y * SIDE + x) as usize]
    } else {
        0
    }
}

fn nonzero_count(output: &[u8; PIXELS]) -> usize {
    output.iter().filter(|&&value| value != 0).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PatternAlgorithm;

    #[test]
    fn all_pattern_algorithms_terminate_and_are_deterministic() {
        for pattern in [
            PatternAlgorithm::ClusterStamps,
            PatternAlgorithm::CellularClumps,
            PatternAlgorithm::BrokenStrata,
            PatternAlgorithm::ShortWalks,
        ] {
            let options = GenerateOptions {
                pattern,
                ..Default::default()
            };
            let mut first = FixedRng::new(91);
            let mut second = FixedRng::new(91);
            assert_eq!(
                best_pattern(&mut first, &options),
                best_pattern(&mut second, &options)
            );
        }
    }

    #[test]
    fn quality_metrics_penalize_repeating_stripes() {
        let mut stripes = [0_u8; PIXELS];
        for y in 0..16 {
            for x in 0..16 {
                stripes[y * 16 + x] = (x % 2) as u8;
            }
        }
        let mut irregular = [0_u8; PIXELS];
        for &(x, y) in &[(4, 5), (5, 5), (5, 6), (9, 8), (9, 9), (10, 9)] {
            set_index(&mut irregular, x, y, 1);
        }
        assert!(artifact_metrics(&stripes).score > artifact_metrics(&irregular).score);
    }

    #[test]
    fn every_ore_algorithm_has_bounded_connected_output() {
        for ore_pattern in [
            OrePattern::CenterGrowth,
            OrePattern::BranchingWalk,
            OrePattern::CompactCellular,
        ] {
            let options = GenerateOptions {
                ore_pattern,
                ..Default::default()
            };
            let mut rng = FixedRng::new(31);
            let mask = ore_mask(&mut rng, &options);
            assert!(mask.iter().filter(|&&occupied| occupied).count() >= 76);
        }
    }
}
