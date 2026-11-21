use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Store {
    Steam,
    Epic,
    Gog,
    Amazon,
    Local,
}

impl Store {
    pub fn as_str(self) -> &'static str {
        match self {
            Store::Steam => "steam",
            Store::Epic => "epic",
            Store::Gog => "gog",
            Store::Amazon => "amazon",
            Store::Local => "local",
        }
    }
}

/// Stable, store-qualified identifier, e.g. `steam:730`.
/// Used as the key for profiles and as the argument to `arkor run`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameId {
    pub store: Store,
    pub id: String,
}

impl GameId {
    pub fn new(store: Store, id: impl Into<String>) -> Self {
        Self {
            store,
            id: id.into(),
        }
    }

    /// Filesystem-safe form used for profile file names.
    pub fn file_stem(&self) -> String {
        let safe: String = self
            .id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}-{}", self.store.as_str(), safe)
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.store.as_str(), self.id)
    }
}

impl FromStr for GameId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (store, id) = s
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("game id must look like `store:id`, got `{s}`"))?;
        let store = match store {
            "steam" => Store::Steam,
            "epic" => Store::Epic,
            "gog" => Store::Gog,
            "amazon" => Store::Amazon,
            "local" => Store::Local,
            other => anyhow::bail!("unknown store `{other}`"),
        };
        anyhow::ensure!(!id.is_empty(), "empty game id");
        Ok(Self::new(store, id))
    }
}

#[derive(Debug, Clone)]
pub struct Game {
    pub id: GameId,
    pub name: String,
    pub install_dir: Option<PathBuf>,
    pub size_on_disk: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_id_roundtrip() {
        let id: GameId = "steam:730".parse().unwrap();
        assert_eq!(id, GameId::new(Store::Steam, "730"));
        assert_eq!(id.to_string(), "steam:730");
        assert_eq!(id.file_stem(), "steam-730");
        assert!("nostore".parse::<GameId>().is_err());
        assert!("foo:1".parse::<GameId>().is_err());
    }
}
