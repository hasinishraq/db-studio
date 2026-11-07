//! Id newtypes over [`uuid::Uuid`], serialized as strings.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type)]
        #[serde(transparent)]
        pub struct $name(#[specta(type = String)] pub Uuid);

        impl $name {
            /// Creates a new random id.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

id_type!(
    /// A saved connection profile.
    ProfileId
);
id_type!(
    /// An open database session.
    SessionId
);
id_type!(
    /// One execution of a script.
    RunId
);
id_type!(
    /// One statement proposed by the AI in a turn.
    QueryId
);
id_type!(
    /// An AI conversation.
    ConversationId
);
id_type!(
    /// One user message and its reply.
    TurnId
);
id_type!(
    /// A saved query draft.
    DraftId
);
