use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    io,
    path::PathBuf,
};

pub const BLOCK_SIZE: u32 = 16;
pub const VARIANT_COUNT: usize = 4;
pub const PACK_SCHEMA_VERSION: u32 = 1;
pub const GENERATOR_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl BlockKind {
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
    pub const HOTBAR: [Self; 7] = [
        Self::Dirt,
        Self::Stone,
        Self::CoalOre,
        Self::IronOre,
        Self::Wood,
        Self::Leaves,
        Self::Torch,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Dirt => "dirt",
            Self::Stone => "stone",
            Self::CoalOre => "coal_ore",
            Self::IronOre => "iron_ore",
            Self::Wood => "wood",
            Self::Leaves => "leaves",
            Self::Torch => "torch",
            Self::Bedrock => "bedrock",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    Side,
    Top,
    Bottom,
}

impl Face {
    pub const ALL: [Self; 3] = [Self::Side, Self::Top, Self::Bottom];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Side => "side",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl PackImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, PackError> {
        if pixels.len() != width as usize * height as usize * 4 {
            return Err(PackError::Invalid(format!(
                "{width}x{height} RGBA image has {} bytes",
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width * height {
            pixels.extend_from_slice(&color);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * self.width + x) * 4) as usize;
        self.pixels[offset..offset + 4]
            .try_into()
            .expect("RGBA pixel")
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        let offset = ((y * self.width + x) * 4) as usize;
        self.pixels[offset..offset + 4].copy_from_slice(&color);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub player: PartialPlayerPalette,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartialPlayerPalette {
    pub skin: Option<[u8; 3]>,
    pub shirt: Option<[u8; 3]>,
    pub pants: Option<[u8; 3]>,
    pub hair: Option<[u8; 3]>,
    pub details: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerPalette {
    pub skin: [u8; 3],
    pub shirt: [u8; 3],
    pub pants: [u8; 3],
    pub hair: [u8; 3],
    pub details: [u8; 3],
}

impl Default for PlayerPalette {
    fn default() -> Self {
        Self {
            skin: [174, 111, 62],
            shirt: [25, 91, 105],
            pants: [31, 40, 57],
            hair: [55, 29, 16],
            details: [27, 22, 17],
        }
    }
}

impl PlayerPalette {
    pub(super) fn resolve(default: Self, custom: &PartialPlayerPalette) -> Self {
        Self {
            skin: custom.skin.unwrap_or(default.skin),
            shirt: custom.shirt.unwrap_or(default.shirt),
            pants: custom.pants.unwrap_or(default.pants),
            hair: custom.hair.unwrap_or(default.hair),
            details: custom.details.unwrap_or(default.details),
        }
    }

    pub(super) fn from_complete(partial: &PartialPlayerPalette) -> Result<Self, PackError> {
        Ok(Self {
            skin: required_color(partial.skin, "player.skin")?,
            shirt: required_color(partial.shirt, "player.shirt")?,
            pants: required_color(partial.pants, "player.pants")?,
            hair: required_color(partial.hair, "player.hair")?,
            details: required_color(partial.details, "player.details")?,
        })
    }
}

fn required_color(value: Option<[u8; 3]>, field: &str) -> Result<[u8; 3], PackError> {
    value.ok_or_else(|| PackError::Invalid(format!("default pack is missing {field}")))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenerationManifest {
    pub generator_version: u32,
    pub seed: u64,
    pub resolved: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPack {
    pub manifest: PackManifest,
    pub player: PlayerPalette,
    pub blocks: BTreeMap<(BlockKind, Face, usize), PackImage>,
    pub icons: BTreeMap<BlockKind, PackImage>,
    pub sun: PackImage,
    pub moons: Vec<PackImage>,
    pub stars: PackImage,
    pub cloud: PackImage,
    pub preview: PackImage,
}

impl ResolvedPack {
    pub fn block(&self, block: BlockKind, face: Face, variant: usize) -> &PackImage {
        &self.blocks[&(block, face, variant % VARIANT_COUNT)]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexturePackSummary {
    pub id: String,
    pub name: String,
    pub author: String,
    pub path: PathBuf,
    pub validation_error: Option<String>,
}

#[derive(Debug)]
pub enum PackError {
    Io(io::Error),
    TomlDecode(toml::de::Error),
    TomlEncode(toml::ser::Error),
    Image(image::ImageError),
    Invalid(String),
}

impl Display for PackError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::TomlDecode(error) => write!(formatter, "{error}"),
            Self::TomlEncode(error) => write!(formatter, "{error}"),
            Self::Image(error) => write!(formatter, "{error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for PackError {}

impl From<io::Error> for PackError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml::de::Error> for PackError {
    fn from(error: toml::de::Error) -> Self {
        Self::TomlDecode(error)
    }
}

impl From<toml::ser::Error> for PackError {
    fn from(error: toml::ser::Error) -> Self {
        Self::TomlEncode(error)
    }
}

impl From<image::ImageError> for PackError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}
