//! `agentero ui wait --idle`: wait until the Host job queue stays empty.
//!
//! "Idle" means no JobCenter job (import, parse, layout, translate, sync,
//! downloads, compile, …) is queued or running, continuously for
//! [`SETTLE`]. The settle window covers jobs that enqueue follow-up work as
//! they finish. Renderer activity outside the job queue (Agent streaming, PDF
//! tile rendering, network fetches) is not tracked; wait for a result control
//! with `ui wait <control>` instead.

use super::host_action::{HostAction, HostActionError};
use super::{UiIdleSpec, UiRequestPayload};
use crate::features::jobs::{JobCenter, JobState};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime};

const POLL: Duration = Duration::from_millis(250);
const SETTLE: Duration = Duration::from_millis(750);

pub async fn handle<R: Runtime>(app: AppHandle<R>, req: UiRequestPayload) {
    let Some(action) = HostAction::start(&req) else {
        return;
    };
    let result = wait(&app, &req).await;
    action.finish(result);
}

async fn wait<R: Runtime>(
    app: &AppHandle<R>,
    req: &UiRequestPayload,
) -> Result<serde_json::Value, HostActionError> {
    let spec = req
        .path
        .as_deref()
        .and_then(|spec| serde_json::from_str::<UiIdleSpec>(spec).ok())
        .ok_or_else(|| HostActionError::new("invalid_argument", "invalid idle wait request"))?;
    let center = app
        .try_state::<JobCenter>()
        .ok_or_else(|| HostActionError::new("action_failed", "the job queue is not ready yet"))?
        .handle();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(spec.timeout_ms);
    let mut idle_since: Option<Instant> = None;
    loop {
        let mut active = BTreeMap::<String, usize>::new();
        for job in center.list(None, None).await {
            if matches!(job.state, JobState::Queued | JobState::Running) {
                *active.entry(format!("{:?}", job.kind)).or_default() += 1;
            }
        }
        let now = Instant::now();
        if active.is_empty() {
            let since = *idle_since.get_or_insert(now);
            // At the deadline an empty queue still counts as idle.
            if now.duration_since(since) >= SETTLE || now >= deadline {
                let elapsed_ms = now.duration_since(started).as_millis() as u64;
                return Ok(serde_json::json!({
                    "action": "wait",
                    "idle": true,
                    "elapsedMs": elapsed_ms,
                    "lines": [format!("The job queue is idle after {elapsed_ms} ms")],
                }));
            }
        } else {
            idle_since = None;
            if now >= deadline {
                let total: usize = active.values().sum();
                let kinds = active
                    .iter()
                    .map(|(kind, count)| format!("{kind}×{count}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(HostActionError::new(
                    "wait_timeout",
                    format!(
                        "Timed out after {} ms waiting for the app to become idle; {total} job(s) still active: {kinds}.",
                        spec.timeout_ms
                    ),
                ));
            }
        }
        tokio::time::sleep(POLL.min(deadline.saturating_duration_since(now))).await;
    }
}
