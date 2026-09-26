#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    Ok,
    Full,
    Corrupt,
}

#[derive(Debug, Clone)]
pub struct DriveDiskState {
    pub drive_id: String,
    pub state: DiskState,
    pub quarantined: Vec<String>,
}
