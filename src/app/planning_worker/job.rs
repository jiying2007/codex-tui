//! Projection and reclamation share the existing bounded worker and completion channel.
use super::{Input, Output};

pub(super) enum Job {
    Project(Box<Input>),
    Reclaim(Box<Output>),
}

impl Job {
    pub(super) fn execute(self) -> Option<Output> {
        match self {
            Self::Project(input) => Some(input.compute()),
            Self::Reclaim(output) => {
                drop(output);
                None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn into_projection(self) -> Input {
        match self {
            Self::Project(input) => *input,
            Self::Reclaim(_) => panic!("expected a projection, not a reclamation job"),
        }
    }
}
