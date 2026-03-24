use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub branding: BrandingConfig,
    pub toolchain: ToolchainConfig,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            server: ServerConfig {
                host: env::var("MM_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("MM_PORT")
                    .ok()
                    .and_then(|v| v.parse::<u16>().ok())
                    .unwrap_or(8080),
            },
            branding: BrandingConfig {
                app_name: env::var("MM_BRANDING_APP_NAME")
                    .unwrap_or_else(|_| "Media Manager".to_string()),
                short_name: env::var("MM_BRANDING_SHORT_NAME")
                    .unwrap_or_else(|_| "MM".to_string()),
                logo_url: env::var("MM_BRANDING_LOGO_URL")
                    .unwrap_or_else(|_| "/assets/logo.svg".to_string()),
                browser_title_template: env::var("MM_BRANDING_BROWSER_TITLE_TEMPLATE")
                    .unwrap_or_else(|_| "{app_name}".to_string()),
                theme_tokens: BrandingThemeTokens {
                    accent: env::var("MM_BRANDING_ACCENT").unwrap_or_else(|_| "#0f766e".to_string()),
                    accent_contrast: env::var("MM_BRANDING_ACCENT_CONTRAST")
                        .unwrap_or_else(|_| "#f8fafc".to_string()),
                },
            },
            toolchain: ToolchainConfig {
                ffmpeg_path: env_path("MM_FFMPEG_PATH"),
                ffprobe_path: env_path("MM_FFPROBE_PATH"),
                mediainfo_path: env_path("MM_MEDIAINFO_PATH"),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandingConfig {
    pub app_name: String,
    pub short_name: String,
    pub logo_url: String,
    pub browser_title_template: String,
    pub theme_tokens: BrandingThemeTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrandingThemeTokens {
    pub accent: String,
    pub accent_contrast: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainConfig {
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub mediainfo_path: Option<PathBuf>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var(key).ok().map(PathBuf::from)
}
