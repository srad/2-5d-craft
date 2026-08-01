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
    let attempts = match options.quality {
        QualityPreset::Relaxed => 8,
        QualityPreset::Balanced => 16,
        QualityPreset::Strict => 32,
    };
    let mut best = [0_u8; PIXELS];
    let mut best_score = f32::INFINITY;
    for _ in 0..attempts {
        let mut candidate = generate_pattern(rng, options);
        for _ in 0..options.smoothing_passes {
            candidate = cellular_cleanup(candidate);
        }
        let score = patchwork_score(&candidate);
        if score < best_score {
            best = candidate;
            best_score = score;
        }
    }
    best
}

fn generate_pattern(rng: &mut FixedRng, options: &GenerateOptions) -> [u8; PIXELS] {
    let targets = shade_targets(options.cluster_density);
    let mut remaining = targets;
    remaining[0] = 0;
    let mut output = [0_u8; PIXELS];
    let mut origins = Vec::new();
    let mut cluster_index = 0;
    let mut attempts = 0;

    while remaining[1..].iter().sum::<usize>() > 0 && attempts < PIXELS * 8 {
        attempts += 1;
        let primary = choose_role(rng, &remaining, None);
        let size = cluster_size(rng, options, remaining[primary]);
        let origin = choose_origin(rng, &output, &origins, cluster_index, options.placement);
        let cells = grow_surface_cluster(rng, &output, origin, size, options);
        if cells.is_empty() {
            continue;
        }
        origins.push(origin);
        cluster_index += 1;
        for index in cells {
            if output[index] != 0 || remaining[1..].iter().sum::<usize>() == 0 {
                continue;
            }
            let use_support = rng.chance(0.28)
                && remaining
                    .iter()
                    .enumerate()
                    .any(|(role, count)| role != primary && role != 0 && *count > 0);
            let role = if remaining[primary] > 0 && !use_support {
                primary
            } else {
                choose_role(rng, &remaining, Some(primary))
            };
            output[index] = role as u8;
            remaining[role] -= 1;
        }
    }

    fill_remaining_roles(rng, &mut output, &mut remaining);
    output
}

fn shade_targets(density: f32) -> [usize; 4] {
    let base = ((0.62 - density).clamp(0.30, 0.58) * PIXELS as f32).round() as usize;
    let varied = PIXELS - base;
    let first = (varied as f32 * 0.46).round() as usize;
    let second = (varied as f32 * 0.27).round() as usize;
    [base, first, second, varied - first - second]
}

fn choose_role(rng: &mut FixedRng, remaining: &[usize; 4], avoid: Option<usize>) -> usize {
    let total = (1..4)
        .filter(|role| Some(*role) != avoid)
        .map(|role| remaining[role])
        .sum::<usize>();
    if total == 0 {
        return (1..4).find(|role| remaining[*role] > 0).unwrap_or(1);
    }
    let mut choice = rng.index(total);
    for (role, count) in remaining.iter().enumerate().skip(1) {
        if Some(role) == avoid {
            continue;
        }
        if choice < *count {
            return role;
        }
        choice -= *count;
    }
    3
}

fn cluster_size(rng: &mut FixedRng, options: &GenerateOptions, remaining: usize) -> usize {
    let maximum = match options.pattern {
        PatternAlgorithm::ClusterStamps => options.cluster_size.clamp(2, 8),
        PatternAlgorithm::EvenlyVaried => options.cluster_size.clamp(1, 3),
        PatternAlgorithm::CellularClumps => (options.cluster_size + 3).clamp(4, 12),
        PatternAlgorithm::BrokenStrata => options.cluster_size.clamp(3, 8),
        PatternAlgorithm::ShortWalks => options.cluster_size.clamp(2, 9),
    }
    .min(remaining.max(1));
    let minimum = match options.pattern {
        PatternAlgorithm::CellularClumps => maximum.min(4),
        PatternAlgorithm::BrokenStrata => maximum.min(3),
        _ => 1,
    };
    rng.range(minimum, maximum)
}

fn choose_origin(
    rng: &mut FixedRng,
    output: &[u8; PIXELS],
    origins: &[(i32, i32)],
    cluster_index: usize,
    placement: PlacementAlgorithm,
) -> (i32, i32) {
    let preferred = match placement {
        PlacementAlgorithm::Uniform => (rng.index(16) as i32, rng.index(16) as i32),
        PlacementAlgorithm::JitteredGrid => {
            let cell = cluster_index % 16;
            (
                ((cell % 4) * 4 + rng.index(4)) as i32,
                ((cell / 4) * 4 + rng.index(4)) as i32,
            )
        }
        PlacementAlgorithm::PoissonDisc => {
            let mut best = (rng.index(16) as i32, rng.index(16) as i32);
            let mut best_distance = -1;
            for _ in 0..24 {
                let candidate = (rng.index(16) as i32, rng.index(16) as i32);
                if get_wrapped(output, candidate.0, candidate.1) != 0 {
                    continue;
                }
                let distance = origins
                    .iter()
                    .map(|origin| toroidal_distance(*origin, candidate))
                    .min()
                    .unwrap_or(SIDE);
                if distance > best_distance {
                    best = candidate;
                    best_distance = distance;
                }
            }
            best
        }
    };
    nearest_base(output, preferred)
}

fn nearest_base(output: &[u8; PIXELS], preferred: (i32, i32)) -> (i32, i32) {
    (0..PIXELS)
        .filter(|index| output[*index] == 0)
        .map(coordinates)
        .min_by_key(|point| toroidal_distance(*point, preferred))
        .unwrap_or(preferred)
}

fn grow_surface_cluster(
    rng: &mut FixedRng,
    output: &[u8; PIXELS],
    origin: (i32, i32),
    size: usize,
    options: &GenerateOptions,
) -> Vec<usize> {
    let rectangular = matches!(options.cluster_shape, ClusterShape::Rectangular)
        || matches!(options.cluster_shape, ClusterShape::Mixed) && rng.chance(0.32);
    if rectangular {
        return rectangular_cluster(rng, output, origin, size);
    }

    let mut cells = Vec::with_capacity(size);
    let origin_index = wrapped_index(origin.0, origin.1);
    if output[origin_index] == 0 {
        cells.push(origin_index);
    }
    let mut attempts = 0;
    while cells.len() < size && attempts < size * 48 {
        attempts += 1;
        let source = match options.pattern {
            PatternAlgorithm::ShortWalks => *cells.last().unwrap_or(&origin_index),
            PatternAlgorithm::CellularClumps => cells[rng.index(cells.len())],
            _ => cells[rng.index(cells.len())],
        };
        let (x, y) = coordinates(source);
        let direction = surface_direction(rng, options.pattern);
        let next = wrapped_index(x + direction.0, y + direction.1);
        if output[next] == 0 && !cells.contains(&next) {
            cells.push(next);
        }
    }
    cells
}

fn rectangular_cluster(
    rng: &mut FixedRng,
    output: &[u8; PIXELS],
    origin: (i32, i32),
    size: usize,
) -> Vec<usize> {
    let width = (size as f32).sqrt().ceil() as usize;
    let height = size.div_ceil(width);
    let transpose = rng.chance(0.5);
    let mut cells = Vec::with_capacity(size);
    for row in 0..height {
        for column in 0..width {
            let (dx, dy) = if transpose {
                (row as i32, column as i32)
            } else {
                (column as i32, row as i32)
            };
            let index = wrapped_index(origin.0 + dx, origin.1 + dy);
            if output[index] == 0 && !cells.contains(&index) {
                cells.push(index);
                if cells.len() == size {
                    return cells;
                }
            }
        }
    }
    cells
}

fn surface_direction(rng: &mut FixedRng, pattern: PatternAlgorithm) -> (i32, i32) {
    let horizontal_chance = match pattern {
        PatternAlgorithm::BrokenStrata => 0.78,
        PatternAlgorithm::ShortWalks => 0.62,
        _ => 0.5,
    };
    if rng.chance(horizontal_chance) {
        if rng.chance(0.5) { (1, 0) } else { (-1, 0) }
    } else if rng.chance(0.5) {
        (0, 1)
    } else {
        (0, -1)
    }
}

fn fill_remaining_roles(rng: &mut FixedRng, output: &mut [u8; PIXELS], remaining: &mut [usize; 4]) {
    while remaining[1..].iter().sum::<usize>() > 0 {
        let role = choose_role(rng, remaining, None);
        let mut best = None;
        for _ in 0..32 {
            let index = rng.index(PIXELS);
            if output[index] != 0 {
                continue;
            }
            let (x, y) = coordinates(index);
            let same = CARDINALS
                .iter()
                .filter(|&&(dx, dy)| get_wrapped(output, x + dx, y + dy) == role as u8)
                .count();
            let varied = CARDINALS
                .iter()
                .filter(|&&(dx, dy)| get_wrapped(output, x + dx, y + dy) != 0)
                .count();
            let score = same * 3 + varied;
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((index, score));
            }
        }
        let index = best
            .map(|(index, _)| index)
            .or_else(|| output.iter().position(|value| *value == 0))
            .expect("shade quotas leave base pixels available");
        output[index] = role as u8;
        remaining[role] -= 1;
    }
}

fn cellular_cleanup(input: [u8; PIXELS]) -> [u8; PIXELS] {
    let mut output = input;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let mut counts = [0_u8; 4];
            for (dx, dy) in CARDINALS {
                counts[get_wrapped(&input, x + dx, y + dy) as usize] += 1;
            }
            let (role, count) = counts
                .iter()
                .copied()
                .enumerate()
                .max_by_key(|(_, count)| *count)
                .unwrap();
            if count >= 3 {
                output[wrapped_index(x, y)] = role as u8;
            }
        }
    }
    output
}

fn patchwork_score(indices: &[u8; PIXELS]) -> f32 {
    let mut counts = [0_usize; 4];
    for &role in indices {
        counts[role as usize] += 1;
    }
    let dominant = *counts.iter().max().unwrap() as f32 / PIXELS as f32;
    let sparse_penalty = counts
        .iter()
        .map(|count| (0.08 - *count as f32 / PIXELS as f32).max(0.0))
        .sum::<f32>();

    let mut checkerboards = 0;
    let mut horizontal_transitions = 0_i32;
    let mut vertical_transitions = 0_i32;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let value = get_wrapped(indices, x, y);
            horizontal_transitions += i32::from(value != get_wrapped(indices, x + 1, y));
            vertical_transitions += i32::from(value != get_wrapped(indices, x, y + 1));
            let right = get_wrapped(indices, x + 1, y);
            let down = get_wrapped(indices, x, y + 1);
            let diagonal = get_wrapped(indices, x + 1, y + 1);
            if value == diagonal && right == down && value != right {
                checkerboards += 1;
            }
        }
    }
    let orientation = (horizontal_transitions - vertical_transitions).unsigned_abs() as f32
        / (horizontal_transitions + vertical_transitions).max(1) as f32;
    let periodicity = patchwork_periodicity(indices);
    let long_runs = long_run_penalty(indices);
    let seam = seam_penalty(indices);

    (dominant - 0.50).max(0.0) * 5.0
        + sparse_penalty * 5.0
        + checkerboards as f32 / PIXELS as f32 * 1.8
        + orientation * 0.7
        + periodicity * 1.2
        + long_runs * 0.8
        + seam * 0.8
}

fn patchwork_periodicity(indices: &[u8; PIXELS]) -> f32 {
    let mut best = 0.0_f32;
    for shift in 1..=4 {
        let matches = (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|&(x, y)| get_wrapped(indices, x, y) == get_wrapped(indices, x + shift, y))
            .count();
        best = best.max(matches as f32 / PIXELS as f32);
    }
    ((best - 0.58) / 0.42).clamp(0.0, 1.0)
}

fn long_run_penalty(indices: &[u8; PIXELS]) -> f32 {
    let mut excess = 0;
    for fixed in 0..SIDE {
        for vertical in [false, true] {
            let mut run = 1;
            for offset in 1..SIDE {
                let previous = if vertical {
                    get_wrapped(indices, fixed, offset - 1)
                } else {
                    get_wrapped(indices, offset - 1, fixed)
                };
                let current = if vertical {
                    get_wrapped(indices, fixed, offset)
                } else {
                    get_wrapped(indices, offset, fixed)
                };
                if current == previous {
                    run += 1;
                } else {
                    excess += (run - 6).max(0);
                    run = 1;
                }
            }
            excess += (run - 6).max(0);
        }
    }
    excess as f32 / PIXELS as f32
}

fn seam_penalty(indices: &[u8; PIXELS]) -> f32 {
    let horizontal_interior = (0..SIDE)
        .flat_map(|y| (0..SIDE - 1).map(move |x| (x, y)))
        .filter(|&(x, y)| get_wrapped(indices, x, y) != get_wrapped(indices, x + 1, y))
        .count() as f32
        / (SIDE * (SIDE - 1)) as f32;
    let vertical_interior = (0..SIDE - 1)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter(|&(x, y)| get_wrapped(indices, x, y) != get_wrapped(indices, x, y + 1))
        .count() as f32
        / (SIDE * (SIDE - 1)) as f32;
    let horizontal_seam = (0..SIDE)
        .filter(|&y| get_wrapped(indices, 15, y) != get_wrapped(indices, 0, y))
        .count() as f32
        / SIDE as f32;
    let vertical_seam = (0..SIDE)
        .filter(|&x| get_wrapped(indices, x, 15) != get_wrapped(indices, x, 0))
        .count() as f32
        / SIDE as f32;
    (horizontal_interior - horizontal_seam).abs() + (vertical_interior - vertical_seam).abs()
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
    let component_count = (options.ore_branches + 4).clamp(6, 12).min(target / 2);
    let sizes = distributed_sizes(rng, target, component_count, 2, 16);
    let mut mask = [false; PIXELS];

    for size in sizes {
        let Some(origin) = choose_mask_origin(rng, &mask, options.ore_center_bias) else {
            break;
        };
        let component = grow_ore_component(rng, &mask, origin, size, options);
        if component.len() < 2 {
            continue;
        }
        for index in component {
            mask[index] = true;
        }
    }
    fill_mask_to_target(&mut mask, target);
    mask
}

pub(crate) fn leaf_hole_mask(rng: &mut FixedRng, density: f32) -> [bool; PIXELS] {
    let target = (density * PIXELS as f32).round() as usize;
    if target == 0 {
        return [false; PIXELS];
    }
    let component_count = (target / 3).clamp(1, 24).min(target);
    let mut best = [false; PIXELS];
    let mut best_score = usize::MAX;
    for _ in 0..16 {
        let sizes = distributed_sizes(rng, target, component_count, 1, 8);
        let mut candidate = [false; PIXELS];
        for size in sizes {
            let Some(origin) = choose_mask_origin(rng, &candidate, 0.0) else {
                break;
            };
            for index in grow_hole_component(rng, &candidate, origin, size) {
                candidate[index] = true;
            }
        }
        fill_mask_to_target(&mut candidate, target);
        let holes = component_sizes(&candidate, true);
        let opaque = component_sizes(&candidate, false);
        let largest_opaque = opaque.into_iter().max().unwrap_or(0);
        let disconnected = (PIXELS - target).saturating_sub(largest_opaque);
        let oversized = holes
            .iter()
            .map(|size| size.saturating_sub(8))
            .sum::<usize>();
        let score = disconnected * 20 + oversized * 4 + holes.len().abs_diff(component_count);
        if score < best_score {
            best = candidate;
            best_score = score;
        }
    }
    best
}

fn distributed_sizes(
    rng: &mut FixedRng,
    target: usize,
    count: usize,
    minimum: usize,
    maximum: usize,
) -> Vec<usize> {
    let mut sizes = vec![minimum; count];
    let mut remaining = target.saturating_sub(minimum * count);
    while remaining > 0 {
        let available = sizes
            .iter()
            .enumerate()
            .filter_map(|(index, size)| (*size < maximum).then_some(index))
            .collect::<Vec<_>>();
        if available.is_empty() {
            break;
        }
        sizes[available[rng.index(available.len())]] += 1;
        remaining -= 1;
    }
    sizes
}

fn choose_mask_origin(
    rng: &mut FixedRng,
    mask: &[bool; PIXELS],
    center_bias: f32,
) -> Option<(i32, i32)> {
    (0..PIXELS)
        .filter(|index| mask_clear(mask, *index))
        .map(|index| {
            let point = coordinates(index);
            let separation = mask
                .iter()
                .enumerate()
                .filter(|(_, occupied)| **occupied)
                .map(|(other, _)| toroidal_distance(point, coordinates(other)))
                .min()
                .unwrap_or(SIDE);
            let center_distance = (point.0 - 7).abs() + (point.1 - 7).abs();
            let score = separation * 32 - (center_distance as f32 * center_bias * 3.0) as i32
                + rng.index(7) as i32;
            (score, point)
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, point)| point)
}

fn grow_ore_component(
    rng: &mut FixedRng,
    mask: &[bool; PIXELS],
    origin: (i32, i32),
    size: usize,
    options: &GenerateOptions,
) -> Vec<usize> {
    let mut cells = vec![wrapped_index(origin.0, origin.1)];
    let mut attempts = 0;
    while cells.len() < size && attempts < size * 96 {
        attempts += 1;
        let source = match options.ore_pattern {
            OrePattern::BranchingWalk => *cells.last().unwrap(),
            OrePattern::CenterGrowth | OrePattern::CompactCellular => cells[rng.index(cells.len())],
        };
        let (x, y) = coordinates(source);
        let horizontal = match options.ore_pattern {
            OrePattern::BranchingWalk => 0.72,
            OrePattern::CenterGrowth => 0.64,
            OrePattern::CompactCellular => 0.55,
        };
        let direction = if rng.chance(horizontal) {
            if rng.chance(0.5) { (1, 0) } else { (-1, 0) }
        } else if rng.chance(0.5) {
            (0, 1)
        } else {
            (0, -1)
        };
        let next = wrapped_index(x + direction.0, y + direction.1);
        let (_, next_y) = coordinates(next);
        if toroidal_axis_distance(origin.1, next_y) > options.ore_thickness as i32
            || cells.contains(&next)
            || !mask_clear(mask, next)
        {
            continue;
        }
        cells.push(next);
    }
    while cells.len() < size {
        let mut candidates = (0..PIXELS)
            .filter(|index| !cells.contains(index) && mask_clear(mask, *index))
            .filter(|index| {
                let (x, y) = coordinates(*index);
                cells.iter().any(|cell| {
                    let point = coordinates(*cell);
                    toroidal_distance(point, (x, y)) == 1
                })
            })
            .collect::<Vec<_>>();
        let bounded = candidates
            .iter()
            .copied()
            .filter(|index| {
                toroidal_axis_distance(origin.1, coordinates(*index).1)
                    <= options.ore_thickness as i32
            })
            .collect::<Vec<_>>();
        if !bounded.is_empty() {
            candidates = bounded;
        }
        let Some(next) = candidates.get(rng.index(candidates.len().max(1))).copied() else {
            break;
        };
        cells.push(next);
    }
    cells
}

fn grow_hole_component(
    rng: &mut FixedRng,
    mask: &[bool; PIXELS],
    origin: (i32, i32),
    size: usize,
) -> Vec<usize> {
    let mut cells = vec![wrapped_index(origin.0, origin.1)];
    let mut attempts = 0;
    while cells.len() < size && attempts < size * 64 {
        attempts += 1;
        let source = cells[rng.index(cells.len())];
        let (x, y) = coordinates(source);
        let (dx, dy) = CARDINALS[rng.index(CARDINALS.len())];
        let next = wrapped_index(x + dx, y + dy);
        if !cells.contains(&next) && mask_clear(mask, next) {
            cells.push(next);
        }
    }
    cells
}

fn mask_clear(mask: &[bool; PIXELS], index: usize) -> bool {
    if mask[index] {
        return false;
    }
    let (x, y) = coordinates(index);
    CARDINALS
        .iter()
        .all(|&(dx, dy)| !mask[wrapped_index(x + dx, y + dy)])
}

fn fill_mask_to_target(mask: &mut [bool; PIXELS], target: usize) {
    while mask.iter().filter(|occupied| **occupied).count() < target {
        let next = (0..PIXELS)
            .filter(|index| !mask[*index])
            .max_by_key(|index| {
                let (x, y) = coordinates(*index);
                CARDINALS
                    .iter()
                    .filter(|&&(dx, dy)| mask[wrapped_index(x + dx, y + dy)])
                    .count()
            });
        let Some(next) = next else {
            break;
        };
        mask[next] = true;
    }
}

fn component_sizes(mask: &[bool; PIXELS], value: bool) -> Vec<usize> {
    let mut seen = [false; PIXELS];
    let mut sizes = Vec::new();
    for start in 0..PIXELS {
        if seen[start] || mask[start] != value {
            continue;
        }
        seen[start] = true;
        let mut pending = vec![start];
        let mut size = 0;
        while let Some(index) = pending.pop() {
            size += 1;
            let (x, y) = coordinates(index);
            for (dx, dy) in CARDINALS {
                let next = wrapped_index(x + dx, y + dy);
                if !seen[next] && mask[next] == value {
                    seen[next] = true;
                    pending.push(next);
                }
            }
        }
        sizes.push(size);
    }
    sizes
}

fn toroidal_distance(first: (i32, i32), second: (i32, i32)) -> i32 {
    toroidal_axis_distance(first.0, second.0) + toroidal_axis_distance(first.1, second.1)
}

fn toroidal_axis_distance(first: i32, second: i32) -> i32 {
    let distance = (first - second).abs();
    distance.min(SIDE - distance)
}

fn coordinates(index: usize) -> (i32, i32) {
    (
        (index % SIDE as usize) as i32,
        (index / SIDE as usize) as i32,
    )
}

fn wrapped_index(x: i32, y: i32) -> usize {
    (y.rem_euclid(SIDE) * SIDE + x.rem_euclid(SIDE)) as usize
}

fn get_wrapped(output: &[u8; PIXELS], x: i32, y: i32) -> u8 {
    output[wrapped_index(x, y)]
}

pub(crate) fn inside(x: i32, y: i32) -> bool {
    (0..SIDE).contains(&x) && (0..SIDE).contains(&y)
}

#[cfg(test)]
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

    #[test]
    fn all_pattern_algorithms_are_deterministic_and_use_four_balanced_roles() {
        for pattern in [
            PatternAlgorithm::ClusterStamps,
            PatternAlgorithm::EvenlyVaried,
            PatternAlgorithm::CellularClumps,
            PatternAlgorithm::BrokenStrata,
            PatternAlgorithm::ShortWalks,
        ] {
            let options = GenerateOptions {
                pattern,
                smoothing_passes: 0,
                ..Default::default()
            };
            let mut first = FixedRng::new(91);
            let mut second = FixedRng::new(91);
            let first = best_pattern(&mut first, &options);
            assert_eq!(first, best_pattern(&mut second, &options));
            let counts = (0..4)
                .map(|role| first.iter().filter(|value| **value == role).count())
                .collect::<Vec<_>>();
            assert!(
                counts.iter().all(|count| *count >= 30),
                "{pattern:?}: {counts:?}"
            );
            assert!(
                *counts.iter().max().unwrap() <= 128,
                "{pattern:?}: {counts:?}"
            );
        }
    }

    #[test]
    fn cluster_density_changes_base_share_monotonically() {
        let pattern = |density| {
            let mut rng = FixedRng::new(19);
            best_pattern(
                &mut rng,
                &GenerateOptions {
                    cluster_density: density,
                    smoothing_passes: 0,
                    ..Default::default()
                },
            )
        };
        let low = pattern(0.04).iter().filter(|value| **value == 0).count();
        let high = pattern(0.48).iter().filter(|value| **value == 0).count();
        assert!(low > high);
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
    fn ore_algorithms_create_separate_bounded_clusters() {
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
            let sizes = component_sizes(&mask, true);
            assert_eq!(mask.iter().filter(|value| **value).count(), 64);
            assert!(
                (6..=12).contains(&sizes.len()),
                "{ore_pattern:?}: {sizes:?}"
            );
            assert!(
                sizes.iter().all(|size| (2..=16).contains(size)),
                "{ore_pattern:?}: {sizes:?}"
            );
        }
    }

    #[test]
    fn ore_clusters_remain_bounded_across_supported_profiles() {
        for seed in 0..32 {
            for ore_pattern in [
                OrePattern::CenterGrowth,
                OrePattern::BranchingWalk,
                OrePattern::CompactCellular,
            ] {
                for (coverage, branches, thickness) in [(0.20, 1, 1), (0.25, 4, 2), (0.28, 8, 4)] {
                    let options = GenerateOptions {
                        ore_pattern,
                        ore_coverage: coverage,
                        ore_branches: branches,
                        ore_thickness: thickness,
                        ..Default::default()
                    };
                    let mut rng = FixedRng::new(seed);
                    let mask = ore_mask(&mut rng, &options);
                    let sizes = component_sizes(&mask, true);
                    let target = (coverage * PIXELS as f32).round() as usize;
                    assert_eq!(mask.iter().filter(|value| **value).count(), target);
                    assert!(
                        sizes.iter().all(|size| (2..=16).contains(size)),
                        "seed {seed}, {ore_pattern:?}: {sizes:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn leaf_holes_hit_the_target_without_fragmenting_the_canopy() {
        let mut rng = FixedRng::new(47);
        let mask = leaf_hole_mask(&mut rng, 0.22);
        assert_eq!(mask.iter().filter(|value| **value).count(), 56);
        let largest_opaque = component_sizes(&mask, false).into_iter().max().unwrap();
        assert!(largest_opaque as f32 / (PIXELS - 56) as f32 >= 0.90);
    }
}
