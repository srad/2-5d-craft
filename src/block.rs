use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub enum BlockKind {
    Grass,
    Dirt,
    Stone,
    CoalOre,
    IronOre,
    Wood,
    Leaves,
    Torch,
    Bedrock,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockDef {
    pub hardness_seconds: f32,
    pub solid: bool,
    pub light_opacity: u8,
    pub emitted_light: u8,
    pub hotbar_slot: Option<u8>,
}

impl BlockKind {
    pub const HOTBAR: [Self; 8] = [
        Self::Grass,
        Self::Dirt,
        Self::Stone,
        Self::CoalOre,
        Self::IronOre,
        Self::Wood,
        Self::Leaves,
        Self::Torch,
    ];

    pub const ALL: [Self; 9] = [
        Self::Grass,
        Self::Dirt,
        Self::Stone,
        Self::CoalOre,
        Self::IronOre,
        Self::Wood,
        Self::Leaves,
        Self::Torch,
        Self::Bedrock,
    ];

    pub const fn def(self) -> BlockDef {
        match self {
            Self::Grass => BlockDef::solid(0.40, 15, Some(1)),
            Self::Dirt => BlockDef::solid(0.35, 15, Some(2)),
            Self::Stone => BlockDef::solid(0.80, 15, Some(3)),
            Self::CoalOre => BlockDef::solid(1.20, 15, Some(4)),
            Self::IronOre => BlockDef::solid(1.50, 15, Some(5)),
            Self::Wood => BlockDef::solid(0.80, 15, Some(6)),
            Self::Leaves => BlockDef::solid(0.20, 2, Some(7)),
            Self::Torch => BlockDef {
                hardness_seconds: 0.10,
                solid: false,
                light_opacity: 0,
                emitted_light: 12,
                hotbar_slot: Some(8),
            },
            Self::Bedrock => BlockDef {
                hardness_seconds: f32::INFINITY,
                solid: true,
                light_opacity: 15,
                emitted_light: 0,
                hotbar_slot: None,
            },
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Grass => "Grass",
            Self::Dirt => "Dirt",
            Self::Stone => "Stone",
            Self::CoalOre => "Coal ore",
            Self::IronOre => "Iron ore",
            Self::Wood => "Wood",
            Self::Leaves => "Leaves",
            Self::Torch => "Torch",
            Self::Bedrock => "Bedrock",
        }
    }

    pub const fn breakable(self) -> bool {
        !matches!(self, Self::Bedrock)
    }

    pub const fn code(self) -> u8 {
        self as u8 + 1
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        if code == 0 || code as usize > Self::ALL.len() {
            None
        } else {
            Some(Self::ALL[code as usize - 1])
        }
    }
}

impl BlockDef {
    const fn solid(hardness_seconds: f32, light_opacity: u8, hotbar_slot: Option<u8>) -> Self {
        Self {
            hardness_seconds,
            solid: true,
            light_opacity,
            emitted_light: 0,
            hotbar_slot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn hotbar_slots_are_complete_and_unique() {
        let slots: HashSet<_> = BlockKind::HOTBAR
            .iter()
            .map(|kind| kind.def().hotbar_slot)
            .collect();
        assert_eq!(slots.len(), 8);
        assert!(slots.contains(&Some(1)));
        assert!(slots.contains(&Some(8)));
    }

    #[test]
    fn bedrock_is_the_only_unbreakable_block() {
        for kind in BlockKind::ALL {
            assert_eq!(kind.breakable(), kind != BlockKind::Bedrock);
        }
        assert!(BlockKind::Bedrock.def().hardness_seconds.is_infinite());
    }

    #[test]
    fn torch_is_non_solid_and_emissive() {
        let torch = BlockKind::Torch.def();
        assert!(!torch.solid);
        assert_eq!(torch.light_opacity, 0);
        assert_eq!(torch.emitted_light, 12);
    }

    #[test]
    fn binary_palette_entries_round_trip() {
        for kind in BlockKind::ALL {
            let encoded = postcard::to_allocvec(&kind).unwrap();
            let (decoded, remaining): (BlockKind, &[u8]) =
                postcard::take_from_bytes(&encoded).unwrap();
            assert_eq!(decoded, kind);
            assert!(remaining.is_empty());
        }
    }

    #[test]
    fn dense_codes_reserve_zero_for_air() {
        assert_eq!(BlockKind::from_code(0), None);
        for kind in BlockKind::ALL {
            assert_eq!(BlockKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(BlockKind::from_code(255), None);
    }
}
