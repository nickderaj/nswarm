use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::{CredentialGrant, JobId, Sha, UnitId};

/// Request for one isolated branch and worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRequest {
    /// Existing local repository whose object database is shared read-only.
    pub repository: PathBuf,
    /// New worktree directory under the provisioner's allowed root.
    pub destination: PathBuf,
    /// Job embedded in the branch namespace.
    pub job_id: JobId,
    /// Unit embedded in the branch namespace.
    pub unit_id: UnitId,
    /// Exact base object.
    pub base_sha: Sha,
}

impl WorktreeRequest {
    /// Deterministic branch owned by this one coder unit.
    #[must_use]
    pub fn branch_name(&self) -> String {
        format!("nswarm/{}/{}", self.job_id, self.unit_id)
    }
}

/// Creates isolated worktrees without exposing shell interpretation.
pub trait WorktreeProvisioner {
    /// Creates the requested branch and worktree from an exact base SHA.
    ///
    /// # Errors
    ///
    /// Returns [`ProvisionError`] if scope validation or Git fails.
    fn provision(&self, request: &WorktreeRequest) -> Result<(), ProvisionError>;
}

/// Local Git implementation constrained to direct children of one root.
#[derive(Clone, Debug)]
pub struct LocalWorktreeProvisioner {
    allowed_root: PathBuf,
}

impl LocalWorktreeProvisioner {
    /// Creates a local provisioner rooted at an existing canonical directory.
    ///
    /// # Errors
    ///
    /// Returns [`ProvisionError`] when the root cannot be canonicalized.
    pub fn new(allowed_root: impl AsRef<Path>) -> Result<Self, ProvisionError> {
        Ok(Self {
            allowed_root: allowed_root.as_ref().canonicalize()?,
        })
    }
}

impl WorktreeProvisioner for LocalWorktreeProvisioner {
    // coverage-critical
    fn provision(&self, request: &WorktreeRequest) -> Result<(), ProvisionError> {
        let repository = request.repository.canonicalize()?;
        let git_control = repository.join(".git");
        let Ok(git_metadata) = git_control.symlink_metadata() else {
            return Err(ProvisionError::NotRepository(repository));
        };
        if git_metadata.file_type().is_symlink()
            || !(git_metadata.is_dir() || git_metadata.is_file())
        {
            return Err(ProvisionError::NotRepository(repository));
        }
        match request.destination.symlink_metadata() {
            Ok(_) => {
                return Err(ProvisionError::DestinationExists(
                    request.destination.clone(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let parent = request
            .destination
            .parent()
            .ok_or_else(|| ProvisionError::OutsideAllowedRoot(request.destination.clone()))?
            .canonicalize()?;
        if parent != self.allowed_root {
            return Err(ProvisionError::OutsideAllowedRoot(
                request.destination.clone(),
            ));
        }
        let status = Command::new("git")
            .args(["-C"])
            .arg(&repository)
            .args(["worktree", "add", "-b"])
            .arg(request.branch_name())
            .arg(&request.destination)
            .arg(request.base_sha.as_str())
            .status()?;
        if !status.success() {
            return Err(ProvisionError::GitFailed(status.code()));
        }
        Ok(())
    }
}

/// Opaque credential lease metadata. Secret material never crosses this API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialLease {
    /// Broker-side lease handle suitable for later revocation.
    pub lease_id: String,
    /// Unix timestamp after which the grant is invalid.
    pub expires_at: i64,
}

/// Broker boundary for job-bound, method-scoped credentials.
pub trait CredentialBroker {
    /// Issues opaque lease metadata for a validated grant request.
    ///
    /// # Errors
    ///
    /// Returns [`ProvisionError`] if policy or the backing broker refuses it.
    fn issue(
        &self,
        job_id: &JobId,
        grants: &[CredentialGrant],
        expires_at: i64,
    ) -> Result<Vec<CredentialLease>, ProvisionError>;
}

/// Deterministic broker used by ordinary tests; it owns no secrets.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSecretBroker;

impl CredentialBroker for NoSecretBroker {
    fn issue(
        &self,
        _job_id: &JobId,
        grants: &[CredentialGrant],
        _expires_at: i64,
    ) -> Result<Vec<CredentialLease>, ProvisionError> {
        if grants.is_empty() {
            Ok(Vec::new())
        } else {
            Err(ProvisionError::CredentialUnavailable)
        }
    }
}

/// Safe worktree and credential provisioning failures.
#[derive(Debug, Error)]
pub enum ProvisionError {
    /// Filesystem inspection or Git process launch failed.
    #[error("filesystem or process error: {0}")]
    Io(#[from] std::io::Error),
    /// Source does not contain a Git control directory.
    #[error("not a Git repository: {path}", path = .0.display())]
    NotRepository(PathBuf),
    /// Existing mutable state is never adopted implicitly.
    #[error("worktree destination already exists: {path}", path = .0.display())]
    DestinationExists(PathBuf),
    /// Worktrees are direct children of one scheduler-owned root.
    #[error("worktree destination is outside the allowed root: {path}", path = .0.display())]
    OutsideAllowedRoot(PathBuf),
    /// Git rejected the exact request.
    #[error("git worktree command failed with status {0:?}")]
    GitFailed(Option<i32>),
    /// The deterministic test broker has no credentials to invent.
    #[error("no-secret broker cannot satisfy credential grants")]
    CredentialUnavailable,
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::process::Command;

    use tempfile::TempDir;

    use super::{
        CredentialBroker, LocalWorktreeProvisioner, NoSecretBroker, ProvisionError,
        WorktreeProvisioner, WorktreeRequest,
    };
    use crate::{CredentialGrant, JobId, Sha, UnitId};

    fn git(repository: &std::path::Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .expect("git launches");
        assert!(output.status.success(), "git command failed");
        String::from_utf8(output.stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_owned()
    }

    fn repository() -> (TempDir, Sha) {
        let repository = TempDir::new().expect("temporary repository");
        git(repository.path(), &["init", "-q"]);
        git(repository.path(), &["config", "user.name", "nswarm test"]);
        git(
            repository.path(),
            &["config", "user.email", "test@invalid.example"],
        );
        fs::write(repository.path().join("README.md"), "fixture\n").expect("write fixture");
        git(repository.path(), &["add", "README.md"]);
        git(repository.path(), &["commit", "-q", "-m", "fixture"]);
        let sha =
            Sha::new(git(repository.path(), &["rev-parse", "HEAD"])).expect("Git returns full SHA");
        (repository, sha)
    }

    #[test]
    fn no_secret_broker_returns_no_ambient_credentials() {
        let leases = NoSecretBroker
            .issue(&JobId::new("job-1").expect("valid id"), &[], 100)
            .expect("empty request is allowed");
        assert!(leases.is_empty());
    }

    #[test]
    fn no_secret_broker_refuses_to_invent_a_grant() {
        let error = NoSecretBroker
            .issue(
                &JobId::new("job-1").expect("valid id"),
                &[CredentialGrant {
                    credential_id: "github-job-push".to_owned(),
                    methods: vec!["git:push:assigned-branch".to_owned()],
                }],
                100,
            )
            .expect_err("broker owns no secrets");
        assert!(matches!(error, ProvisionError::CredentialUnavailable));
    }

    #[test]
    fn provisioner_requires_an_existing_scheduler_root() {
        let root = TempDir::new().expect("temporary root");
        let provisioner = LocalWorktreeProvisioner::new(root.path()).expect("canonical root");
        assert!(format!("{provisioner:?}").contains("allowed_root"));
    }

    #[test]
    fn local_provisioner_creates_only_the_scoped_branch() {
        let (repository, base_sha) = repository();
        let root = TempDir::new().expect("worktree root");
        let destination = root.path().join("unit-1");
        let request = WorktreeRequest {
            repository: repository.path().to_path_buf(),
            destination: destination.clone(),
            job_id: JobId::new("job-1").expect("valid job"),
            unit_id: UnitId::new("unit-1").expect("valid unit"),
            base_sha,
        };
        assert_eq!(request.branch_name(), "nswarm/job-1/unit-1");
        LocalWorktreeProvisioner::new(root.path())
            .expect("provisioner")
            .provision(&request)
            .expect("worktree provisioned");
        assert!(destination.join("README.md").is_file());
        assert_eq!(
            git(&destination, &["branch", "--show-current"]),
            "nswarm/job-1/unit-1"
        );
    }

    #[test]
    fn local_provisioner_rejects_existing_outside_and_non_repository_paths() {
        let (repository, base_sha) = repository();
        let root = TempDir::new().expect("worktree root");
        let outside = TempDir::new().expect("outside root");
        let provisioner = LocalWorktreeProvisioner::new(root.path()).expect("provisioner");

        let request = WorktreeRequest {
            repository: repository.path().to_path_buf(),
            destination: outside.path().join("unit-1"),
            job_id: JobId::new("job-1").expect("valid job"),
            unit_id: UnitId::new("unit-1").expect("valid unit"),
            base_sha: base_sha.clone(),
        };
        assert!(matches!(
            provisioner.provision(&request),
            Err(ProvisionError::OutsideAllowedRoot(_))
        ));

        let existing = root.path().join("existing");
        fs::create_dir(&existing).expect("existing destination");
        let request = WorktreeRequest {
            destination: existing,
            ..request
        };
        assert!(matches!(
            provisioner.provision(&request),
            Err(ProvisionError::DestinationExists(_))
        ));

        let not_repository = TempDir::new().expect("plain directory");
        let request = WorktreeRequest {
            repository: not_repository.path().to_path_buf(),
            destination: root.path().join("unit-2"),
            base_sha: base_sha.clone(),
            ..request
        };
        assert!(matches!(
            provisioner.provision(&request),
            Err(ProvisionError::NotRepository(_))
        ));

        fs::write(not_repository.path().join(".git"), "gitdir: missing\n")
            .expect("write invalid git control file");
        let request = WorktreeRequest {
            destination: root.path().join("unit-3"),
            base_sha,
            ..request
        };
        assert!(matches!(
            provisioner.provision(&request),
            Err(ProvisionError::GitFailed(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn local_provisioner_rejects_a_dangling_destination_symlink() {
        let (repository, base_sha) = repository();
        let root = TempDir::new().expect("worktree root");
        let destination = root.path().join("unit-1");
        symlink(root.path().join("missing-target"), &destination).expect("dangling symlink");
        let request = WorktreeRequest {
            repository: repository.path().to_path_buf(),
            destination,
            job_id: JobId::new("job-1").expect("valid job"),
            unit_id: UnitId::new("unit-1").expect("valid unit"),
            base_sha,
        };
        let error = LocalWorktreeProvisioner::new(root.path())
            .expect("provisioner")
            .provision(&request)
            .expect_err("dangling symlink must not be followed");
        assert!(matches!(error, ProvisionError::DestinationExists(_)));

        let symlink_repository = TempDir::new().expect("symlink repository");
        symlink(
            repository.path().join(".git"),
            symlink_repository.path().join(".git"),
        )
        .expect("git control symlink");
        let request = WorktreeRequest {
            repository: symlink_repository.path().to_path_buf(),
            destination: root.path().join("unit-2"),
            ..request
        };
        assert!(matches!(
            LocalWorktreeProvisioner::new(root.path())
                .expect("provisioner")
                .provision(&request),
            Err(ProvisionError::NotRepository(_))
        ));

        let special_repository = TempDir::new().expect("special repository");
        let status = Command::new("mkfifo")
            .arg(special_repository.path().join(".git"))
            .status()
            .expect("mkfifo launches");
        assert!(status.success());
        let request = WorktreeRequest {
            repository: special_repository.path().to_path_buf(),
            destination: root.path().join("unit-3"),
            ..request
        };
        assert!(matches!(
            LocalWorktreeProvisioner::new(root.path())
                .expect("provisioner")
                .provision(&request),
            Err(ProvisionError::NotRepository(_))
        ));

        let request = WorktreeRequest {
            repository: repository.path().to_path_buf(),
            destination: root.path().join("x".repeat(4096)),
            ..request
        };
        assert!(matches!(
            LocalWorktreeProvisioner::new(root.path())
                .expect("provisioner")
                .provision(&request),
            Err(ProvisionError::Io(_))
        ));
    }
}
