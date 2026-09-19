//! AuthZ / keys — first-binary bootstrap admin, SCRAM default 16384, master-key (014).

pub const SCRAM_ITERATIONS_DEFAULT: u32 = 16384;

pub struct MasterKey {
    pub bytes: Vec<u8>,
}

impl MasterKey {
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self { bytes })
    }
}

/// FR-017: implicit grant for bootstrap admin on first-binary.
pub fn bootstrap_admin_implicit_grant() -> bool {
    true
}
