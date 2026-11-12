//! Command-line test tool. Runs a folder of jewelry photos through the
//! chosen output styles and writes the results with a side-by-side report,
//! so prompt wording can be tuned before the desktop app is built.

mod env;
mod pipeline;
mod report;

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use chrono::Local;
use clap::Parser;
use studio_core::cost::format_usd;
use studio_core::gemini::GeminiClient;
use studio_core::library::{Library, StyleTemplate};

use crate::pipeline::RunSettings;

const KEY_HELP: &str = "no Gemini API key found.\n\
    1. Get a key at https://aistudio.google.com (\"Get API key\").\n\
    2. Copy the file .env.example to .env in this folder.\n\
    3. Paste the key after GEMINI_API_KEY= and save.";

const BILLING_HELP: &str = "this API key's Google project is on the free tier, \
    and Google's image models are not included in the free tier.\n\
    1. Open https://ai.dev/rate-limit, signed in with the Google account that owns the key.\n\
    2. Set up billing for the project (the page shows the current tier and how to upgrade).\n\
    3. Run this again. Nothing was charged for this attempt.";

#[derive(Parser, Debug)]
#[command(
    name = "poc-cli",
    version,
    about = "Generates studio-style product photos of jewelry from reference photos."
)]
struct Args {
    /// Folder of reference photos. A sub-folder named after a category
    /// (ring, necklace, ...) sets the category of the photos inside it.
    #[arg(long, default_value = "samples")]
    input: PathBuf,

    /// Folder where each run's results are written.
    #[arg(long, default_value = "output")]
    output: PathBuf,

    /// Folder holding master.toml, pricing.toml and styles/.
    #[arg(long, default_value = "library")]
    library: PathBuf,

    /// Style ids to generate, separated by commas. Default: every style.
    #[arg(long, value_delimiter = ',')]
    styles: Vec<String>,

    /// Category for photos that are not in a category sub-folder.
    #[arg(long)]
    category: Option<String>,

    /// How many options to generate for each photo and style.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=8))]
    variants: u32,

    /// Model id. Default: default_model in pricing.toml.
    #[arg(long)]
    model: Option<String>,

    /// Image size: 1K, 2K or 4K. Default: default_size in pricing.toml.
    #[arg(long)]
    size: Option<String>,

    /// Aspect ratio such as 1:1 or 4:5. Default: follow the reference photo.
    #[arg(long)]
    aspect: Option<String>,

    /// Small adjustment to the scene, for example "slightly warmer light".
    #[arg(long)]
    note: Option<String>,

    /// How many images to generate at the same time.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=8))]
    concurrency: u32,

    /// Show the plan, the cost estimate and one full prompt, then stop.
    #[arg(long)]
    dry_run: bool,

    /// Do not ask for confirmation before spending money.
    #[arg(long, short = 'y')]
    yes: bool,

    /// List the styles, categories and models in the library, then stop.
    #[arg(long)]
    list: bool,

    /// Check that the API key works and the model is available, then stop.
    #[arg(long)]
    check: bool,

    /// Open the review page in the browser when the run finishes.
    #[arg(long)]
    open: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nError: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    let library = Library::load(&args.library).map_err(|e| e.to_string())?;
    let pricing = &library.pricing;
    let model = args.model.unwrap_or_else(|| pricing.default_model.clone());
    let size = args.size.unwrap_or_else(|| pricing.default_size.clone());

    if args.list {
        print_library(&library);
        return Ok(());
    }
    if args.check {
        return check_key(&model).await;
    }

    let per_image = pricing.per_image(&model, &size);
    if let (Some(known), None) = (pricing.model(&model), per_image) {
        let sizes: Vec<&str> = known.per_image.keys().map(String::as_str).collect();
        return Err(format!(
            "{model} has no price on file for size {size}. Sizes on file: {}",
            sizes.join(", ")
        ));
    }

    let styles = select_styles(&library, &args.styles)?;
    let items = pipeline::discover_items(&args.input, &library, args.category.as_deref())?;
    if items.is_empty() {
        return Err(format!(
            "no photos found in {}. Put JPG, PNG or WebP photos there, \
             or in a sub-folder named after a category such as \"ring\".",
            args.input.display()
        ));
    }
    let item_count = items.len();
    let plan = pipeline::plan(items, &styles, &library, args.note.as_deref())?;
    let total = plan.image_count(args.variants);
    if total == 0 {
        return Err("none of the chosen styles is set up for these photos' categories".into());
    }
    let estimate = per_image.map(|price| price * total as f64);

    println!(
        "{item_count} photo(s) x {} style(s) x {} option(s) = {total} image(s)",
        styles.len(),
        args.variants
    );
    println!("Model: {model}, size {size}");
    match (per_image, estimate) {
        (Some(price), Some(estimate)) => println!(
            "Estimated cost: {} ({} per image, prices checked {})",
            format_usd(estimate),
            format_usd(price),
            pricing.checked_on
        ),
        _ => println!("Estimated cost: unknown, {model} has no price in pricing.toml"),
    }
    for skipped in &plan.skipped {
        println!("Skipping {skipped}");
    }

    if args.dry_run {
        if let Some(first) = plan.items.first().and_then(|item| item.styles.first()) {
            println!(
                "\n--- Prompt for {} with {} ---\n{}",
                plan.items[0].item.id, first.style.id, first.prompt.text
            );
        }
        println!("\nDry run: nothing was sent and nothing was charged.");
        return Ok(());
    }

    let api_key = env::load_api_key(Path::new(".env")).ok_or(KEY_HELP)?;
    if !args.yes && !confirm(&format!("Generate {total} image(s)?"))? {
        println!("Nothing was generated.");
        return Ok(());
    }

    let now = Local::now();
    let run_dir = args.output.join(now.format("%Y%m%d-%H%M%S").to_string());
    let settings = RunSettings {
        started_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        model,
        image_size: Some(size),
        aspect_ratio: args.aspect,
        variants: args.variants,
        concurrency: args.concurrency as usize,
        per_image_usd: per_image,
        note: args.note,
    };
    let client = Arc::new(new_client(api_key)?);

    println!();
    let manifest =
        pipeline::execute(client, &plan, library.master.version, &settings, &run_dir).await?;

    println!(
        "\n{} image(s) saved, {} failed.",
        manifest.saved(),
        manifest.failed()
    );
    if let Some(cost) = manifest.cost_usd() {
        println!("Cost of the saved images: about {}", format_usd(cost));
    }
    if manifest.saved() == 0 {
        // Google words a missing billing setup as a rate limit of zero.
        if manifest.errors().any(|e| e.contains("Free Tier")) {
            return Err(BILLING_HELP.into());
        }
        return Err("no image could be generated; see the messages above".into());
    }

    let report = run_dir.join("report.html");
    println!("Open this file to review them: {}", report.display());
    if args.open {
        open_in_browser(&report);
    }
    Ok(())
}

/// Best effort: the path has already been printed if this does nothing.
fn open_in_browser(report: &Path) {
    if cfg!(windows) {
        if let Ok(absolute) = std::path::absolute(report) {
            let _ = std::process::Command::new("explorer").arg(absolute).spawn();
        }
    }
}

/// `GEMINI_BASE_URL` redirects requests to a local test server or a proxy.
fn new_client(api_key: String) -> Result<GeminiClient, String> {
    let client = GeminiClient::new(api_key).map_err(|e| e.to_string())?;
    Ok(match std::env::var("GEMINI_BASE_URL") {
        Ok(base_url) if !base_url.trim().is_empty() => client.with_base_url(base_url.trim()),
        _ => client,
    })
}

fn select_styles<'a>(
    library: &'a Library,
    requested: &[String],
) -> Result<Vec<&'a StyleTemplate>, String> {
    if requested.is_empty() {
        return Ok(library.styles.iter().collect());
    }
    requested
        .iter()
        .map(|id| {
            library.style(id.trim()).ok_or_else(|| {
                let known: Vec<&str> = library.styles.iter().map(|s| s.id.as_str()).collect();
                format!("unknown style \"{id}\". Available: {}", known.join(", "))
            })
        })
        .collect()
}

fn print_library(library: &Library) {
    println!("Styles:");
    for style in &library.styles {
        println!(
            "  {:<16} v{:<3} {}",
            style.id, style.version, style.description
        );
    }
    println!("\nCategories:");
    for category in &library.master.categories {
        println!("  {:<16} {}", category.id, category.label);
    }
    let pricing = &library.pricing;
    println!(
        "\nModels (prices per image, checked {}):",
        pricing.checked_on
    );
    for model in &pricing.models {
        let prices: Vec<String> = model
            .per_image
            .iter()
            .map(|(size, price)| format!("{size} {}", format_usd(*price)))
            .collect();
        let default = if model.id == pricing.default_model {
            " (default)"
        } else {
            ""
        };
        println!("  {}{default}: {}", model.id, prices.join(", "));
        if !model.note.is_empty() {
            println!("      {}", model.note);
        }
    }
}

async fn check_key(model: &str) -> Result<(), String> {
    let api_key = env::load_api_key(Path::new(".env")).ok_or(KEY_HELP)?;
    let client = new_client(api_key)?;
    let models = client.list_models().await.map_err(|e| e.to_string())?;
    println!("The API key works.");

    if models.iter().any(|name| name == model) {
        println!("Model {model} is available.");
        return Ok(());
    }
    let image_models: Vec<&str> = models
        .iter()
        .map(String::as_str)
        .filter(|name| name.contains("image") || name.contains("banana"))
        .collect();
    Err(format!(
        "model {model} is not in the list this key can use.\nImage models this key can use: {}",
        if image_models.is_empty() {
            "none found".to_owned()
        } else {
            image_models.join(", ")
        }
    ))
}

fn confirm(question: &str) -> Result<bool, String> {
    print!("{question} [y/N] ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}
