//! Bounded session-scoped job state. Arguments are never retained after dispatch.
use super::*;

const MAX_JOBS: usize = 256;
const MAX_SESSION_JOBS: usize = 64;
const MAX_ACTIVE_PER_SESSION: usize = 16;
const MAX_RETAINED_RESULT: usize = 262_144;

struct JobEntry {
    owner: String,
    snapshot: JobSnapshot,
    cancellation: CancellationToken,
}

#[derive(Default)]
pub(super) struct JobStore {
    entries: BTreeMap<String, JobEntry>,
    order: VecDeque<String>,
}

impl JobStore {
    fn remove_oldest_terminal(&mut self, owner: Option<&str>) -> bool {
        let Some(position) = self.order.iter().position(|id| {
            self.entries.get(id).is_some_and(|entry| {
                entry.snapshot.state.terminal() && owner.is_none_or(|owner| entry.owner == owner)
            })
        }) else {
            return false;
        };
        if let Some(id) = self.order.remove(position) {
            self.entries.remove(&id);
            true
        } else {
            false
        }
    }

    pub fn reserve(
        &mut self,
        owner: &str,
        command: &str,
        cancellation: CancellationToken,
    ) -> Result<JobSnapshot> {
        while self.entries.values().filter(|e| e.owner == owner).count() >= MAX_SESSION_JOBS {
            if !self.remove_oldest_terminal(Some(owner)) {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Session job retention limit reached",
                ));
            }
        }
        while self.entries.len() >= MAX_JOBS {
            if !self.remove_oldest_terminal(None) {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Broker job retention limit reached",
                ));
            }
        }
        if self
            .entries
            .values()
            .filter(|entry| entry.owner == owner && !entry.snapshot.state.terminal())
            .count()
            >= MAX_ACTIVE_PER_SESSION
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Session has too many active jobs",
            ));
        }
        let id = unique_id();
        let snapshot = JobSnapshot {
            id: id.clone(),
            command: command.into(),
            state: JobState::Queued,
            created_at_ms: event_time(),
            started_at_ms: None,
            finished_at_ms: None,
            cancellation_requested: false,
            cancellable: true,
            result: None,
            result_omitted: false,
        };
        self.order.push_back(id.clone());
        self.entries.insert(
            id,
            JobEntry {
                owner: owner.into(),
                snapshot: snapshot.clone(),
                cancellation,
            },
        );
        Ok(snapshot)
    }

    pub fn mark_running(&mut self, id: &str) -> Result<JobSnapshot> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Job not found"))?;
        if entry.snapshot.state == JobState::Queued {
            entry.snapshot.state = JobState::Running;
            entry.snapshot.started_at_ms = Some(event_time());
        }
        Ok(entry.snapshot.clone())
    }

    pub fn finish(&mut self, id: &str, envelope: Envelope) -> Result<JobSnapshot> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Job not found"))?;
        entry.snapshot.state = if envelope.ok {
            JobState::Succeeded
        } else if envelope
            .error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::Cancelled)
        {
            JobState::Cancelled
        } else {
            JobState::Failed
        };
        entry.snapshot.finished_at_ms = Some(event_time());
        entry.snapshot.cancellable = false;
        if serde_json::to_vec(&envelope).is_ok_and(|body| body.len() <= MAX_RETAINED_RESULT) {
            entry.snapshot.result = Some(Box::new(envelope));
        } else {
            entry.snapshot.result = None;
            entry.snapshot.result_omitted = true;
        }
        Ok(entry.snapshot.clone())
    }

    pub fn get(&self, owner: &str, id: &str) -> Result<JobSnapshot> {
        self.entries
            .get(id)
            .filter(|entry| entry.owner == owner)
            .map(|entry| entry.snapshot.clone())
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Job not found"))
    }

    pub fn cancel(&mut self, owner: &str, id: &str) -> Result<(JobSnapshot, bool)> {
        let entry = self
            .entries
            .get_mut(id)
            .filter(|entry| entry.owner == owner)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Job not found"))?;
        if entry.snapshot.state.terminal() {
            return Ok((entry.snapshot.clone(), false));
        }
        let changed = !entry.snapshot.cancellation_requested;
        entry.snapshot.cancellation_requested = true;
        entry.cancellation.cancel();
        Ok((entry.snapshot.clone(), changed))
    }

    pub fn revoke_session(&mut self, owner: &str) -> usize {
        let ids: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.owner == owner)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            if let Some(entry) = self.entries.remove(id) {
                entry.cancellation.cancel();
            }
        }
        self.order.retain(|id| self.entries.contains_key(id));
        ids.len()
    }
}

impl Broker {
    pub(super) fn start_job(
        self: &Arc<Self>,
        session: &str,
        request: ExecuteRequest,
    ) -> Result<JobSnapshot> {
        if self.runtime_stop.is_cancelled() {
            return Err(Error::unavailable("Broker is shutting down"));
        }
        if request.command.starts_with("jobs.") {
            return Err(Error::invalid(
                "Job control commands cannot be nested as jobs",
            ));
        }
        let invocation = self.invocation(&request.command)?;
        invocation.capability.validate_input(&request.args)?;
        let cancellation = self.runtime_stop.child_token();
        let snapshot = self
            .jobs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Job store lock poisoned"))?
            .reserve(session, &request.command, cancellation.clone())?;
        self.job_event(session, "job.queued", &snapshot);
        let id = snapshot.id.clone();
        let owner = session.to_owned();
        let broker = self.clone();
        self.job_tasks.spawn(async move {
            let running = broker
                .jobs
                .lock()
                .ok()
                .and_then(|mut jobs| jobs.mark_running(&id).ok());
            if let Some(snapshot) = running {
                broker.job_event(&owner, "job.started", &snapshot);
            }
            let envelope = broker
                .clone()
                .execute(owner.clone(), id.clone(), request, cancellation)
                .await;
            let terminal = broker
                .jobs
                .lock()
                .ok()
                .and_then(|mut jobs| jobs.finish(&id, envelope).ok());
            if let Some(snapshot) = terminal {
                let kind = match snapshot.state {
                    JobState::Succeeded => "job.succeeded",
                    JobState::Cancelled => "job.cancelled",
                    _ => "job.failed",
                };
                broker.job_event(&owner, kind, &snapshot);
            }
        });
        Ok(snapshot)
    }

    pub(super) fn get_job(&self, session: &str, id: &str) -> Result<JobSnapshot> {
        self.jobs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Job store lock poisoned"))?
            .get(session, id)
    }

    pub(super) fn cancel_job(&self, session: &str, id: &str) -> Result<JobSnapshot> {
        let (snapshot, changed) = self
            .jobs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Job store lock poisoned"))?
            .cancel(session, id)?;
        if changed {
            self.job_event(session, "job.cancel_requested", &snapshot);
        }
        Ok(snapshot)
    }

    fn job_event(&self, session: &str, kind: &str, job: &JobSnapshot) {
        self.session_event(
            session,
            self.core_event(kind)
                .with_attribute("job_id", json!(job.id))
                .with_attribute("command", json!(job.command))
                .with_attribute("state", json!(job.state))
                .with_attribute("cancellation_requested", json!(job.cancellation_requested))
                .with_attribute("result_omitted", json!(job.result_omitted)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(ok: bool) -> Envelope {
        Envelope::finish(
            unique_id(),
            "fixture.read".into(),
            "fixture".into(),
            Duration::from_millis(1),
            false,
            if ok {
                Ok(json!({"value":1}))
            } else {
                Err(Error::new(ErrorCode::BackendFailed, "fixture failure"))
            },
        )
    }

    #[test]
    fn jobs_are_session_scoped_and_cancellation_is_idempotent() {
        let mut jobs = JobStore::default();
        let token = CancellationToken::new();
        let job = jobs
            .reserve("session-a", "fixture.read", token.clone())
            .unwrap();
        assert_eq!(
            jobs.get("session-b", &job.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        let (first, changed) = jobs.cancel("session-a", &job.id).unwrap();
        assert!(changed);
        assert!(first.cancellation_requested);
        assert!(token.is_cancelled());
        let (_, changed) = jobs.cancel("session-a", &job.id).unwrap();
        assert!(!changed);
    }

    #[test]
    fn terminal_result_is_retained_and_no_longer_cancellable() {
        let mut jobs = JobStore::default();
        let job = jobs
            .reserve("session-a", "fixture.read", CancellationToken::new())
            .unwrap();
        jobs.mark_running(&job.id).unwrap();
        let terminal = jobs.finish(&job.id, envelope(true)).unwrap();
        assert_eq!(terminal.state, JobState::Succeeded);
        assert!(!terminal.cancellable);
        assert!(terminal.result.as_ref().is_some_and(|result| result.ok));
    }

    #[test]
    fn revoking_a_session_cancels_and_forgets_only_its_jobs() {
        let mut jobs = JobStore::default();
        let a_token = CancellationToken::new();
        let b_token = CancellationToken::new();
        let a = jobs
            .reserve("session-a", "fixture.read", a_token.clone())
            .unwrap();
        let b = jobs
            .reserve("session-b", "fixture.read", b_token.clone())
            .unwrap();
        assert_eq!(jobs.revoke_session("session-a"), 1);
        assert!(a_token.is_cancelled());
        assert!(!b_token.is_cancelled());
        assert_eq!(
            jobs.get("session-a", &a.id).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(jobs.get("session-b", &b.id).unwrap().id, b.id);
    }
}
