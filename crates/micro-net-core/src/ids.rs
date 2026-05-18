//! Strongly typed identifiers used across the simulator.
//!
//! The identifiers are string-backed on purpose: experiment artifacts remain
//! human-readable and stable across graph backend implementations.

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Creates a new identifier from any string-like value.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the raw identifier value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

id_type!(NodeId, "Stable domain identifier of a graph node.");
id_type!(EdgeId, "Stable domain identifier of a graph edge/link.");
id_type!(
    LogicalServiceId,
    "Logical service name, for example `payments` or `orders`."
);
id_type!(
    ServiceInstanceId,
    "Concrete service replica identifier, for example `payments-1`."
);
id_type!(
    LogicalResourceId,
    "Logical shared resource name, for example `payments-db`."
);
id_type!(
    LogicalDependencyId,
    "Stable identifier of a logical dependency declared by a service."
);
id_type!(
    HostId,
    "Host or machine identifier used to model resource colocation."
);
id_type!(ZoneId, "Availability zone or placement group identifier.");
id_type!(ExperimentId, "Stable identifier of one experiment run.");
id_type!(RequestClassId, "Identifier of a request class/profile.");

/// Monotonic simulation tick.
pub type Tick = u64;

/// Stable logical request identifier.
pub type RequestId = u64;
