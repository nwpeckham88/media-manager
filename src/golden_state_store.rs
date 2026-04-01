use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProvider {
    Tmdb,
    Imdb,
    Tvdb,
}

impl MetadataProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tmdb => "tmdb",
            Self::Imdb => "imdb",
            Self::Tvdb => "tvdb",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tmdb" => Some(Self::Tmdb),
            "imdb" => Some(Self::Imdb),
            "tvdb" => Some(Self::Tvdb),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NamingFormat {
    MovieTitleYear,
    MovieTitleSubtitleYear,
}

impl NamingFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MovieTitleYear => "movie_title_year",
            Self::MovieTitleSubtitleYear => "movie_title_subtitle_year",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "movie_title_year" => Some(Self::MovieTitleYear),
            "movie_title_subtitle_year" => Some(Self::MovieTitleSubtitleYear),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenStatePreferences {
    pub metadata_provider: MetadataProvider,
    pub naming_format: NamingFormat,
    pub updated_at_ms: i64,
}

impl Default for GoldenStatePreferences {
    fn default() -> Self {
        Self {
            metadata_provider: MetadataProvider::Tmdb,
            naming_format: NamingFormat::MovieTitleSubtitleYear,
            updated_at_ms: 0,
        }
    }
}

#[derive(Debug, Error)]
pub enum GoldenStateStoreError {
    #[error("sqlite open failed: {0}")]
    Open(String),
    #[error("sqlite query failed: {0}")]
    Query(String),
}

pub fn load(db_path: &Path) -> Result<GoldenStatePreferences, GoldenStateStoreError> {
    let conn = Connection::open(db_path).map_err(|e| GoldenStateStoreError::Open(e.to_string()))?;

    let row = conn.query_row(
        "SELECT metadata_provider, naming_format, updated_at_ms FROM golden_state_preferences WHERE id = 1",
        [],
        |r| {
            let metadata_provider: String = r.get(0)?;
            let naming_format: String = r.get(1)?;
            let updated_at_ms: i64 = r.get(2)?;
            Ok((metadata_provider, naming_format, updated_at_ms))
        },
    );

    match row {
        Ok((metadata_provider, naming_format, updated_at_ms)) => {
            let provider = MetadataProvider::parse(&metadata_provider).unwrap_or(MetadataProvider::Tmdb);
            let format = NamingFormat::parse(&naming_format).unwrap_or(NamingFormat::MovieTitleSubtitleYear);
            Ok(GoldenStatePreferences {
                metadata_provider: provider,
                naming_format: format,
                updated_at_ms,
            })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(GoldenStatePreferences::default()),
        Err(err) => Err(GoldenStateStoreError::Query(err.to_string())),
    }
}

pub fn save(
    db_path: &Path,
    metadata_provider: MetadataProvider,
    naming_format: NamingFormat,
    updated_at_ms: i64,
) -> Result<GoldenStatePreferences, GoldenStateStoreError> {
    let conn = Connection::open(db_path).map_err(|e| GoldenStateStoreError::Open(e.to_string()))?;

    conn.execute(
        "INSERT INTO golden_state_preferences(id, metadata_provider, naming_format, updated_at_ms)
         VALUES(1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
           metadata_provider = excluded.metadata_provider,
           naming_format = excluded.naming_format,
           updated_at_ms = excluded.updated_at_ms",
        params![metadata_provider.as_str(), naming_format.as_str(), updated_at_ms],
    )
    .map_err(|e| GoldenStateStoreError::Query(e.to_string()))?;

    Ok(GoldenStatePreferences {
        metadata_provider,
        naming_format,
        updated_at_ms,
    })
}
