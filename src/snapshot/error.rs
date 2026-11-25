use std::fmt;

#[derive(Debug)]
pub enum SnapshotScopeError<E> {
    Setup(anyhow::Error),
    ExporterLost(anyhow::Error),
    Work(E),
    Rollback {
        error: anyhow::Error,
        prior_failure: Option<Box<Self>>,
    },
}

impl<E: fmt::Display> fmt::Display for SnapshotScopeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(error) => write!(f, "Source reader setup: {error:#}"),
            Self::ExporterLost(error) => {
                write!(f, "Source coordinator session disconnected: {error:#}")
            }
            Self::Work(error) => write!(f, "Source reader callback: {error:#}"),
            Self::Rollback {
                error,
                prior_failure,
            } => {
                if let Some(failure) = prior_failure {
                    write!(f, "{failure}. ")?;
                }
                write!(f, "Source rollback failed: {error:#}")
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for SnapshotScopeError<E> {}
