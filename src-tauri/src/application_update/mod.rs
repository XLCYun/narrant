mod ports;
mod preference;
mod staging;
mod state;

pub(crate) use ports::{
    Clock, InstallError, InstallIntent, InstallOutcome, StableReleaseProvider, UpdateInstaller,
};
pub(crate) use preference::{FilePreferenceStore, PreferenceError, PreferenceStore};
pub(crate) use staging::{
    InstallKind, MinisignVerifier, StageCandidate, StagedUpdate, StagingContext, StagingError,
    StagingRepository, VerifiedStagedPackage,
};
pub(crate) use state::{
    DownloadProgress, ReleasePackage, StableRelease, UpdateController, UpdateDomainError,
    UpdateEffect, UpdateFailure, UpdateFailureCategory, UpdateInput, UpdateOperation, UpdateOutput,
    UpdateSnapshot, UpdateState,
};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
