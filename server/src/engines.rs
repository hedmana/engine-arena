use std::{
    collections::BTreeMap,
    fmt, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Mutex};

use crate::uci::Engine;

pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
/// The first `uv run` of a script may resolve and download its dependencies.
const HANDSHAKE: Duration = Duration::from_secs(60);
const STOCKFISH_ID: &str = "stockfish";
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Python,
    Binary,
    Stockfish,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineEntry {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub uci_name: String,
    #[serde(default)]
    pub author: String,
    /// UCI options applied after every start.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

pub struct Upload {
    pub name: Option<String>,
    pub python: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum RegistryError {
    NotFound,
    Invalid(String),
    Io(io::Error),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("engine not found"),
            Self::Invalid(message) => f.write_str(message),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for RegistryError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

type Result<T> = std::result::Result<T, RegistryError>;

/// Engines stored as `<dir>/<id>/manifest.json` plus the uploaded file. Every path is
/// server-generated; ids are validated before they touch the filesystem.
pub struct Registry {
    dir: PathBuf,
    stockfish: String,
    writes: Mutex<()>,
}

impl Registry {
    pub async fn open(dir: &Path, stockfish: String) -> io::Result<Self> {
        fs::create_dir_all(dir).await?;
        let registry = Self {
            dir: fs::canonicalize(dir).await?,
            stockfish,
            writes: Mutex::new(()),
        };
        if registry.get(STOCKFISH_ID).await.is_err() {
            let entry = EngineEntry {
                id: STOCKFISH_ID.into(),
                name: "Stockfish".into(),
                kind: Kind::Stockfish,
                uci_name: String::new(),
                author: String::new(),
                options: BTreeMap::from([
                    ("Threads".into(), "1".into()),
                    ("Hash".into(), "64".into()),
                ]),
            };
            fs::create_dir_all(registry.dir.join(STOCKFISH_ID)).await?;
            registry.save(&entry).await?;
        }
        Ok(registry)
    }

    pub async fn list(&self) -> io::Result<Vec<EngineEntry>> {
        let mut entries = Vec::new();
        let mut dirs = fs::read_dir(&self.dir).await?;
        while let Some(dir) = dirs.next_entry().await? {
            let id = dir.file_name().to_string_lossy().into_owned();
            if let Ok(entry) = self.get(&id).await {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|e| (e.kind != Kind::Stockfish, e.id.parse::<u64>().unwrap_or(0)));
        Ok(entries)
    }

    pub async fn get(&self, id: &str) -> Result<EngineEntry> {
        let json = fs::read(self.engine_dir(id)?.join(MANIFEST))
            .await
            .map_err(|_| RegistryError::NotFound)?;
        serde_json::from_slice(&json).map_err(|e| RegistryError::Io(e.into()))
    }

    /// Validates the upload with a UCI handshake in a staging directory, then publishes it.
    pub async fn create(&self, upload: Upload) -> Result<EngineEntry> {
        let _guard = self.writes.lock().await;
        let requested = upload
            .name
            .as_deref()
            .map(valid_name)
            .transpose()?
            .filter(|n| !n.is_empty());
        let (staging, mut entry) = self.stage(&upload).await?;
        let mut next = 1;
        for existing in self.list().await? {
            next = next.max(existing.id.parse::<u64>().unwrap_or(0) + 1);
        }
        entry.id = next.to_string();
        let reported = valid_name(&entry.uci_name).ok().filter(|n| !n.is_empty());
        entry.name = requested
            .or(reported)
            .unwrap_or_else(|| format!("Engine {next}"));
        let dir = self.dir.join(&entry.id);
        fs::rename(&staging, &dir).await?;
        self.save(&entry).await?;
        Ok(entry)
    }

    /// Replaces an uploaded engine's file, keeping its name and options.
    pub async fn replace(&self, id: &str, upload: Upload) -> Result<EngineEntry> {
        let _guard = self.writes.lock().await;
        let mut entry = self.get(id).await?;
        if entry.kind == Kind::Stockfish {
            return Err(RegistryError::Invalid(
                "the built-in Stockfish cannot be replaced".into(),
            ));
        }
        let (staging, staged) = self.stage(&upload).await?;
        let dir = self.engine_dir(id)?;
        let _ = fs::remove_file(dir.join(file_name(entry.kind))).await;
        fs::rename(
            staging.join(file_name(staged.kind)),
            dir.join(file_name(staged.kind)),
        )
        .await?;
        let _ = fs::remove_dir_all(&staging).await;
        (entry.kind, entry.uci_name, entry.author) = (staged.kind, staged.uci_name, staged.author);
        self.save(&entry).await?;
        Ok(entry)
    }

    pub async fn update(
        &self,
        id: &str,
        name: Option<String>,
        options: Option<BTreeMap<String, String>>,
    ) -> Result<EngineEntry> {
        let _guard = self.writes.lock().await;
        let mut entry = self.get(id).await?;
        if let Some(name) = name {
            entry.name = valid_name(&name)?;
            if entry.name.is_empty() {
                return Err(RegistryError::Invalid("name must not be empty".into()));
            }
        }
        if let Some(options) = options {
            for (key, value) in &options {
                let valid = |s: &str, max| s.len() <= max && !s.chars().any(char::is_control);
                if key.trim().is_empty() || !valid(key, 64) || !valid(value, 256) {
                    return Err(RegistryError::Invalid(format!("invalid option {key:?}")));
                }
            }
            entry.options = options;
        }
        self.save(&entry).await?;
        Ok(entry)
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.writes.lock().await;
        if self.get(id).await?.kind == Kind::Stockfish {
            return Err(RegistryError::Invalid(
                "the built-in Stockfish cannot be deleted".into(),
            ));
        }
        Ok(fs::remove_dir_all(self.engine_dir(id)?).await?)
    }

    /// Starts the engine in its own directory and applies its options.
    pub async fn start(&self, entry: &EngineEntry, limit: Duration) -> io::Result<Engine> {
        let dir = self.dir.join(&entry.id);
        let mut engine = self.spawn(entry.kind, &dir, limit).await?;
        for (name, value) in &entry.options {
            engine.set_option(name, value).await?;
        }
        engine.ready(limit).await?;
        Ok(engine)
    }

    async fn spawn(&self, kind: Kind, dir: &Path, limit: Duration) -> io::Result<Engine> {
        let file = dir.join(file_name(kind)).to_string_lossy().into_owned();
        let (program, args) = match kind {
            Kind::Python => (
                "uv",
                vec!["run".into(), "--quiet".into(), "--script".into(), file],
            ),
            Kind::Binary => (file.as_str(), Vec::new()),
            Kind::Stockfish => (self.stockfish.as_str(), Vec::new()),
        };
        match Engine::start(program, &args, Some(dir), limit).await {
            Err(e) if e.kind() == io::ErrorKind::NotFound && kind == Kind::Python => Err(
                io::Error::new(e.kind(), "uv not found: install uv to run Python engines"),
            ),
            result => result,
        }
    }

    async fn stage(&self, upload: &Upload) -> Result<(PathBuf, EngineEntry)> {
        if upload.bytes.is_empty() {
            return Err(RegistryError::Invalid("empty file".into()));
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let staging = self.dir.join(format!(".staging-{nanos}"));
        fs::create_dir(&staging).await?;
        let kind = if upload.python {
            Kind::Python
        } else {
            Kind::Binary
        };
        let file = staging.join(file_name(kind));
        let handshake = async {
            fs::write(&file, &upload.bytes).await?;
            fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).await?;
            let mut engine = self.spawn(kind, &staging, HANDSHAKE).await?;
            engine.ready(HANDSHAKE).await?;
            Ok::<_, io::Error>((engine.name.clone(), engine.author.clone()))
        };
        match handshake.await {
            Ok((uci_name, author)) => {
                let entry = EngineEntry {
                    id: String::new(),
                    name: String::new(),
                    kind,
                    uci_name,
                    author,
                    options: BTreeMap::new(),
                };
                Ok((staging, entry))
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&staging).await;
                Err(RegistryError::Invalid(format!(
                    "not a working UCI engine: {e}"
                )))
            }
        }
    }

    async fn save(&self, entry: &EngineEntry) -> io::Result<()> {
        let dir = self.dir.join(&entry.id);
        let tmp = dir.join(".manifest.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(entry)?).await?;
        fs::rename(tmp, dir.join(MANIFEST)).await
    }

    fn engine_dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(RegistryError::NotFound);
        }
        Ok(self.dir.join(id))
    }
}

fn file_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Python => "engine.py",
        Kind::Binary | Kind::Stockfish => "engine",
    }
}

fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(RegistryError::Invalid(
            "name must be at most 64 printable characters".into(),
        ));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = "#!/bin/sh\nwhile read -r line; do case \"$line\" in\n  uci) echo 'id name Fake 1.0'; echo 'id author Tester'; echo uciok;;\n  isready) echo readyok;;\n  quit) exit 0;;\nesac; done\n";
    const PYTHON: &str = "import sys\nfor line in sys.stdin:\n    cmd = line.strip()\n    if cmd == 'uci': print('id name PyFake\\nuciok', flush=True)\n    elif cmd == 'isready': print('readyok', flush=True)\n    elif cmd == 'quit': break\n";

    async fn registry() -> (Registry, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("engine-arena-test-{nanos}"));
        (Registry::open(&dir, "stockfish".into()).await.unwrap(), dir)
    }

    fn upload(script: &str, python: bool, name: Option<&str>) -> Upload {
        Upload {
            name: name.map(Into::into),
            python,
            bytes: script.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn lifecycle() {
        let (registry, dir) = registry().await;
        assert_eq!(registry.list().await.unwrap()[0].kind, Kind::Stockfish);

        let entry = registry.create(upload(FAKE, false, None)).await.unwrap();
        assert_eq!(
            (
                entry.id.as_str(),
                entry.name.as_str(),
                entry.author.as_str()
            ),
            ("1", "Fake 1.0", "Tester")
        );
        let second = registry
            .create(upload(FAKE, false, Some("Mine")))
            .await
            .unwrap();
        assert_eq!((second.id.as_str(), second.name.as_str()), ("2", "Mine"));

        let options = BTreeMap::from([("Hash".to_string(), "32".to_string())]);
        let updated = registry
            .update("1", Some("Renamed".into()), Some(options.clone()))
            .await
            .unwrap();
        assert_eq!(
            (updated.name.as_str(), &updated.options),
            ("Renamed", &options)
        );
        drop(registry.start(&updated, HANDSHAKE).await.unwrap());

        let replaced = registry
            .replace("1", upload(FAKE, false, None))
            .await
            .unwrap();
        assert_eq!(
            (replaced.name.as_str(), &replaced.options),
            ("Renamed", &options)
        );

        registry.delete("1").await.unwrap();
        let ids: Vec<_> = registry
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["stockfish", "2"]);
        assert!(!std::fs::read_dir(&dir).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".staging")
        }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let (registry, dir) = registry().await;
        assert!(matches!(
            registry
                .create(upload("#!/bin/sh\nexit 1\n", false, None))
                .await,
            Err(RegistryError::Invalid(_))
        ));
        assert!(matches!(
            registry.create(upload("", false, None)).await,
            Err(RegistryError::Invalid(_))
        ));
        assert!(matches!(
            registry
                .create(upload(FAKE, false, Some("bad\nname")))
                .await,
            Err(RegistryError::Invalid(_))
        ));
        assert!(matches!(
            registry.get("../etc").await,
            Err(RegistryError::NotFound)
        ));
        assert!(matches!(
            registry.delete("stockfish").await,
            Err(RegistryError::Invalid(_))
        ));
        let options = BTreeMap::from([("Hash\nquit".to_string(), "1".to_string())]);
        assert!(matches!(
            registry.update("stockfish", None, Some(options)).await,
            Err(RegistryError::Invalid(_))
        ));
        assert_eq!(registry.list().await.unwrap().len(), 1);
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no staging leftovers"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn python_engine_via_uv() {
        let (registry, dir) = registry().await;
        match registry.create(upload(PYTHON, true, None)).await {
            Ok(entry) => assert_eq!((entry.kind, entry.name.as_str()), (Kind::Python, "PyFake")),
            Err(RegistryError::Invalid(e)) if e.contains("uv not found") => {
                eprintln!("uv not on PATH, skipping")
            }
            Err(e) => panic!("{e}"),
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
