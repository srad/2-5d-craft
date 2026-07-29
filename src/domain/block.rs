#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct BlockId(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct BlockState {
    id: BlockId,
    variant: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[repr(u8)]
pub enum TorchMount {
    Floor,
    WallLeft,
    WallRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockDef {
    pub hardness_seconds: f32,
    pub solid: bool,
    pub light_opacity: u8,
    pub emitted_light: u8,
    pub hotbar_slot: Option<u8>,
}

impl BlockId {
    pub const GRASS: Self = Self(1);
    pub const DIRT: Self = Self(2);
    pub const STONE: Self = Self(3);
    pub const COAL_ORE: Self = Self(4);
    pub const IRON_ORE: Self = Self(5);
    pub const WOOD: Self = Self(6);
    pub const LEAVES: Self = Self(7);
    pub const TORCH: Self = Self(8);
    pub const BEDROCK: Self = Self(9);

    pub const HOTBAR: [Self; 7] = [
        Self::DIRT,
        Self::STONE,
        Self::COAL_ORE,
        Self::IRON_ORE,
        Self::WOOD,
        Self::LEAVES,
        Self::TORCH,
    ];

    pub const ALL: [Self; 9] = [
        Self::GRASS,
        Self::DIRT,
        Self::STONE,
        Self::COAL_ORE,
        Self::IRON_ORE,
        Self::WOOD,
        Self::LEAVES,
        Self::TORCH,
        Self::BEDROCK,
    ];

    pub const fn new(value: u16) -> Option<Self> {
        if value >= Self::GRASS.0 && value <= Self::BEDROCK.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    pub const fn def(self) -> BlockDef {
        match self {
            Self::GRASS => BlockDef::solid(0.40, 15, None),
            Self::DIRT => BlockDef::solid(0.35, 15, Some(1)),
            Self::STONE => BlockDef::solid(0.80, 15, Some(2)),
            Self::COAL_ORE => BlockDef::solid(1.20, 15, Some(3)),
            Self::IRON_ORE => BlockDef::solid(1.50, 15, Some(4)),
            Self::WOOD => BlockDef::solid(0.80, 15, Some(5)),
            Self::LEAVES => BlockDef::solid(0.20, 2, Some(6)),
            Self::TORCH => BlockDef {
                hardness_seconds: 0.10,
                solid: false,
                light_opacity: 0,
                emitted_light: 14,
                hotbar_slot: Some(7),
            },
            Self::BEDROCK => BlockDef {
                hardness_seconds: f32::INFINITY,
                solid: true,
                light_opacity: 15,
                emitted_light: 0,
                hotbar_slot: None,
            },
            _ => panic!("invalid block id"),
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::GRASS => "Grass",
            Self::DIRT => "Dirt",
            Self::STONE => "Stone",
            Self::COAL_ORE => "Coal ore",
            Self::IRON_ORE => "Iron ore",
            Self::WOOD => "Wood",
            Self::LEAVES => "Leaves",
            Self::TORCH => "Torch",
            Self::BEDROCK => "Bedrock",
            _ => panic!("invalid block id"),
        }
    }

    pub const fn breakable(self) -> bool {
        !matches!(self, Self::BEDROCK)
    }

    pub const fn code(self) -> u8 {
        self.0 as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        Self::new(code as u16)
    }
}

impl BlockState {
    pub const GRASS: Self = Self::from_id(BlockId::GRASS);
    pub const DIRT: Self = Self::from_id(BlockId::DIRT);
    pub const STONE: Self = Self::from_id(BlockId::STONE);
    pub const COAL_ORE: Self = Self::from_id(BlockId::COAL_ORE);
    pub const IRON_ORE: Self = Self::from_id(BlockId::IRON_ORE);
    pub const WOOD: Self = Self::from_id(BlockId::WOOD);
    pub const LEAVES: Self = Self::from_id(BlockId::LEAVES);
    pub const TORCH: Self = Self::from_id(BlockId::TORCH);
    pub const WALL_TORCH_LEFT: Self = Self {
        id: BlockId::TORCH,
        variant: TorchMount::WallLeft as u8,
    };
    pub const WALL_TORCH_RIGHT: Self = Self {
        id: BlockId::TORCH,
        variant: TorchMount::WallRight as u8,
    };
    pub const BEDROCK: Self = Self::from_id(BlockId::BEDROCK);

    pub const HOTBAR: [Self; 7] = [
        Self::DIRT,
        Self::STONE,
        Self::COAL_ORE,
        Self::IRON_ORE,
        Self::WOOD,
        Self::LEAVES,
        Self::TORCH,
    ];

    pub const ALL: [Self; 11] = [
        Self::GRASS,
        Self::DIRT,
        Self::STONE,
        Self::COAL_ORE,
        Self::IRON_ORE,
        Self::WOOD,
        Self::LEAVES,
        Self::TORCH,
        Self::BEDROCK,
        Self::WALL_TORCH_LEFT,
        Self::WALL_TORCH_RIGHT,
    ];

    pub const fn new(id: BlockId, variant: u8) -> Option<Self> {
        if variant == 0
            || (id.value() == BlockId::TORCH.value() && variant <= TorchMount::WallRight as u8)
        {
            Some(Self { id, variant })
        } else {
            None
        }
    }

    pub const fn from_id(id: BlockId) -> Self {
        Self { id, variant: 0 }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        if code == 10 {
            return Some(Self::WALL_TORCH_LEFT);
        }
        if code == 11 {
            return Some(Self::WALL_TORCH_RIGHT);
        }
        match BlockId::from_code(code) {
            Some(id) => Some(Self::from_id(id)),
            None => None,
        }
    }

    pub const fn id(self) -> BlockId {
        self.id
    }

    pub const fn variant(self) -> u8 {
        self.variant
    }

    pub const fn torch(mount: TorchMount) -> Self {
        Self {
            id: BlockId::TORCH,
            variant: mount as u8,
        }
    }

    pub const fn torch_mount(self) -> Option<TorchMount> {
        if self.id.value() != BlockId::TORCH.value() {
            return None;
        }
        match self.variant {
            0 => Some(TorchMount::Floor),
            1 => Some(TorchMount::WallLeft),
            2 => Some(TorchMount::WallRight),
            _ => None,
        }
    }

    pub const fn def(self) -> BlockDef {
        self.id.def()
    }

    pub const fn display_name(self) -> &'static str {
        self.id.display_name()
    }

    pub const fn breakable(self) -> bool {
        self.id.breakable()
    }

    pub const fn code(self) -> u8 {
        match self.torch_mount() {
            Some(TorchMount::WallLeft) => 10,
            Some(TorchMount::WallRight) => 11,
            _ => self.id.code(),
        }
    }
}

impl TorchMount {
    pub const fn support_offset(self) -> (i32, i32) {
        match self {
            Self::Floor => (0, -1),
            Self::WallLeft => (1, 0),
            Self::WallRight => (-1, 0),
        }
    }

    pub const fn from_placement_offset(x: i32, y: i32) -> Option<Self> {
        match (x, y) {
            (0, 1) => Some(Self::Floor),
            (-1, 0) => Some(Self::WallLeft),
            (1, 0) => Some(Self::WallRight),
            _ => None,
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
        let slots: HashSet<_> = BlockState::HOTBAR
            .iter()
            .map(|state| state.def().hotbar_slot)
            .collect();
        assert_eq!(slots.len(), 7);
        assert!(slots.contains(&Some(1)));
        assert!(slots.contains(&Some(7)));
        assert_eq!(BlockState::GRASS.def().hotbar_slot, None);
    }

    #[test]
    fn bedrock_is_the_only_unbreakable_block() {
        for state in BlockState::ALL {
            assert_eq!(state.breakable(), state != BlockState::BEDROCK);
        }
        assert!(BlockState::BEDROCK.def().hardness_seconds.is_infinite());
    }

    #[test]
    fn torch_is_non_solid_and_emissive() {
        let torch = BlockState::TORCH.def();
        assert!(!torch.solid);
        assert_eq!(torch.light_opacity, 0);
        assert_eq!(torch.emitted_light, 14);
        assert!(
            BlockState::ALL
                .into_iter()
                .filter(|state| state.def().emitted_light > 0)
                .all(|state| state.torch_mount().is_some())
        );
    }

    #[test]
    fn block_states_validate_ids_and_variants() {
        assert_eq!(BlockId::new(1), Some(BlockId::GRASS));
        assert_eq!(BlockId::new(9), Some(BlockId::BEDROCK));
        assert_eq!(BlockId::new(0), None);
        assert_eq!(BlockId::new(10), None);
        assert_eq!(BlockState::new(BlockId::STONE, 0), Some(BlockState::STONE));
        assert_eq!(BlockState::new(BlockId::STONE, 1), None);
        assert_eq!(
            BlockState::new(BlockId::TORCH, 1),
            Some(BlockState::WALL_TORCH_LEFT)
        );
        assert_eq!(
            BlockState::new(BlockId::TORCH, 2),
            Some(BlockState::WALL_TORCH_RIGHT)
        );
        assert_eq!(BlockState::new(BlockId::TORCH, 3), None);
    }

    #[test]
    fn dense_codes_reserve_zero_for_air() {
        assert_eq!(BlockState::from_code(0), None);
        for state in BlockState::ALL {
            assert_eq!(BlockState::from_code(state.code()), Some(state));
        }
        assert_eq!(BlockState::from_code(255), None);
    }

    #[test]
    fn torch_mounts_define_unambiguous_support_directions() {
        assert_eq!(TorchMount::Floor.support_offset(), (0, -1));
        assert_eq!(TorchMount::WallLeft.support_offset(), (1, 0));
        assert_eq!(TorchMount::WallRight.support_offset(), (-1, 0));
        assert_eq!(
            TorchMount::from_placement_offset(-1, 0),
            Some(TorchMount::WallLeft)
        );
        assert_eq!(TorchMount::from_placement_offset(0, -1), None);
    }
}
