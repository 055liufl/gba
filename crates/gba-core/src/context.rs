//! GBA project context — project root discovery, configuration, and path resolution.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::GbaCoreError;

/// Default model used when no config file is present.
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Default maximum budget in USD.
const DEFAULT_MAX_BUDGET_USD: f64 = 10.0;

/// Per-preset configuration override.
///
/// Only `max_turns` is overridable, and it can only tighten (lower) the default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresetOverride {
    /// Override the maximum number of turns for this preset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

/// GBA project-level configuration loaded from `.gba/config.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GbaConfig {
    /// Model identifier to use for queries.
    #[serde(default = "default_model")]
    pub model: String,
    /// Maximum budget in USD for a single run.
    #[serde(default = "default_max_budget_usd")]
    pub max_budget_usd: f64,
    /// Per-preset overrides (key is the preset name, e.g., `"build"`).
    #[serde(default)]
    pub presets: HashMap<String, PresetOverride>,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_owned()
}

fn default_max_budget_usd() -> f64 {
    DEFAULT_MAX_BUDGET_USD
}

impl Default for GbaConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            max_budget_usd: default_max_budget_usd(),
            presets: HashMap::new(),
        }
    }
}

/// Project context: resolved paths and configuration.
///
/// Created by walking up from the current directory to find the project root.
#[derive(Debug, Clone)]
pub struct GbaContext {
    /// Absolute path to the project root (where `.git/` lives).
    pub project_root: PathBuf,
    /// Path to the `.gba/` directory.
    pub gba_dir: PathBuf,
    /// Path to the `.trees/` directory.
    pub trees_dir: PathBuf,
    /// Loaded configuration (defaults if no config file exists).
    pub config: GbaConfig,
}

impl GbaContext {
    /// Discover the project context by walking up from `cwd`.
    ///
    /// Stops when it finds a directory containing `.git/` or `.gba/`.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::ContextNotFound` if no project root is found.
    /// Returns `GbaCoreError::ConfigLoad` if a config file exists but is invalid.
    pub async fn load(cwd: &Path) -> Result<Self, GbaCoreError> {
        let project_root = find_project_root(cwd)?;
        let gba_dir = project_root.join(".gba");
        let trees_dir = project_root.join(".trees");

        let config = load_config_from_dir(&gba_dir).await?;

        Ok(Self {
            project_root,
            gba_dir,
            trees_dir,
            config,
        })
    }

    /// Check whether the project has been initialized (`.gba/` exists).
    pub async fn is_initialized(&self) -> bool {
        tokio::fs::try_exists(&self.gba_dir).await.unwrap_or(false)
    }

    /// Resolve the feature directory path by scanning `.gba/` for a directory
    /// matching `NNNN_<slug>`.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::FeatureNotFound` if no matching directory exists.
    /// Returns `GbaCoreError::StateLoad` if the `.gba/` directory cannot be read.
    pub async fn feature_dir(&self, slug: &str) -> Result<PathBuf, GbaCoreError> {
        let suffix = format!("_{slug}");

        if !tokio::fs::try_exists(&self.gba_dir).await.unwrap_or(false) {
            return Err(GbaCoreError::FeatureNotFound {
                slug: slug.to_owned(),
            });
        }

        let mut entries =
            tokio::fs::read_dir(&self.gba_dir)
                .await
                .map_err(|e| GbaCoreError::StateLoad {
                    path: self.gba_dir.clone(),
                    source: e.into(),
                })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| GbaCoreError::StateLoad {
                path: self.gba_dir.clone(),
                source: e.into(),
            })?
        {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.ends_with(&suffix)
                && let Some(prefix) = name_str.strip_suffix(&suffix)
                && !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_digit())
            {
                let path = entry.path();
                if let Ok(meta) = tokio::fs::metadata(&path).await
                    && meta.is_dir()
                {
                    return Ok(path);
                }
            }
        }

        Err(GbaCoreError::FeatureNotFound {
            slug: slug.to_owned(),
        })
    }

    /// Resolve the feature directory path with a known number.
    pub fn feature_dir_numbered(&self, number: u32, slug: &str) -> PathBuf {
        self.gba_dir.join(format!("{number:04}_{slug}"))
    }

    /// Determine the next feature number by scanning `.gba/` for existing
    /// numbered directories (format: `NNNN_<slug>`).
    ///
    /// Returns 1 if `.gba/` does not exist or is empty.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::StateLoad` if the directory cannot be read.
    pub async fn next_feature_number(&self) -> Result<u32, GbaCoreError> {
        if !tokio::fs::try_exists(&self.gba_dir).await.unwrap_or(false) {
            return Ok(1);
        }

        let mut max_number: u32 = 0;
        let mut entries =
            tokio::fs::read_dir(&self.gba_dir)
                .await
                .map_err(|e| GbaCoreError::StateLoad {
                    path: self.gba_dir.clone(),
                    source: e.into(),
                })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| GbaCoreError::StateLoad {
                path: self.gba_dir.clone(),
                source: e.into(),
            })?
        {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Parse directories matching NNNN_<slug>
            if let Some(num_str) = name_str.split('_').next()
                && let Ok(num) = num_str.parse::<u32>()
                && num > max_number
            {
                max_number = num;
            }
        }

        Ok(max_number.saturating_add(1))
    }

    /// Reload configuration from `.gba/config.yml`.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::ConfigLoad` if the file exists but is invalid.
    pub async fn load_config(&mut self) -> Result<(), GbaCoreError> {
        self.config = load_config_from_dir(&self.gba_dir).await?;
        Ok(())
    }

    /// Apply CLI overrides to the configuration.
    ///
    /// Merge precedence: CLI flags > config.yml > defaults.
    pub fn apply_overrides(&mut self, model: Option<&str>, budget: Option<f64>) {
        if let Some(m) = model {
            self.config.model = m.to_owned();
        }
        if let Some(b) = budget {
            self.config.max_budget_usd = b;
        }
    }
}

/// Walk up from `start` to find a directory containing `.git/` or `.gba/`.
fn find_project_root(start: &Path) -> Result<PathBuf, GbaCoreError> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() || current.join(".gba").exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(GbaCoreError::ContextNotFound {
                start_dir: start.to_owned(),
            });
        }
    }
}

/// Load `config.yml` from the given `.gba/` directory. Returns defaults if the
/// file does not exist.
async fn load_config_from_dir(gba_dir: &Path) -> Result<GbaConfig, GbaCoreError> {
    let config_path = gba_dir.join("config.yml");

    if !tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
        return Ok(GbaConfig::default());
    }

    let contents =
        tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| GbaCoreError::ConfigLoad {
                path: config_path.clone(),
                source: e.into(),
            })?;

    serde_yml::from_str(&contents).map_err(|e| GbaCoreError::ConfigLoad {
        path: config_path,
        source: e.into(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_should_load_context_from_git_dir() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join(".git");
        tokio::fs::create_dir(&git_dir).await.unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        assert_eq!(ctx.project_root, dir.path());
        assert_eq!(ctx.gba_dir, dir.path().join(".gba"));
    }

    #[tokio::test]
    async fn test_should_return_default_config_when_no_file() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        assert_eq!(ctx.config.model, DEFAULT_MODEL);
        assert!((ctx.config.max_budget_usd - DEFAULT_MAX_BUDGET_USD).abs() < f64::EPSILON);
        assert!(ctx.config.presets.is_empty());
    }

    #[tokio::test]
    async fn test_should_load_custom_config() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.unwrap();

        let config_yaml = r#"
model: "claude-opus-4"
max_budget_usd: 25.0
presets:
  build:
    max_turns: 50
"#;
        tokio::fs::write(gba_dir.join("config.yml"), config_yaml)
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        assert_eq!(ctx.config.model, "claude-opus-4");
        assert!((ctx.config.max_budget_usd - 25.0).abs() < f64::EPSILON);
        assert_eq!(
            ctx.config.presets.get("build").and_then(|p| p.max_turns),
            Some(50)
        );
    }

    #[tokio::test]
    async fn test_should_report_not_initialized() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        assert!(!ctx.is_initialized().await);
    }

    #[tokio::test]
    async fn test_should_report_initialized() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.path().join(".gba"))
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        assert!(ctx.is_initialized().await);
    }

    #[tokio::test]
    async fn test_should_return_next_feature_number() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.unwrap();
        tokio::fs::create_dir(gba_dir.join("0001_first-feature"))
            .await
            .unwrap();
        tokio::fs::create_dir(gba_dir.join("0003_third-feature"))
            .await
            .unwrap();
        // Also has a non-feature file (config.yml)
        tokio::fs::write(gba_dir.join("config.yml"), "model: test")
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        let next = ctx.next_feature_number().await.unwrap();
        assert_eq!(next, 4);
    }

    #[tokio::test]
    async fn test_should_return_one_for_empty_gba_dir() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.path().join(".gba"))
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        let next = ctx.next_feature_number().await.unwrap();
        assert_eq!(next, 1);
    }

    #[tokio::test]
    async fn test_should_return_error_when_no_project_root() {
        let dir = TempDir::new().unwrap();
        let result = GbaContext::load(dir.path()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaCoreError::ContextNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_should_resolve_feature_dir_numbered() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let ctx = GbaContext::load(dir.path()).await.unwrap();
        let path = ctx.feature_dir_numbered(3, "web-frontend");
        assert_eq!(path, dir.path().join(".gba").join("0003_web-frontend"));
    }

    #[tokio::test]
    async fn test_should_apply_model_override() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let mut ctx = GbaContext::load(dir.path()).await.unwrap();
        assert_eq!(ctx.config.model, DEFAULT_MODEL);

        ctx.apply_overrides(Some("claude-opus-4"), None);
        assert_eq!(ctx.config.model, "claude-opus-4");
        // Budget should remain default
        assert!((ctx.config.max_budget_usd - DEFAULT_MAX_BUDGET_USD).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_should_apply_budget_override() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let mut ctx = GbaContext::load(dir.path()).await.unwrap();
        ctx.apply_overrides(None, Some(50.0));
        assert!((ctx.config.max_budget_usd - 50.0).abs() < f64::EPSILON);
        // Model should remain default
        assert_eq!(ctx.config.model, DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn test_should_apply_both_overrides() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let mut ctx = GbaContext::load(dir.path()).await.unwrap();
        ctx.apply_overrides(Some("claude-opus-4"), Some(99.99));
        assert_eq!(ctx.config.model, "claude-opus-4");
        assert!((ctx.config.max_budget_usd - 99.99).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_should_not_change_when_no_overrides() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();

        let mut ctx = GbaContext::load(dir.path()).await.unwrap();
        let original_model = ctx.config.model.clone();
        let original_budget = ctx.config.max_budget_usd;

        ctx.apply_overrides(None, None);
        assert_eq!(ctx.config.model, original_model);
        assert!((ctx.config.max_budget_usd - original_budget).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_should_override_config_file_values() {
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir(dir.path().join(".git"))
            .await
            .unwrap();
        let gba_dir = dir.path().join(".gba");
        tokio::fs::create_dir(&gba_dir).await.unwrap();

        let config_yaml = "model: \"claude-sonnet-4\"\nmax_budget_usd: 15.0\n";
        tokio::fs::write(gba_dir.join("config.yml"), config_yaml)
            .await
            .unwrap();

        let mut ctx = GbaContext::load(dir.path()).await.unwrap();
        assert_eq!(ctx.config.model, "claude-sonnet-4");
        assert!((ctx.config.max_budget_usd - 15.0).abs() < f64::EPSILON);

        // CLI override takes precedence
        ctx.apply_overrides(Some("claude-opus-4"), Some(100.0));
        assert_eq!(ctx.config.model, "claude-opus-4");
        assert!((ctx.config.max_budget_usd - 100.0).abs() < f64::EPSILON);
    }
}
