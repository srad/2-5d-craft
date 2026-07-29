use super::{BlockKind, Face, PackImage, ResolvedPack, VARIANT_COUNT};

pub fn compose_preview(pack: &ResolvedPack) -> PackImage {
    let mut output = PackImage::solid(320, 180, [24, 21, 18, 255]);
    blit_scaled(&mut output, &pack.stars, 0, 0, 320, 112);
    blit_scaled(&mut output, &pack.sun, 258, 12, 48, 48);
    blit_scaled(&mut output, &pack.cloud, 16, 22, 128, 48);
    for (index, block) in BlockKind::ALL.into_iter().enumerate() {
        let x = 16 + index as u32 * 32;
        let y = if index % 2 == 0 { 116 } else { 132 };
        blit_scaled(
            &mut output,
            pack.block(block, Face::Side, index % VARIANT_COUNT),
            x,
            y,
            32,
            32,
        );
    }
    output
}

fn blit_scaled(
    destination: &mut PackImage,
    source: &PackImage,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) {
    for y in 0..height {
        for x in 0..width {
            let color = source.pixel(x * source.width / width, y * source.height / height);
            if color[3] == 0 {
                continue;
            }
            let target_x = left + x;
            let target_y = top + y;
            if target_x < destination.width && target_y < destination.height {
                destination.set_pixel(target_x, target_y, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{GenerateOptions, generate_pack};

    #[test]
    fn generated_preview_uses_contract_size() {
        let pack = generate_pack(&GenerateOptions::default()).unwrap();
        assert_eq!((pack.preview.width, pack.preview.height), (320, 180));
    }
}
