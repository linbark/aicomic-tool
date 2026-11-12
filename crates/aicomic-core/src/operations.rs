//! Durable isolation manifests. Main-database receipts are the commit oracle.
//! No destructor removes data: interrupted or failed recovery remains inspectable.
use crate::*;
use ring::hmac;
use serde::Deserialize;
use std::path::PathBuf;

const DIRECTORY: &str = ".aicomic-operations";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    operation_id: String,
    database_id: String,
    project_id: Option<i64>,
    moves: Vec<String>,
    created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_tag: Option<Vec<u8>>,
}

impl Manifest {
    fn authenticated_bytes(&self) -> Result<Vec<u8>> {
        let mut intent = self.clone();
        intent.auth_tag = None;
        let mut bytes = b"aicomic-isolation-manifest-v2\0".to_vec();
        serde_json::to_writer(&mut bytes, &intent)?;
        Ok(bytes)
    }
}

pub(crate) struct Operation {
    paths: AppPaths,
    manifest: Manifest,
}

fn required(id: &str, reason: impl ToString) -> Error {
    Error::RecoveryRequired {
        operation_id: id.into(),
        reason: reason.to_string(),
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn database_id(conn: &Connection) -> Result<String> {
    Ok(conn.query_row(
        "SELECT database_id FROM aicomic_storage_identity WHERE singleton=1",
        [],
        |r| r.get(0),
    )?)
}
// The key never leaves SQLite as a field, diagnostic, Debug value or manifest.
// Only a short-lived ring key is constructed for signing/verification.
fn authentication_key(conn: &Connection) -> Result<hmac::Key> {
    let bytes: Vec<u8> = conn.query_row(
        "SELECT journal_auth_key FROM aicomic_storage_identity WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    if bytes.len() != 32 {
        return Err(Error::Invalid(
            "journal authentication key is unavailable".into(),
        ));
    }
    Ok(hmac::Key::new(hmac::HMAC_SHA256, &bytes))
}
fn media_references(conn: &Connection, relative: &str) -> Result<i64> {
    Ok(conn.query_row("SELECT (SELECT count(*) FROM assets WHERE replace(file_path,'\\','/')=?)+(SELECT count(*) FROM shots WHERE replace(video_path,'\\','/')=?)", params![relative, relative], |r| r.get(0))?)
}
fn directory(paths: &AppPaths) -> Result<PathBuf> {
    paths.resolve(DIRECTORY)
}

pub(crate) fn has_pending(paths: &AppPaths) -> Result<bool> {
    let directory = directory(paths)?;
    if !directory.try_exists()? {
        return Ok(false);
    }
    Ok(std::fs::read_dir(directory)?.next().is_some())
}

impl Operation {
    pub(crate) fn prepare(
        paths: &AppPaths,
        tx: &Connection,
        originals: Vec<String>,
        project_id: Option<i64>,
        created: Option<String>,
    ) -> Result<Self> {
        let mut moves = Vec::new();
        for original in originals {
            let original = original.replace('\\', "/");
            let path = Self::original(paths, &original, project_id)?;
            if path.try_exists()? && !moves.contains(&original) {
                moves.push(original);
            }
        }
        if let Some(created) = &created {
            let destination = Self::destination(paths, created)?;
            if destination.try_exists()? {
                return Err(Error::Invalid(
                    "new media destination already exists".into(),
                ));
            }
        }
        let mut operation = Self {
            paths: paths.clone(),
            manifest: Manifest {
                version: 2,
                operation_id: uuid::Uuid::new_v4().simple().to_string(),
                database_id: database_id(tx)?,
                project_id,
                moves,
                created,
                auth_tag: None,
            },
        };
        operation.manifest.auth_tag = Some(
            hmac::sign(
                &authentication_key(tx)?,
                &operation.manifest.authenticated_bytes()?,
            )
            .as_ref()
            .to_vec(),
        );
        std::fs::create_dir_all(directory(paths)?)?;
        std::fs::create_dir(operation.dir()?)?;
        let published = (|| -> Result<()> {
            #[cfg(test)]
            if crate::operation_tests::error_at("intent_publish") {
                return Err(std::io::Error::other("injected intent publication failure").into());
            }
            paths::write_json(
                &operation.manifest_path()?,
                &serde_json::to_value(&operation.manifest)?,
            )?;
            #[cfg(test)]
            if crate::operation_tests::error_at("intent_sync") {
                return Err(
                    std::io::Error::other("injected intent synchronization failure").into(),
                );
            }
            paths::sync_directory(&operation.dir()?)?;
            paths::sync_directory(&directory(paths)?)?;
            tx.execute(
                "INSERT INTO aicomic_operation_receipts(operation_id,manifest_json) VALUES(?,?)",
                params![operation.id(), serde_json::to_string(&operation.manifest)?],
            )?;
            Ok(())
        })();
        if let Err(error) = published {
            // No payload has been created or renamed yet. Only our freshly
            // created metadata directory can be removed on ordinary errors.
            return Err(match operation.remove_manifest() {
                Ok(()) => error,
                Err(cleanup) => required(
                    operation.id(),
                    format!("{error}; intent cleanup: {cleanup}"),
                ),
            });
        }
        #[cfg(test)]
        crate::operation_tests::hit("intent_durable");
        Ok(operation)
    }
    fn load(paths: &AppPaths, conn: &Connection, id: &str, bytes: &[u8]) -> Result<(Self, bool)> {
        let manifest: Manifest = serde_json::from_slice(bytes)?;
        if !valid_id(id)
            || manifest.operation_id != id
            || manifest.database_id != database_id(conn)?
        {
            return Err(required(id, "manifest identity or database does not match"));
        }
        let receipt: Option<String> = conn
            .query_row(
                "SELECT manifest_json FROM aicomic_operation_receipts WHERE operation_id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let canonical = serde_json::to_string(&manifest)?;
        match manifest.version {
            2 => {
                let tag = manifest
                    .auth_tag
                    .as_deref()
                    .ok_or_else(|| required(id, "manifest authentication tag is missing"))?;
                let key = authentication_key(conn)
                    .map_err(|_| required(id, "journal authentication key is unavailable"))?;
                hmac::verify(&key, &manifest.authenticated_bytes()?, tag)
                    .map_err(|_| required(id, "manifest authentication failed"))?;
            }
            // Previously emitted v1 intents were not authenticated. Only the
            // exact committed receipt in trusted main can establish provenance.
            1 if manifest.auth_tag.is_none() && receipt.as_deref() == Some(canonical.as_str()) => {}
            1 => {
                return Err(required(
                    id,
                    "unsigned manifest has no exact trusted committed receipt",
                ));
            }
            _ => return Err(required(id, "unsupported manifest version")),
        }
        if receipt
            .as_ref()
            .is_some_and(|receipt| *receipt != canonical)
        {
            return Err(required(id, "receipt and manifest differ"));
        }
        // Authenticate before interpreting any manifest-controlled path or
        // performing any filesystem or business-data recovery.
        let operation = Self {
            paths: paths.clone(),
            manifest,
        };
        operation.validate()?;
        Ok((operation, receipt.is_some()))
    }
    pub(crate) fn id(&self) -> &str {
        &self.manifest.operation_id
    }
    fn dir(&self) -> Result<PathBuf> {
        paths::safe_join(&directory(&self.paths)?, self.id())
    }
    fn manifest_path(&self) -> Result<PathBuf> {
        paths::safe_join(&self.dir()?, "manifest.json")
    }
    fn staged(&self, index: usize) -> Result<PathBuf> {
        paths::safe_join(&self.dir()?, format!("removed-{index}"))
    }
    fn original(paths: &AppPaths, original: &str, project_id: Option<i64>) -> Result<PathBuf> {
        let path = paths.resolve(original)?;
        let media = original
            .strip_prefix("data/")
            .is_some_and(|p| !p.is_empty());
        let project = project_id.is_some_and(|id| original == format!("projects/{id}"));
        if !media && !project {
            return Err(Error::UnsafePath(original.into()));
        }
        Self::check_type(&path, project)?;
        Ok(path)
    }
    fn check_type(path: &Path, directory: bool) -> Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(meta)
                if if directory {
                    meta.file_type().is_dir()
                } else {
                    meta.file_type().is_file()
                } =>
            {
                Ok(())
            }
            Ok(_) => Err(Error::UnsafePath(path.display().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    fn validate(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for (index, original) in self.manifest.moves.iter().enumerate() {
            if original.contains('\\') {
                return Err(Error::UnsafePath(original.clone()));
            }
            let path = Self::original(&self.paths, original, self.manifest.project_id)?;
            if !seen.insert(path) {
                return Err(required(self.id(), "duplicate original path"));
            }
            Self::check_type(&self.staged(index)?, original.starts_with("projects/"))?;
        }
        if let Some(created) = &self.manifest.created {
            let destination = Self::destination(&self.paths, created)?;
            if seen.contains(&destination) {
                return Err(required(self.id(), "new destination is also an original"));
            }
            Self::check_type(&self.copied_path()?, false)?;
        }
        // Unknown entries are not ours to purge. Keep the manifest with them.
        for entry in std::fs::read_dir(self.dir()?)? {
            let name = entry?.file_name();
            let known = name == "manifest.json"
                || (self.manifest.created.is_some() && name == "new-0")
                || (0..self.manifest.moves.len())
                    .any(|index| name == format!("removed-{index}").as_str());
            if !known {
                return Err(required(
                    self.id(),
                    "unexpected file in operation directory",
                ));
            }
        }
        Ok(())
    }
    fn destination(paths: &AppPaths, relative: &str) -> Result<PathBuf> {
        if !relative.starts_with("data/") || relative.contains('\\') {
            return Err(Error::UnsafePath(relative.into()));
        }
        Self::original(paths, relative, None)
    }
    pub(crate) fn copied_path(&self) -> Result<PathBuf> {
        paths::safe_join(&self.dir()?, "new-0")
    }
    pub(crate) fn copy_from(&self, source: &Path) -> Result<()> {
        use std::io::{Read, Write};
        let mut source = std::fs::File::open(source)?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.copied_path()?)?;
        let mut buffer = [0; 65536];
        let size = source.read(&mut buffer)?;
        target.write_all(&buffer[..size])?;
        #[cfg(test)]
        crate::operation_tests::hit("new_copy_partial");
        #[cfg(test)]
        if crate::operation_tests::error_at("copy") {
            return Err(std::io::Error::other("injected partial copy failure").into());
        }
        std::io::copy(&mut source, &mut target)?;
        target.sync_all()?;
        drop(target);
        paths::sync_directory(&self.dir()?)?;
        Ok(())
    }
    pub(crate) fn stage(&self) -> Result<()> {
        for (index, original) in self.manifest.moves.iter().enumerate() {
            let from = Self::original(&self.paths, original, self.manifest.project_id)?;
            let to = self.staged(index)?;
            #[cfg(test)]
            if index == 1 && crate::operation_tests::error_at("second_rename") {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected second rename failure",
                )
                .into());
            }
            std::fs::rename(&from, &to)?;
            paths::sync_directory(from.parent().unwrap())?;
            paths::sync_directory(&self.dir()?)?;
        }
        #[cfg(test)]
        crate::operation_tests::hit("quarantined");
        Ok(())
    }
    pub(crate) fn abort(&self, cause: Error) -> Error {
        match self.rollback() {
            Ok(()) => cause,
            Err(error) => required(self.id(), format!("{cause}; rollback: {error}")),
        }
    }
    pub(crate) fn commit_main(&self, tx: rusqlite::Transaction<'_>) -> Result<()> {
        if let Err(error) = tx.commit() {
            // A failed COMMIT is not proof of rollback. Only consult a fresh
            // connection after rusqlite has dropped/rolled back the transaction.
            let receipt = Connection::open_with_flags(
                &self.paths.database_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .and_then(|conn| {
                conn.query_row(
                    "SELECT count(*) FROM aicomic_operation_receipts WHERE operation_id=?",
                    [self.id()],
                    |r| r.get::<_, i64>(0),
                )
            });
            return Err(match receipt {
                Ok(0) => self.abort(error.into()),
                Ok(_) => self.pending(error),
                Err(inspection) => required(
                    self.id(),
                    format!("commit: {error}; receipt unavailable: {inspection}"),
                ),
            });
        }
        #[cfg(test)]
        crate::operation_tests::hit("main_committed");
        Ok(())
    }
    pub(crate) fn pending(&self, error: impl ToString) -> Error {
        Error::CommittedPending {
            operation_id: self.id().into(),
            reason: error.to_string(),
        }
    }
    fn validate_rollback_owners(&self, conn: &Connection) -> Result<()> {
        if let Some(project_id) = self.manifest.project_id {
            require(conn, "projects", project_id).map_err(|error| required(self.id(), error))?;
        }
        for original in &self.manifest.moves {
            if let Some(relative) = original.strip_prefix("data/")
                && media_references(conn, relative)? == 0
            {
                return Err(required(
                    self.id(),
                    "rollback original has no owner in the rolled-back main database",
                ));
            }
        }
        Ok(())
    }
    fn rollback(&self) -> Result<()> {
        self.validate()?;
        for (index, original) in self.manifest.moves.iter().enumerate().rev() {
            let original = Self::original(&self.paths, original, self.manifest.project_id)?;
            let staged = self.staged(index)?;
            match (original.try_exists()?, staged.try_exists()?) {
                (false, true) => {
                    std::fs::rename(&staged, &original)?;
                    paths::sync_directory(original.parent().unwrap())?;
                    paths::sync_directory(&self.dir()?)?;
                }
                (true, false) => {}
                (true, true) => {
                    return Err(required(
                        self.id(),
                        "original and quarantined paths both exist",
                    ));
                }
                (false, false) => {
                    return Err(required(
                        self.id(),
                        "original and quarantined paths are both missing",
                    ));
                }
            }
            #[cfg(test)]
            crate::operation_tests::hit("recovery_step");
        }
        // Only the operation's private copy is disposable before commit. Never
        // remove a destination in data based on an uncommitted disk manifest.
        if self.manifest.created.is_some() {
            self.remove_private_copy()?;
        }
        self.remove_manifest()
    }
    fn remove_private_copy(&self) -> Result<()> {
        match std::fs::remove_file(self.copied_path()?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub(crate) fn complete(&self, conn: &mut Connection) -> Result<()> {
        // Memory must already be committed/dropped before reacquiring main.
        // Serialize late promotion against other Store instances consuming it.
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| self.pending(error))?;
        if !self
            .dir()
            .map_err(|error| self.pending(error))?
            .try_exists()
            .map_err(|error| self.pending(error))?
        {
            return Ok(());
        }
        self.validate().map_err(|error| self.pending(error))?;
        self.finish_files(&tx)
            .map_err(|error| self.pending(error))?;
        tx.commit().map_err(|error| self.pending(error))
    }
    fn finish_files(&self, conn: &Connection) -> Result<()> {
        if let Some(destination) = &self.manifest.created {
            let relative = destination
                .strip_prefix("data/")
                .ok_or_else(|| Error::UnsafePath(destination.clone()))?;
            let references = media_references(conn, relative)?;
            if references == 0 {
                self.remove_private_copy()?;
            } else {
                let destination = Self::destination(&self.paths, destination)?;
                let staged = self.copied_path()?;
                match (staged.try_exists()?, destination.try_exists()?) {
                    (true, false) => {
                        std::fs::create_dir_all(destination.parent().unwrap())?;
                        Self::destination(&self.paths, self.manifest.created.as_ref().unwrap())?;
                        std::fs::rename(&staged, &destination)?;
                        paths::sync_directory(destination.parent().unwrap())?;
                        paths::sync_directory(&self.dir()?)?;
                    }
                    (false, true) => {}
                    (true, true) => {
                        return Err(required(
                            self.id(),
                            "new media exists in both staging and destination",
                        ));
                    }
                    (false, false) => {
                        return Err(required(self.id(), "committed new media is missing"));
                    }
                }
            }
            #[cfg(test)]
            crate::operation_tests::hit("recovery_step");
        }
        #[cfg(test)]
        if crate::operation_tests::error_at("purge") {
            return Err(std::io::Error::other("injected purge failure").into());
        }
        for index in 0..self.manifest.moves.len() {
            let staged = self.staged(index)?;
            match std::fs::symlink_metadata(&staged) {
                Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&staged)?,
                Ok(_) => std::fs::remove_file(&staged)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            #[cfg(test)]
            crate::operation_tests::hit("recovery_step");
        }
        self.remove_manifest()
    }
    fn remove_manifest(&self) -> Result<()> {
        match std::fs::remove_file(self.manifest_path()?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        match std::fs::remove_dir(self.dir()?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        paths::sync_directory(&directory(&self.paths)?)
    }
}

/// Called after acquiring main IMMEDIATE and before new business writes.
/// An unresolved rollback must not lose the SQL facts it is restoring files for.
pub(crate) fn ensure_ready(conn: &Connection, paths: &AppPaths) -> Result<()> {
    let directory = directory(paths)?;
    if !directory.try_exists()? {
        return Ok(());
    }
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| required("unknown", "non-UTF8 operation directory"))?;
        let result = (|| -> Result<()> {
            if !valid_id(&id) || !entry.file_type()?.is_dir() {
                return Err(required(&id, "invalid operation directory"));
            }
            let dir = paths::safe_join(&directory, &id)?;
            let bytes = std::fs::read(paths::safe_join(&dir, "manifest.json")?)?;
            let (_, committed) = Operation::load(paths, conn, &id, &bytes)?;
            if !committed {
                return Err(required(
                    &id,
                    "uncommitted operation must be recovered before another write",
                ));
            }
            Ok(())
        })();
        result.map_err(|error| required(&id, error))?;
    }
    Ok(())
}

pub(crate) fn warnings(paths: &AppPaths) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for entry in std::fs::read_dir(&paths.root)? {
        let name = entry?.file_name();
        if name.to_string_lossy().starts_with(".aicomic-trash-") {
            warnings.push(format!(
                "legacy quarantine {name:?} has no manifest; data preserved for manual recovery"
            ));
        }
    }
    warnings.sort();
    Ok(warnings)
}

pub(crate) fn recover(conn: &mut Connection, paths: &AppPaths) -> Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for warning in warnings(paths)? {
        eprintln!("storage warning: {warning}");
    }
    let directory = directory(paths)?;
    let mut rollbacks = Vec::new();
    let mut committed_operations = Vec::new();
    let mut empty_directories = Vec::new();
    if directory.try_exists()? {
        let entries = std::fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        #[cfg(test)]
        let entries = {
            let mut entries = entries;
            if crate::operation_tests::error_at("committed_first") {
                entries.sort_by_key(|entry| {
                    tx.query_row(
                        "SELECT count(*) FROM aicomic_operation_receipts WHERE operation_id=?",
                        [entry.file_name().to_string_lossy().as_ref()],
                        |r| r.get::<_, i64>(0),
                    )
                    .unwrap()
                        == 0
                });
            }
            entries
        };
        for entry in entries {
            let id = entry
                .file_name()
                .into_string()
                .map_err(|_| required("unknown", "non-UTF8 operation directory"))?;
            let result = (|| -> Result<()> {
                if !valid_id(&id) || !entry.file_type()?.is_dir() {
                    return Err(required(&id, "invalid operation directory"));
                }
                let dir = paths::safe_join(&directory, &id)?;
                let bytes = match std::fs::read(paths::safe_join(&dir, "manifest.json")?) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // Killed between removing the completed manifest and its
                        // empty directory, or before publishing the first intent.
                        // Never remove a manifestless directory containing data.
                        match std::fs::read_dir(&dir) {
                            Ok(mut entries) => {
                                if entries.next().is_none() {
                                    empty_directories.push(dir);
                                    return Ok(());
                                }
                                return Err(required(
                                    &id,
                                    "manifest missing but operation directory contains data",
                                ));
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                            Err(e) => return Err(e.into()),
                        }
                    }
                    Err(e) => return Err(e.into()),
                };
                let (operation, committed) = Operation::load(paths, &tx, &id, &bytes)?;
                if committed {
                    committed_operations.push(operation);
                } else {
                    operation.validate_rollback_owners(&tx)?;
                    rollbacks.push(operation);
                }
                Ok(())
            })();
            match result {
                Err(error @ Error::CommittedPending { .. }) => return Err(error),
                Err(error) => return Err(required(&id, error)),
                Ok(()) => {}
            }
        }
    }
    // A later uncommitted deletion can currently quarantine a promoted file
    // owned by an older committed operation. Restore all such consumers first,
    // regardless of filesystem enumeration order, before checking promotions.
    // Every manifest was validated above before any recovery side effect.
    for operation in rollbacks {
        operation
            .rollback()
            .map_err(|error| required(operation.id(), error))?;
    }
    for operation in committed_operations {
        if let Some(project_id) = operation.manifest.project_id
            && let Some(memory) = memory_cleanup::Prepared::existing(
                &tx,
                &paths.resolve("memory_store.db")?,
                project_id,
            )
            .map_err(|error| operation.pending(error))?
        {
            memory.commit().map_err(|error| operation.pending(error))?;
        }
        operation
            .finish_files(&tx)
            .map_err(|error| operation.pending(error))?;
    }
    for dir in empty_directories {
        std::fs::remove_dir(&dir)?;
        paths::sync_directory(&directory)?;
    }
    tx.commit()?;
    Ok(())
}
