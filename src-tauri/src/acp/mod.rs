//! ACP client, normalization, redaction, and conversation reducer.

pub mod normalize;
pub mod process;
pub mod redact;
pub mod reducer;
pub mod types;

/// ACP protocol major version we speak.
pub const ACP_PROTOCOL_MAJOR: u32 = 1;
