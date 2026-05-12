//! Handler for the `gba list` command.
//!
//! Scans `.gba/` for feature directories, loads each `state.yml`, and
//! displays a summary table with colored status indicators.

use std::io::{self, Write};

use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use gba_core::{FeatureState, FeatureStatus, GbaContext};
use tracing::debug;

/// Summary of a single feature for display.
#[derive(Debug)]
struct FeatureSummary {
    /// Sequential feature number.
    number: u32,
    /// URL-friendly slug.
    slug: String,
    /// Current high-level status.
    status: FeatureStatus,
    /// Total turns consumed.
    turns: u32,
    /// Total cost in USD.
    cost_usd: f64,
    /// PR URL (empty if none).
    pr_url: String,
}

/// Execute the list command.
///
/// Discovers the project context, verifies initialization, scans `.gba/`
/// for feature directories, and prints a formatted summary table.
///
/// # Errors
///
/// Returns an error if context discovery fails, the project is not
/// initialized, or reading feature state fails.
pub async fn execute() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let ctx = GbaContext::load(&cwd).await?;
    if !ctx.is_initialized().await {
        anyhow::bail!("Project not initialized. Run `gba init` first.");
    }

    let features = list_features(&ctx).await?;
    if features.is_empty() {
        println!("No features found. Run `gba plan <slug>` to create one.");
        return Ok(());
    }

    // Print header
    println!(
        "{:<6} {:<30} {:<12} {:>6} {:>10} PR",
        "#", "Feature", "Status", "Turns", "Cost"
    );
    println!("{}", "\u{2500}".repeat(80));

    for feat in &features {
        let pr_str = if feat.pr_url.is_empty() {
            "\u{2014}".to_owned()
        } else {
            feat.pr_url.clone()
        };

        print_feature_row(feat, &pr_str)?;
    }

    Ok(())
}

/// Print a single feature row with colored status.
fn print_feature_row(feat: &FeatureSummary, pr_str: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    let color = status_color(&feat.status);
    let status_label = status_label(&feat.status);

    // Print number, slug (plain)
    write!(
        stdout,
        "{:<6} {:<30} ",
        format!("{:04}", feat.number),
        feat.slug
    )?;

    // Print colored status
    crossterm::execute!(
        stdout,
        SetForegroundColor(color),
        Print(format!("{status_label:<12}")),
        ResetColor,
    )?;

    // Print turns, cost, PR (plain)
    writeln!(
        stdout,
        " {:>6} {:>10} {}",
        feat.turns,
        format!("${:.2}", feat.cost_usd),
        pr_str,
    )?;

    stdout.flush()
}

/// Map a feature status to a crossterm color.
fn status_color(status: &FeatureStatus) -> Color {
    match status {
        FeatureStatus::Planned => Color::Yellow,
        FeatureStatus::Running => Color::Blue,
        FeatureStatus::Completed => Color::Green,
        FeatureStatus::Failed => Color::Red,
        FeatureStatus::Reviewing => Color::Magenta,
    }
}

/// Map a feature status to a display label.
fn status_label(status: &FeatureStatus) -> &'static str {
    match status {
        FeatureStatus::Planned => "Planned",
        FeatureStatus::Running => "Running",
        FeatureStatus::Completed => "Completed",
        FeatureStatus::Failed => "Failed",
        FeatureStatus::Reviewing => "Reviewing",
    }
}

/// Scan `.gba/` for feature directories and load summaries from `state.yml`.
///
/// Feature directories follow the naming convention `NNNN_<slug>`.
/// Directories without a valid `state.yml` are silently skipped.
async fn list_features(ctx: &GbaContext) -> anyhow::Result<Vec<FeatureSummary>> {
    let mut features = Vec::new();

    let mut entries = tokio::fs::read_dir(&ctx.gba_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Parse NNNN_<slug> format
        let Some((number, slug)) = parse_feature_dir_name(&name_str) else {
            continue;
        };

        // Must be a directory
        let Ok(meta) = tokio::fs::metadata(entry.path()).await else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }

        // Try to load state.yml
        let state_path = entry.path().join("state.yml");
        let Ok(state) = FeatureState::load(&state_path).await else {
            debug!(dir = %name_str, "skipping feature dir without valid state.yml");
            continue;
        };

        features.push(FeatureSummary {
            number,
            slug,
            status: state.status,
            turns: state.totals.turns,
            cost_usd: state.totals.cost_usd,
            pr_url: state.pr.url,
        });
    }

    // Sort by feature number
    features.sort_by_key(|f| f.number);

    Ok(features)
}

/// Parse a directory name in `NNNN_<slug>` format.
///
/// Returns `(number, slug)` if the name matches, `None` otherwise.
fn parse_feature_dir_name(name: &str) -> Option<(u32, String)> {
    let underscore_pos = name.find('_')?;

    let prefix = name.get(..underscore_pos)?;
    let slug = name.get(underscore_pos.saturating_add(1)..)?;

    // Prefix must be all digits and non-empty
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    // Slug must be non-empty
    if slug.is_empty() {
        return None;
    }

    let number = prefix.parse::<u32>().ok()?;
    Some((number, slug.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_parse_valid_feature_dir_name() {
        let result = parse_feature_dir_name("0001_web-frontend");
        assert_eq!(result, Some((1, "web-frontend".to_owned())));
    }

    #[test]
    fn test_should_parse_higher_number() {
        let result = parse_feature_dir_name("0042_api-service");
        assert_eq!(result, Some((42, "api-service".to_owned())));
    }

    #[test]
    fn test_should_reject_no_underscore() {
        let result = parse_feature_dir_name("config.yml");
        assert_eq!(result, None);
    }

    #[test]
    fn test_should_reject_non_numeric_prefix() {
        let result = parse_feature_dir_name("abc_feature");
        assert_eq!(result, None);
    }

    #[test]
    fn test_should_reject_empty_slug() {
        let result = parse_feature_dir_name("0001_");
        assert_eq!(result, None);
    }

    #[test]
    fn test_should_reject_empty_prefix() {
        let result = parse_feature_dir_name("_feature");
        assert_eq!(result, None);
    }

    #[test]
    fn test_should_map_status_colors() {
        assert_eq!(status_color(&FeatureStatus::Planned), Color::Yellow);
        assert_eq!(status_color(&FeatureStatus::Running), Color::Blue);
        assert_eq!(status_color(&FeatureStatus::Completed), Color::Green);
        assert_eq!(status_color(&FeatureStatus::Failed), Color::Red);
        assert_eq!(status_color(&FeatureStatus::Reviewing), Color::Magenta);
    }

    #[test]
    fn test_should_map_status_labels() {
        assert_eq!(status_label(&FeatureStatus::Planned), "Planned");
        assert_eq!(status_label(&FeatureStatus::Running), "Running");
        assert_eq!(status_label(&FeatureStatus::Completed), "Completed");
        assert_eq!(status_label(&FeatureStatus::Failed), "Failed");
        assert_eq!(status_label(&FeatureStatus::Reviewing), "Reviewing");
    }
}
