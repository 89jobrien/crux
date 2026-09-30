use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Write as _},
    path::{Path, PathBuf},
};

use crux_improve::Crux;
use fs2::FileExt as _;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    BaselineRef, RegressionCaseId, RegressionError, RegressionReport, TraceDigest, digest_trace,
};

use super::RegressionStore;
use crate::artifact::TraceEnvelope;

const OBJECTS_DIR: &str = "objects";
const BASELINES_DIR: &str = "baselines";
const REPORTS_DIR: &str = "reports";
const LOCKS_DIR: &str = "locks";

/// Atomic filesystem-backed regression store.
#[derive(Debug)]
pub struct FileRegressionStore {
    root: PathBuf,
}

impl FileRegressionStore {
    /// Opens or creates a private filesystem regression store.
    ///
    /// # Errors
    ///
    /// Returns an error for inaccessible paths, symlinked managed directories, or invalid
    /// filesystem entries.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RegressionError> {
        let root = root.as_ref().to_path_buf();
        reject_symlink(&root)?;
        ensure_private_directory(&root)?;
        let root = root.canonicalize()?;
        for directory in [OBJECTS_DIR, BASELINES_DIR, REPORTS_DIR, LOCKS_DIR] {
            ensure_private_directory(&root.join(directory))?;
        }
        Ok(Self { root })
    }

    fn object_path(&self, digest: &TraceDigest) -> PathBuf {
        self.root
            .join(OBJECTS_DIR)
            .join(format!("{}.json", digest.hex()))
    }

    fn baseline_path(&self, case: &RegressionCaseId) -> PathBuf {
        self.root.join(BASELINES_DIR).join(format!("{case}.json"))
    }

    fn report_directory(&self, case: &RegressionCaseId) -> PathBuf {
        self.root.join(REPORTS_DIR).join(case.as_str())
    }

    fn lock_path(&self, case: &RegressionCaseId) -> PathBuf {
        self.root.join(LOCKS_DIR).join(format!("{case}.lock"))
    }

    fn read_baseline_optional(
        &self,
        case: &RegressionCaseId,
    ) -> Result<Option<BaselineRef>, RegressionError> {
        let path = self.baseline_path(case);
        if !path.try_exists()? {
            return Ok(None);
        }
        let baseline: BaselineRef = read_json(&path)?;
        if &baseline.case != case {
            return Err(RegressionError::BaselineCaseMismatch {
                expected: case.clone(),
                actual: baseline.case,
            });
        }
        self.trace(&baseline.trace)?;
        Ok(Some(baseline))
    }
}

impl RegressionStore for FileRegressionStore {
    fn put_trace(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError> {
        let digest = digest_trace(trace)?;
        let path = self.object_path(&digest);
        reject_symlink(&path)?;
        if path.try_exists()? {
            self.trace(&digest)?;
            return Ok(digest);
        }
        let envelope = TraceEnvelope::new(trace.clone());
        match write_json_atomic(&path, &envelope, false) {
            Ok(()) => Ok(digest),
            Err(RegressionError::Io(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                self.trace(&digest)?;
                Ok(digest)
            }
            Err(error) => Err(error),
        }
    }

    fn trace(&self, digest: &TraceDigest) -> Result<Crux<Value>, RegressionError> {
        let path = self.object_path(digest);
        reject_symlink(&path)?;
        if !path.try_exists()? {
            return Err(RegressionError::ArtifactNotFound(digest.clone()));
        }
        let envelope: TraceEnvelope = read_json(&path)?;
        envelope.validate()?;
        let actual = digest_trace(&envelope.trace)?;
        if &actual != digest {
            return Err(RegressionError::DigestMismatch {
                expected: digest.clone(),
                actual,
            });
        }
        Ok(envelope.trace)
    }

    fn compare_and_set_baseline(
        &self,
        case: &RegressionCaseId,
        expected: Option<&TraceDigest>,
        next: &BaselineRef,
    ) -> Result<(), RegressionError> {
        if &next.case != case {
            return Err(RegressionError::BaselineCaseMismatch {
                expected: case.clone(),
                actual: next.case.clone(),
            });
        }
        self.trace(&next.trace)?;
        let lock_path = self.lock_path(case);
        reject_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;
        lock.lock_exclusive()?;
        let result = (|| {
            let actual = self.read_baseline_optional(case)?;
            let actual_digest = actual.as_ref().map(|baseline| &baseline.trace);
            if actual_digest != expected {
                return Err(RegressionError::BaselineConflict {
                    case: case.clone(),
                    expected: expected.cloned(),
                    actual: actual_digest.cloned(),
                });
            }
            write_json_atomic(&self.baseline_path(case), next, true)
        })();
        lock.unlock()?;
        result
    }

    fn baseline(&self, case: &RegressionCaseId) -> Result<BaselineRef, RegressionError> {
        self.read_baseline_optional(case)?
            .ok_or_else(|| RegressionError::BaselineNotFound(case.clone()))
    }

    fn append_report(&self, report: &RegressionReport) -> Result<(), RegressionError> {
        self.trace(&report.baseline)?;
        self.trace(&report.candidate)?;
        let directory = self.report_directory(&report.case);
        ensure_private_directory(&directory)?;
        let path = directory.join(format!("{}.json", report.id));
        match write_json_atomic(&path, report, false) {
            Err(RegressionError::Io(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                Err(RegressionError::ReportAlreadyExists(report.id.clone()))
            }
            result => result,
        }
    }

    fn reports(&self, case: &RegressionCaseId) -> Result<Vec<RegressionReport>, RegressionError> {
        let directory = self.report_directory(case);
        reject_symlink(&directory)?;
        if !directory.try_exists()? {
            return Ok(Vec::new());
        }
        let mut reports = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                let report: RegressionReport = read_json(&path)?;
                let file_id = path.file_stem().and_then(|name| name.to_str());
                if report.case != *case || file_id != Some(report.id.as_str()) {
                    return Err(RegressionError::CorruptArtifact {
                        path,
                        message: "report case or id does not match its storage path".into(),
                    });
                }
                self.trace(&report.baseline)?;
                self.trace(&report.candidate)?;
                reports.push(report);
            }
        }
        reports.sort_by(|left: &RegressionReport, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(reports)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, RegressionError> {
    reject_symlink(path)?;
    let file = File::open(path)?;
    serde_json::from_reader(BufReader::new(file)).map_err(|error| {
        RegressionError::CorruptArtifact {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn write_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
    replace: bool,
) -> Result<(), RegressionError> {
    let parent = path
        .parent()
        .ok_or_else(|| RegressionError::CorruptArtifact {
            path: path.to_path_buf(),
            message: "destination has no parent directory".into(),
        })?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), value)?;
    temporary.as_file_mut().write_all(b"\n")?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    if replace {
        temporary
            .persist(path)
            .map_err(|error| RegressionError::Io(error.error))?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| RegressionError::Io(error.error))?;
    }
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), RegressionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(RegressionError::CorruptArtifact {
                path: path.to_path_buf(),
                message: "symbolic links are not allowed in the regression store".into(),
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), RegressionError> {
    reject_symlink(path)?;
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(RegressionError::CorruptArtifact {
            path: path.to_path_buf(),
            message: "managed regression path is not a directory".into(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
