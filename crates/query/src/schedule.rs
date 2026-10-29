//! Task scheduling + concurrency degree (005).

use crate::admission::{AdmissionController, AdmissionToken};
use crate::error::ExecError;
use crate::options::Concurrency;
use crate::planner::{PhysicalPlan, plan_is_spillable};
use crate::rank::{RankKey, rank_nodes};

pub struct ScheduleOutcome {
    pub token: AdmissionToken,
    pub ordered_tasks: Vec<usize>,
    pub degree: u16,
}

pub fn schedule(
    admission: &AdmissionController,
    namespace: &str,
    plan: &PhysicalPlan,
    concurrency: Concurrency,
    memory_estimate: u64,
    rank_inputs: Vec<(String, RankKey)>,
) -> Result<ScheduleOutcome, ExecError> {
    let spillable = plan_is_spillable(plan);
    let token = admission.try_acquire_spillable(namespace, memory_estimate, spillable)?;
    let remaining_slots = admission
        .limits()
        .max_concurrent_per_node
        .saturating_sub(1) // this query holds one
        .max(1);
    let mut degree = concurrency.degree();
    if degree > remaining_slots as u16 {
        degree = remaining_slots as u16;
    }
    // Independent tasks: all when Parallel; else sequential index order.
    let ordered_tasks: Vec<usize> = (0..plan.tasks.len()).collect();
    // Apply ranking to placement lists (in-place conceptual — return ranked node names).
    let _ranked = rank_nodes(rank_inputs);
    Ok(ScheduleOutcome {
        token,
        ordered_tasks,
        degree,
    })
}
