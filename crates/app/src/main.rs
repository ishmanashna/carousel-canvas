mod gui;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use core::{
    get_template_by_id, pin_strip_hero_fill, pick_dumb_fills, pick_smart_fills,
    resolve_strip_hero_argument, scan_image_folder, validate_strip_unique_sources, PythonRandom,
    TEMPLATE_IDS,
};
use render::{parse_color_rgb, run_strip_export, CardEdge, StripExportParams};

#[derive(Parser, Debug)]
#[command(
    name = "carousel-canvas",
    about = "Carousel Canvas - mural strip exporter (Rust)"
)]
struct Cli {
    /// Folder with images (jpg/png).
    #[arg(default_value = ".")]
    folder: std::path::PathBuf,

    /// Export carousel strip: random photos to chosen template, 10 vertical 1080x1350 JPEGs.
    #[arg(long)]
    strip: bool,

    /// Strip template id (slot count and layout vary; use --strip-layout-seed for collage layout).
    #[arg(long, default_value = "strip_mural_v2", value_parser = parse_template_id)]
    strip_template: String,

    /// Random slot assignment (default: match landscape/wide and portrait/tall slots).
    #[arg(long)]
    strip_dumb_shuffle: bool,

    /// Allow reusing the same files when the folder has fewer photos than image slots.
    #[arg(long)]
    strip_allow_repeats: bool,

    /// Layout RNG seed (mural jitter, polaroid scatter, seamless mosaic geometry).
    #[arg(long, default_value_t = 0)]
    strip_layout_seed: i64,

    /// Skip token-based layout retry (mural v2 borderless tries 5 seeds by default).
    #[arg(long)]
    strip_no_layout_retry: bool,

    /// Pin this image to the template flagship (hero) slot.
    #[arg(long, value_name = "PATH_OR_NAME")]
    strip_hero_image: Option<String>,

    /// Card edge style: borderless, wedges, or border (--color for border mode).
    #[arg(long, default_value = "borderless", value_parser = parse_card_edge)]
    strip_card_edge: String,

    /// Output folder for exported JPEGs.
    #[arg(long, default_value = "output")]
    output: std::path::PathBuf,

    /// Color name (e.g. beige) or hex (e.g. #FFFFFF) for border card edge.
    #[arg(long, default_value = "white")]
    color: String,
}

fn parse_template_id(s: &str) -> Result<String, String> {
    if TEMPLATE_IDS.contains(&s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "unknown template {s:?}; expected one of: {}",
            TEMPLATE_IDS.join(", ")
        ))
    }
}

fn parse_card_edge(s: &str) -> Result<String, String> {
    match s.to_ascii_lowercase().as_str() {
        "borderless" | "wedges" | "border" => Ok(s.to_ascii_lowercase()),
        other => Err(format!(
            "unknown card edge {other:?}; expected borderless, wedges, or border"
        )),
    }
}

fn assign_seed() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() & 0xFFFF_FFFF) as u32)
        .unwrap_or(42)
}

fn run_strip_export_cli(cli: &Cli, folder: &Path) -> Result<(), String> {
    let template = get_template_by_id(&cli.strip_template).map_err(|e| e.to_string())?;
    let paths = scan_image_folder(folder).map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Err("Strip needs at least one photo in the folder (H or V).".into());
    }

    let layout_seed = cli.strip_layout_seed;
    validate_strip_unique_sources(
        &template,
        &paths,
        cli.strip_allow_repeats,
        Some(layout_seed),
    )
    .map_err(|e| e.to_string())?;

    let seed = assign_seed();
    let mut fills = if cli.strip_dumb_shuffle {
        pick_dumb_fills(&paths, &template, Some(layout_seed), seed).map_err(|e| e.to_string())?
    } else {
        pick_smart_fills(&paths, &template, Some(layout_seed), seed).map_err(|e| e.to_string())?
    };

    if let Some(ref hero_spec) = cli.strip_hero_image {
        let hero_path = resolve_strip_hero_argument(folder, hero_spec).map_err(|e| e.to_string())?;
        let mut rng = PythonRandom::new(seed.wrapping_add(1));
        pin_strip_hero_fill(
            &mut fills,
            &template,
            &hero_path,
            Some(layout_seed),
            &paths,
            cli.strip_allow_repeats,
            &mut rng,
        )
        .map_err(|e| e.to_string())?;
        eprintln!("Pinned hero image: {}", hero_path.display());
    }

    let card_edge = CardEdge::parse(&cli.strip_card_edge).map_err(|e| e.to_string())?;
    let border_rgb = if card_edge == CardEdge::Border {
        Some(parse_color_rgb(&cli.color))
    } else {
        None
    };

    let params = StripExportParams {
        template,
        fills,
        slot_fills: None,
        source_paths: paths,
        layout_seed,
        card_edge,
        border_rgb,
        no_layout_retry: cli.strip_no_layout_retry,
        locked_layout: None,
        output_dir: cli.output.clone(),
    };

    let result = run_strip_export(params).map_err(|e| e.to_string())?;

    eprintln!("Wrote wide master {}", result.wide_path.display());
    for p in &result.slice_paths {
        eprintln!("Wrote {}", p.display());
    }
    eprintln!("\nAll tasks finished.");
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    if !cli.strip {
        if let Err(e) = gui::run_gui() {
            eprintln!("[FATAL] GUI: {e}");
            std::process::exit(1);
        }
        return;
    }

    let folder = cli
        .folder
        .canonicalize()
        .unwrap_or_else(|_| cli.folder.clone());
    if !folder.is_dir() {
        eprintln!("error: folder does not exist: {}", folder.display());
        std::process::exit(1);
    }

    if let Err(e) = run_strip_export_cli(&cli, &folder) {
        eprintln!("[FATAL] {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_is_ascii_safe() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(help.is_ascii(), "help text must be ASCII-safe for Windows cp1252");
    }

    #[test]
    fn cli_parses_defaults() {
        let cli = Cli::try_parse_from(["carousel-canvas", "--strip", "."]).unwrap();
        assert!(cli.strip);
        assert_eq!(cli.strip_template, "strip_mural_v2");
        assert_eq!(cli.strip_layout_seed, 0);
        assert_eq!(cli.strip_card_edge, "borderless");
        assert!(!cli.strip_allow_repeats);
        assert!(!cli.strip_dumb_shuffle);
        assert!(!cli.strip_no_layout_retry);
    }

    #[test]
    fn no_strip_launches_gui_mode_flag() {
        let cli = Cli::try_parse_from(["carousel-canvas"]).unwrap();
        assert!(!cli.strip);
    }
}
