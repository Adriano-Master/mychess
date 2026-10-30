//! Access token and its storage (spec m8-3, D1307).
//!
//! The token never appears in `Debug`, `Display`, errors, logs or URLs: the
//! only way out is [`Token::expose`], used by the HTTP client to build the
//! `Authorization` header.

use std::{
    fmt,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::error::{Error, Result};

/// Longest token accepted (Lichess tokens may grow; the docs say at least 512).
const MAX_TOKEN_LEN: usize = 1024;

/// A Lichess access token. `Debug` is redacted; there is no `Display`.
pub struct Token(String);

impl Token {
    /// Validates the shape (`^[A-Za-z0-9_]+$`, at most 1024 characters) after
    /// trimming surrounding whitespace.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when the text is not a plausible token. The
    /// error never contains the text.
    pub fn new(text: &str) -> Result<Self> {
        let t = text.trim();
        let ok = !t.is_empty()
            && t.len() <= MAX_TOKEN_LEN
            && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if ok {
            Ok(Self(t.to_owned()))
        } else {
            Err(Error::InvalidArgument { what: "token" })
        }
    }

    /// The secret text. Use only to build the `Authorization` header.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// An explicit copy (there is deliberately no `Clone`).
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Drop for Token {
    /// Overwrites the secret before the memory goes back to the allocator
    /// (D3006). Best effort: copies made by `String` growth or by the HTTP
    /// stack are out of reach.
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// Where a token lives between sessions. Synchronous: the app runs these in
/// `spawn_blocking` (keychains block).
pub trait TokenStore: Send + Sync + fmt::Debug {
    /// Loads the token saved for `login`, if any.
    fn load(&self, login: &str) -> Result<Option<Token>>;
    /// Saves (replacing) the token for `login`.
    fn save(&self, login: &str, token: &Token) -> Result<()>;
    /// Removes the token for `login`; absent is not an error.
    fn delete(&self, login: &str) -> Result<()>;
}

/// Session-only storage (tests, or when the user refuses persistence).
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Option<(String, Token)>>,
}

impl MemoryStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MemoryStore { .. }")
    }
}

impl TokenStore for MemoryStore {
    fn load(&self, login: &str) -> Result<Option<Token>> {
        let guard = self.inner.lock().map_err(|_| poisoned())?;
        Ok(guard
            .as_ref()
            .filter(|(l, _)| l == login)
            .map(|(_, t)| t.duplicate()))
    }

    fn save(&self, login: &str, token: &Token) -> Result<()> {
        *self.inner.lock().map_err(|_| poisoned())? = Some((login.to_owned(), token.duplicate()));
        Ok(())
    }

    fn delete(&self, login: &str) -> Result<()> {
        let mut guard = self.inner.lock().map_err(|_| poisoned())?;
        if guard.as_ref().is_some_and(|(l, _)| l == login) {
            *guard = None;
        }
        Ok(())
    }
}

fn poisoned() -> Error {
    Error::TokenStore("store lock poisoned".to_owned())
}

/// Token in a file `lichess-token` (two lines: login, token), created through a
/// temporary file in the same directory plus `rename`.
///
/// Only used with the user's explicit consent (D1307), when no keychain is
/// available. On Unix the directory is created with mode 0700 and the file is
/// 0600; `load` refuses a symlink, a non-regular file, a file whose mode is
/// not 0600 and anything larger than a token can be. On Windows the file has
/// no ACL of its own: the protection is that of the user profile directory
/// (`data_local_dir`, never the roaming profile), so the keychain is the
/// recommended store there.
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// File name inside the data directory.
    pub const FILE_NAME: &'static str = "lichess-token";

    /// Store at an explicit file path.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Store in the local (non-roaming) data directory of the application,
    /// using the same `ProjectDirs` as the preferences, or `None` when the
    /// platform has no home directory.
    #[must_use]
    pub fn in_data_dir() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("org", "mychess", "MyChess")?;
        Some(Self::at(dirs.data_local_dir().join(Self::FILE_NAME)))
    }

    /// The file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn io_err(what: &str, e: &std::io::Error) -> Error {
    Error::TokenStore(format!("{what}: {:?}", e.kind()))
}

/// Largest file `load` reads: login line, token and slack.
const MAX_FILE_LEN: u64 = MAX_TOKEN_LEN as u64 + 256;

/// Creates `dir` (and missing parents) with mode 0700 on Unix.
fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|e| io_err("create data dir", &e))
}

impl FileStore {
    /// Reads the file within the size limit. `None` when it does not exist.
    fn read_checked(&self) -> Result<Option<String>> {
        use std::io::Read;
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(Error::TokenStore("token file is a symlink".to_owned()));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err("stat token file", &e)),
        }
        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err("read token file", &e)),
        };
        let meta = file.metadata().map_err(|e| io_err("stat token file", &e))?;
        if !meta.is_file() {
            return Err(Error::TokenStore(
                "token file is not a regular file".to_owned(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o777 != 0o600 {
                return Err(Error::TokenStore(
                    "token file permissions are not 0600; delete it and sign in again".to_owned(),
                ));
            }
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_LEN + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_err("read token file", &e))?;
        if bytes.len() as u64 > MAX_FILE_LEN {
            return Err(Error::TokenStore("token file is too large".to_owned()));
        }
        // Not valid UTF-8 counts as a corrupt file, i.e. no token.
        Ok(Some(String::from_utf8(bytes).unwrap_or_default()))
    }
}

impl TokenStore for FileStore {
    fn load(&self, login: &str) -> Result<Option<Token>> {
        let Some(text) = self.read_checked()? else {
            return Ok(None);
        };
        let mut lines = text.lines();
        let (Some(saved_login), Some(token)) = (lines.next(), lines.next()) else {
            return Ok(None);
        };
        if saved_login != login {
            return Ok(None);
        }
        // A corrupt file counts as "no token", never as an error carrying content.
        Ok(Token::new(token).ok())
    }

    fn save(&self, login: &str, token: &Token) -> Result<()> {
        if login.is_empty() || login.contains(['\n', '\r']) {
            return Err(Error::InvalidArgument { what: "login" });
        }
        let dir = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        create_private_dir(dir)?;
        // `NamedTempFile` is created with mode 0600 on Unix.
        let mut tmp =
            tempfile::NamedTempFile::new_in(dir).map_err(|e| io_err("create temp file", &e))?;
        tmp.write_all(format!("{login}\n{}\n", token.expose()).as_bytes())
            .map_err(|e| io_err("write token file", &e))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| io_err("sync token file", &e))?;
        tmp.persist(&self.path)
            .map_err(|e| io_err("replace token file", &e.error))?;
        #[cfg(unix)]
        {
            // Make the rename durable.
            std::fs::File::open(dir)
                .and_then(|d| d.sync_all())
                .map_err(|e| io_err("sync data dir", &e))?;
        }
        Ok(())
    }

    fn delete(&self, login: &str) -> Result<()> {
        // Keep a file that clearly belongs to another login; remove ours and
        // any unreadable leftovers.
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => String::new(),
        };
        let mut lines = text.lines();
        if let (Some(saved), Some(_)) = (lines.next(), lines.next())
            && saved != login
        {
            return Ok(());
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err("remove token file", &e)),
        }
    }
}

/// Token in the system keychain: Keychain (macOS), Credential Manager
/// (Windows), Secret Service (Linux). Service `mychess`, user
/// `lichess:<login>`.
#[cfg(feature = "keyring-store")]
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

#[cfg(feature = "keyring-store")]
impl KeyringStore {
    const SERVICE: &'static str = "mychess";

    /// New handle (the keychain is touched only by the trait methods).
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn entry(login: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(Self::SERVICE, &format!("lichess:{login}")).map_err(|e| keyring_err(&e))
    }
}

/// Coarse error text: the keyring errors can carry secret bytes, so only the
/// variant is reported.
#[cfg(feature = "keyring-store")]
fn keyring_err(e: &keyring::Error) -> Error {
    use keyring::Error as K;
    let kind = match e {
        K::NoDefaultStore => "no keychain available",
        K::NoStorageAccess(_) => "keychain locked or access denied",
        K::PlatformFailure(_) => "keychain failure",
        K::NoEntry => "no entry",
        K::BadEncoding(_) | K::BadDataFormat(..) | K::BadStoreFormat(_) => "bad stored data",
        K::TooLong(..) | K::Invalid(..) => "invalid entry",
        K::Ambiguous(_) => "ambiguous entry",
        _ => "keychain error",
    };
    Error::TokenStore(kind.to_owned())
}

#[cfg(feature = "keyring-store")]
impl TokenStore for KeyringStore {
    fn load(&self, login: &str) -> Result<Option<Token>> {
        match Self::entry(login)?.get_password() {
            Ok(text) => Ok(Token::new(&text).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(&e)),
        }
    }

    fn save(&self, login: &str, token: &Token) -> Result<()> {
        Self::entry(login)?
            .set_password(token.expose())
            .map_err(|e| keyring_err(&e))
    }

    fn delete(&self, login: &str) -> Result<()> {
        match Self::entry(login)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(&e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(s: &str) -> Token {
        Token::new(s).expect("valid token")
    }

    #[test]
    fn token_validation() {
        assert!(Token::new("lip_AbC123xyz").is_ok());
        assert!(Token::new("  spaced_ok \n").is_ok());
        assert!(Token::new("").is_err());
        assert!(Token::new("has space").is_err());
        assert!(Token::new("semi;colon").is_err());
        assert!(Token::new(&"a".repeat(1025)).is_err());
        assert!(Token::new(&"a".repeat(600)).is_ok());
    }

    #[test]
    fn debug_and_errors_never_show_the_secret() {
        let t = tok("lip_SECRETVALUE");
        assert!(!format!("{t:?}").contains("SECRET"));
        let e = Token::new("bad token SECRETVALUE").err();
        assert!(!format!("{e:?}").contains("SECRET"));
        assert!(!format!("{:?}", MemoryStore::new()).contains("SECRET"));
    }

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::new();
        assert!(s.load("bob").expect("load").is_none());
        s.save("bob", &tok("lip_one")).expect("save");
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_one")));
        assert!(s.load("alice").expect("load").is_none());
        s.delete("alice").expect("delete other");
        assert!(s.load("bob").expect("load").is_some());
        s.delete("bob").expect("delete");
        assert!(s.load("bob").expect("load").is_none());
    }

    #[test]
    fn file_store_round_trip_and_replace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = FileStore::at(dir.path().join("nested").join(FileStore::FILE_NAME));
        assert!(s.load("bob").expect("load").is_none());
        s.save("bob", &tok("lip_one")).expect("save");
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_one")));
        s.save("bob", &tok("lip_two")).expect("replace");
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_two")));
        assert!(s.load("alice").expect("load").is_none());
        s.delete("alice").expect("other login is a no-op");
        assert!(s.path().exists());
        s.delete("bob").expect("delete");
        assert!(!s.path().exists());
        s.delete("bob").expect("delete twice");
        // No temporary file is left behind.
        let leftovers = std::fs::read_dir(s.path().parent().expect("parent"))
            .expect("dir")
            .count();
        assert_eq!(leftovers, 0);
    }

    #[cfg(unix)]
    #[test]
    fn file_store_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let s = FileStore::at(dir.path().join("t"));
        s.save("bob", &tok("lip_one")).expect("save");
        let mode = std::fs::metadata(s.path())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn file_store_dir_is_0700_and_load_refuses_unsafe_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().expect("tempdir");
        let s = FileStore::at(dir.path().join("a").join("b").join("t"));
        s.save("bob", &tok("lip_one")).expect("save");
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(s.path().parent().expect("parent")), 0o700);
        // Wrong mode: refused.
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(matches!(s.load("bob"), Err(Error::TokenStore(_))));
        // Saving again replaces it with a 0600 file.
        s.save("bob", &tok("lip_two")).expect("save");
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_two")));
        // Symlink: refused, and the target is untouched.
        let link = dir.path().join("link");
        symlink(s.path(), &link).expect("symlink");
        assert!(matches!(
            FileStore::at(&link).load("bob"),
            Err(Error::TokenStore(_))
        ));
    }

    #[test]
    fn oversized_token_file_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t");
        std::fs::write(&path, format!("bob\n{}\n", "a".repeat(100_000))).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
        assert!(matches!(
            FileStore::at(&path).load("bob"),
            Err(Error::TokenStore(_))
        ));
    }

    #[test]
    fn in_data_dir_uses_the_app_project_dirs() {
        if let Some(s) = FileStore::in_data_dir() {
            assert!(s.path().ends_with(FileStore::FILE_NAME));
        }
    }

    /// Writes a file the way `save` would leave it (0600 on Unix).
    fn write_private(path: &Path, text: &str) {
        std::fs::write(path, text).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
    }

    #[test]
    fn corrupt_or_foreign_file_is_no_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t");
        write_private(&path, "bob\nnot a token!\n");
        let s = FileStore::at(&path);
        assert!(s.load("bob").expect("load").is_none());
        write_private(&path, "only-one-line");
        assert!(s.load("only-one-line").expect("load").is_none());
        assert!(matches!(
            s.save("bad\nlogin", &tok("lip_x")),
            Err(Error::InvalidArgument { .. })
        ));
    }

    /// Needs a running keychain (Secret Service, Keychain, Credential
    /// Manager); run manually with `--ignored`.
    #[cfg(feature = "keyring-store")]
    #[test]
    #[ignore = "needs a system keychain"]
    fn keyring_store_round_trip() {
        let s = KeyringStore::new();
        let login = "mychess-test-user";
        s.save(login, &tok("lip_keyring")).expect("save");
        assert_eq!(s.load(login).expect("load"), Some(tok("lip_keyring")));
        s.delete(login).expect("delete");
        assert!(s.load(login).expect("load").is_none());
    }
}
