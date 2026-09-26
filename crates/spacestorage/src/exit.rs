pub const OK: u8 = 0;
pub const USAGE: u8 = 1;
pub const CONFIG_INVALID: u8 = 2;
pub const UNAUTHORIZED: u8 = 3;
pub const CONNECT: u8 = 4;
pub const PENDING_RESTART: u8 = 5;
/// Slice-10 required for data_* jobs (010 CLI contract; shares exit 5 with pending-restart).
pub const SLICE10_REQUIRED: u8 = 5;
