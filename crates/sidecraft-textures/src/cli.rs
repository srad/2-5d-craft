use crate::recipe::Recipe;
use clap::{Args, Parser, Subcommand};
use sidecraft_textures::{
    PackError, initialize_recipe, load_resolved_pack, validate_pack, write_generated_pack,
    write_preview,
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Parser)]
#[command(
    name = "sidecraft-textures",
    version,
    about = "Generate and validate deterministic voxel texture packs"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Generate(Box<GenerateArgs>),
    Init {
        #[arg(default_value = "texture-generator.toml")]
        path: PathBuf,
    },
    Validate {
        path: PathBuf,
        #[arg(long)]
        complete: bool,
    },
    Preview {
        path: PathBuf,
        #[arg(long, default_value = "assets/texture-packs/default")]
        default: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Args)]
struct GenerateArgs {
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=64))]
    count: u32,
    #[arg(long, default_value = "texture-packs")]
    output: PathBuf,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    name_prefix: Option<String>,
    #[arg(long)]
    author: Option<String>,
    #[arg(long)]
    recipe: Option<PathBuf>,
    #[arg(long)]
    palette: Option<String>,
    #[arg(long)]
    pattern: Option<String>,
    #[arg(long)]
    placement: Option<String>,
    #[arg(long)]
    cluster_shape: Option<String>,
    #[arg(long)]
    cluster_size: Option<String>,
    #[arg(long)]
    cluster_density: Option<String>,
    #[arg(long)]
    smoothing_passes: Option<String>,
    #[arg(long)]
    contrast: Option<String>,
    #[arg(long)]
    saturation: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    lightness: Option<String>,
    #[arg(long)]
    variant_strength: Option<String>,
    #[arg(long)]
    ore_pattern: Option<String>,
    #[arg(long)]
    ore_coverage: Option<String>,
    #[arg(long)]
    ore_branches: Option<String>,
    #[arg(long)]
    ore_thickness: Option<String>,
    #[arg(long)]
    ore_center_bias: Option<String>,
    #[arg(long)]
    leaf_hole_density: Option<String>,
    #[arg(long)]
    grass_fringe_depth: Option<String>,
    #[arg(long)]
    quality: Option<String>,
    #[arg(long = "set", value_name = "MATERIAL.FIELD=VALUE")]
    set_overrides: Vec<String>,
}

impl Default for GenerateArgs {
    fn default() -> Self {
        Self {
            seed: None,
            count: 1,
            output: PathBuf::from("texture-packs"),
            id: None,
            name_prefix: None,
            author: None,
            recipe: None,
            palette: None,
            pattern: None,
            placement: None,
            cluster_shape: None,
            cluster_size: None,
            cluster_density: None,
            smoothing_passes: None,
            contrast: None,
            saturation: None,
            lightness: None,
            variant_strength: None,
            ore_pattern: None,
            ore_coverage: None,
            ore_branches: None,
            ore_thickness: None,
            ore_center_bias: None,
            leaf_hole_density: None,
            grass_fringe_depth: None,
            quality: None,
            set_overrides: Vec::new(),
        }
    }
}

impl Cli {
    pub(crate) fn run(self) -> Result<(), PackError> {
        match self.command {
            Some(Command::Generate(arguments)) => generate(*arguments),
            Some(Command::Init { path }) => {
                initialize_recipe(&path)?;
                println!("initialized {}", path.display());
                Ok(())
            }
            Some(Command::Validate { path, complete }) => {
                validate_pack(&path, complete)?;
                println!("valid {}", path.display());
                Ok(())
            }
            Some(Command::Preview {
                path,
                default,
                output,
            }) => {
                let is_default = path == default;
                let pack = load_resolved_pack(
                    &default,
                    if is_default {
                        None
                    } else {
                        Some(path.as_path())
                    },
                )?;
                let output = output.unwrap_or_else(|| path.join("preview.png"));
                write_preview(&output, &pack.preview)?;
                println!("wrote {}", output.display());
                Ok(())
            }
            None => generate(GenerateArgs::default()),
        }
    }
}

fn generate(arguments: GenerateArgs) -> Result<(), PackError> {
    if arguments.id.is_some() && arguments.count != 1 {
        return Err(PackError::Invalid(
            "--id can only be used when --count is 1".into(),
        ));
    }
    let base_seed = arguments.seed.unwrap_or_else(random_seed);
    let mut recipe = if let Some(path) = &arguments.recipe {
        Recipe::read(path)?
    } else {
        Recipe::default()
    };
    recipe.merge(arguments.recipe_overlay());

    for index in 0..arguments.count {
        let seed = base_seed.wrapping_add(u64::from(index).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut options = recipe.resolve(seed, &arguments.set_overrides)?;
        options.author = arguments
            .author
            .clone()
            .unwrap_or_else(|| options.author.clone());
        let id_prefix = arguments.name_prefix.as_deref().unwrap_or("generated");
        options.id = Some(
            arguments
                .id
                .clone()
                .unwrap_or_else(|| format!("{}-{seed}", slug(id_prefix))),
        );
        options.name = Some(format!(
            "{} {seed}",
            arguments.name_prefix.as_deref().unwrap_or("Generated")
        ));
        let pack = sidecraft_textures::generate_pack(&options)?;
        let path = write_generated_pack(&pack, &arguments.output)?;
        println!("generated {}", path.display());
        println!("seed = {}", pack.generation.seed);
        for (field, value) in &pack.generation.resolved {
            println!("{field} = {value}");
        }
    }
    Ok(())
}

impl GenerateArgs {
    fn recipe_overlay(&self) -> Recipe {
        Recipe {
            palette: self.palette.clone(),
            pattern: self.pattern.clone(),
            placement: self.placement.clone(),
            cluster_shape: self.cluster_shape.clone(),
            cluster_size: self.cluster_size.clone(),
            cluster_density: self.cluster_density.clone(),
            smoothing_passes: self.smoothing_passes.clone(),
            contrast: self.contrast.clone(),
            saturation: self.saturation.clone(),
            lightness: self.lightness.clone(),
            variant_strength: self.variant_strength.clone(),
            ore_pattern: self.ore_pattern.clone(),
            ore_coverage: self.ore_coverage.clone(),
            ore_branches: self.ore_branches.clone(),
            ore_thickness: self.ore_thickness.clone(),
            ore_center_bias: self.ore_center_bias.clone(),
            leaf_hole_density: self.leaf_hole_density.clone(),
            grass_fringe_depth: self.grass_fringe_depth.clone(),
            quality: self.quality.clone(),
            material: Default::default(),
        }
    }
}

fn random_seed() -> u64 {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    time ^ u64::from(std::process::id()).rotate_left(17)
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_hyphen = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_hyphen = false;
        } else if !previous_hyphen && !slug.is_empty() {
            slug.push('-');
            previous_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "generated".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_select_random_generation() {
        let cli = Cli::try_parse_from(["sidecraft-textures"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn generator_accepts_every_control_family() {
        let cli = Cli::try_parse_from([
            "sidecraft-textures",
            "generate",
            "--seed",
            "4",
            "--pattern",
            "short-walks",
            "--placement",
            "poisson-disc",
            "--cluster-size",
            "2..5",
            "--lightness",
            "-0.03",
            "--ore-pattern",
            "center-growth,branching-walk",
            "--set",
            "stone.contrast=1.05..1.1",
        ])
        .unwrap();
        assert!(matches!(cli.command, Some(Command::Generate(_))));
    }

    #[test]
    fn names_are_safe_pack_ids() {
        assert_eq!(slug("My Rich Pack!"), "my-rich-pack");
    }
}
