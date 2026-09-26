//! RTT EMA for ladder-rank overrides (004/005).

#[derive(Debug, Clone)]
pub struct RttEma {
    alpha: f64,
    value_secs: Option<f64>,
}

impl RttEma {
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha,
            value_secs: None,
        }
    }

    pub fn observe(&mut self, sample_secs: f64) -> f64 {
        self.value_secs = Some(match self.value_secs {
            None => sample_secs,
            Some(prev) => self.alpha * sample_secs + (1.0 - self.alpha) * prev,
        });
        self.value_secs.unwrap()
    }

    pub fn get(&self) -> Option<f64> {
        self.value_secs
    }
}
