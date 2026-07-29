use crate::{
    PackImage,
    rng::{FixedRng, mix_seed},
};
use std::collections::BTreeMap;

pub(crate) fn generate_environment(seed: u64, assets: &mut BTreeMap<String, PackImage>) {
    assets.insert("environment/sun.png".into(), sun_image());
    for phase in 0..8 {
        assets.insert(format!("environment/moon_{phase}.png"), moon_image(phase));
    }
    assets.insert("environment/stars.png".into(), stars_image(seed));
    assets.insert("environment/cloud.png".into(), cloud_image());
}

fn sun_image() -> PackImage {
    let mut image = PackImage::solid(32, 32, [0, 0, 0, 0]);
    for y in 4..28 {
        for x in 4..28 {
            let edge = !(7..=24).contains(&x) || !(7..=24).contains(&y);
            let color = if edge {
                [238, 142, 30, 255]
            } else {
                [255, 210, 69, 255]
            };
            image.set_pixel(x, y, color);
        }
    }
    image
}

fn moon_image(phase: usize) -> PackImage {
    let mut image = PackImage::solid(32, 32, [0, 0, 0, 0]);
    let shift = phase as i32 - 4;
    for y in 3..29_i32 {
        for x in 3..29_i32 {
            let dx = x - 16;
            let dy = y - 16;
            if dx * dx + dy * dy <= 13 * 13 {
                let lit = if phase <= 4 {
                    dx >= shift * 3 - 12
                } else {
                    dx <= 12 - shift * 3
                };
                let color = if lit {
                    [215, 208, 174, 255]
                } else {
                    [65, 68, 73, 220]
                };
                image.set_pixel(x as u32, y as u32, color);
            }
        }
    }
    image
}

fn stars_image(seed: u64) -> PackImage {
    let mut image = PackImage::solid(512, 256, [0, 0, 0, 0]);
    let mut rng = FixedRng::new(mix_seed(seed, "stars", 0));
    for _ in 0..190 {
        let x = rng.index(512) as u32;
        let y = rng.index(256) as u32;
        let brightness = rng.range(145, 240) as u8;
        image.set_pixel(x, y, [brightness, brightness, brightness, brightness]);
    }
    image
}

fn cloud_image() -> PackImage {
    let mut image = PackImage::solid(64, 24, [0, 0, 0, 0]);
    for y in 7..20 {
        for x in 5..59 {
            if (x > 11 && x < 51) || (x as i32 - 21).pow(2) + (y as i32 - 9).pow(2) < 64 {
                let value = if y >= 17 {
                    198
                } else if y >= 13 {
                    214
                } else {
                    230
                };
                image.set_pixel(x, y, [value, value, value.saturating_add(5), 210]);
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_assets_have_contract_dimensions() {
        let mut assets = BTreeMap::new();
        generate_environment(4, &mut assets);
        assert_eq!(assets["environment/sun.png"].width, 32);
        assert_eq!(assets["environment/stars.png"].width, 512);
        assert_eq!(assets["environment/cloud.png"].height, 24);
        assert_eq!(
            assets.keys().filter(|path| path.contains("moon_")).count(),
            8
        );
    }
}
