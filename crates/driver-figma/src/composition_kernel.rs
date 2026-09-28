//! Production adapter recovered from the isolated Composition pass.
//! Public FigmaPlanV1 payloads are unchanged. Native execution stays in BridgeHub.
use super::*;
use c::{ConvergenceBudget, Owner, PlanVault, PrincipalBinding};
use semwright_semantic_composition as c;
use std::collections::BTreeSet;

fn contract(e: c::ContractError) -> Error {
    let code = match &e {
        c::ContractError::Stale(_) => ErrorCode::StaleReference,
        c::ContractError::Limit(_) => ErrorCode::ResourceExhausted,
        c::ContractError::Denied(_) => ErrorCode::PolicyDenied,
        _ => ErrorCode::Conflict,
    };
    Error::new(code, format!("composition: {e}"))
}
fn denied(message: &str) -> Error {
    Error::new(ErrorCode::PolicyDenied, message)
}
fn parse(value: &Value) -> semwright_types::Result<FigmaPlanV1> {
    let p: FigmaPlanV1 = c::strict_decode(&serde_json::to_vec(value)?).map_err(contract)?;
    p.verify().map_err(Error::invalid)?;
    Ok(p)
}
struct Issued {
    root: String,
    allowed: BTreeSet<String>,
    roots: BTreeSet<String>,
}
pub struct FigmaCompositionRuntime {
    vault: PlanVault,
    issued: BTreeMap<(Owner, String), Issued>,
}
impl Default for FigmaCompositionRuntime {
    fn default() -> Self {
        Self {
            vault: PlanVault::bounded(64, 16, 512),
            issued: BTreeMap::new(),
        }
    }
}
impl FigmaCompositionRuntime {
    fn known(&self, o: &Owner, p: &FigmaPlanV1) -> semwright_types::Result<&Issued> {
        self.vault.matches(o, &p.digest, p).map_err(contract)?;
        self.issued
            .get(&(o.clone(), p.digest.clone()))
            .ok_or_else(|| denied("plan was not issued by this driver to this host session"))
    }
    fn issue(
        &mut self,
        o: &Owner,
        p: &FigmaPlanV1,
        parent: Option<&FigmaPlanV1>,
    ) -> semwright_types::Result<()> {
        if self.issued.len() >= 64 && !self.issued.contains_key(&(o.clone(), p.digest.clone())) {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Figma composition plan capacity; finish/restart and inspect before replanning",
            ));
        }
        let operations = u32::try_from(p.changeset.creates.len() + p.changeset.modifies.len())
            .map_err(|_| Error::invalid("operation count"))?;
        let budget = if let Some(source) = parent {
            self.vault
                .root_budget(o, &source.digest)
                .map_err(contract)?
        } else {
            ConvergenceBudget {
                max_iterations: p.spec.budgets.max_iterations,
                // Preserve legacy initial create capacity; max_mutations limits the
                // additional repair writes, not a surprise cap on initial max_nodes.
                max_operations: operations
                    .saturating_add(p.spec.budgets.max_mutations)
                    .max(1),
                max_findings: p.spec.budgets.max_findings_per_round,
                max_observations: 32,
                max_elapsed_ms: 300_000,
            }
        };
        let mut allowed = p
            .spec
            .nodes
            .iter()
            .filter_map(|n| n.existing_node_id.clone())
            .collect::<BTreeSet<_>>();
        let mut roots = p
            .spec
            .nodes
            .iter()
            .filter(|n| n.parent.is_none())
            .filter_map(|n| n.existing_node_id.clone())
            .collect::<BTreeSet<_>>();
        let root = if let Some(source) = parent {
            let old = self.known(o, source)?;
            allowed.extend(old.allowed.iter().cloned());
            roots.extend(old.roots.iter().cloned());
            for m in &p.changeset.modifies {
                if !allowed.contains(&m.node_id) {
                    return Err(denied("repair target outside original composition"));
                }
            }
            old.root.clone()
        } else {
            p.digest.clone()
        };
        self.vault
            .issue(
                o,
                &p.digest,
                p,
                budget,
                operations,
                parent.map(|p| p.digest.as_str()),
                parent.is_some(),
            )
            .map_err(contract)?;
        self.issued
            .entry((o.clone(), p.digest.clone()))
            .or_insert(Issued {
                root,
                allowed,
                roots,
            });
        Ok(())
    }
    fn record(
        &mut self,
        o: &Owner,
        p: &FigmaPlanV1,
        value: &Value,
    ) -> semwright_types::Result<Vec<String>> {
        let issued = self
            .issued
            .get_mut(&(o.clone(), p.digest.clone()))
            .ok_or_else(|| denied("missing issued plan"))?;
        let mut effects = vec![];
        for key in ["created", "modified"] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                if items.len() > 512 {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "oversized effect receipt",
                    ));
                }
                for item in items {
                    let id = item
                        .get("nodeId")
                        .or_else(|| item.get("node_id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorCode::ProtocolMismatch,
                                "native effect missing node identity",
                            )
                        })?;
                    c::bounded_id(id).map_err(contract)?;
                    issued.allowed.insert(id.into());
                    effects.push(format!("{key}:{id}"));
                }
            }
        }
        if let Some(roots) = value.get("rootNodeIds").and_then(Value::as_array) {
            for root in roots {
                let id = root.as_str().ok_or_else(|| {
                    Error::new(ErrorCode::ProtocolMismatch, "invalid native root")
                })?;
                if !issued.allowed.contains(id) {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "root not in observed effects",
                    ));
                }
                issued.roots.insert(id.into());
            }
        }
        let root = issued.root.clone();
        let allowed = issued.allowed.clone();
        let roots = issued.roots.clone();
        if let Some(original) = self.issued.get_mut(&(o.clone(), root)) {
            original.allowed.extend(allowed);
            original.roots.extend(roots);
        }
        Ok(effects)
    }
}
impl FigmaDriver {
    pub(super) async fn execute_composition_with_context(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
        context: &DriverExecutionContext,
    ) -> semwright_types::Result<Value> {
        context.check_cancelled()?;
        if self.descriptors.get(command).map(String::as_str) != Some(digest) {
            return Err(Error::new(ErrorCode::Conflict, "descriptor digest changed"));
        }
        // v2 has an implicit single-process host session supplied by the SDK.
        // v3/v4 carry actual Broker sessions. Neither comes from capability args.
        let owner = Owner {
            session: context.session().into(),
            principal: PrincipalBinding::HostSession,
        };
        let source = if matches!(
            command,
            "driver.figma.composition.apply"
                | "driver.figma.composition.repair.apply"
                | "driver.figma.composition.repair.plan"
        ) {
            Some(parse(
                args.get("plan")
                    .ok_or_else(|| Error::invalid("plan required"))?,
            )?)
        } else {
            None
        };
        if let Some(p) = &source {
            self.composition_runtime.known(&owner, p)?;
        }
        if command == "driver.figma.composition.repair.plan" {
            let p = source.as_ref().expect("repair source");
            let issued = self.composition_runtime.known(&owner, p)?;
            let roots = issued.roots.iter().cloned().collect::<Vec<_>>();
            let allowed = issued.allowed.clone();
            if roots.is_empty() || roots.len() > 32 {
                return Err(denied(
                    "repair needs bounded roots from a completed native composition",
                ));
            }
            let findings = args
                .get("findings")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::invalid("findings required"))?;
            if findings.is_empty() {
                return Err(denied("repair requires at least one native finding"));
            }
            if findings.len() > p.spec.budgets.max_findings_per_round as usize {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "repair finding count exceeds the plan's fixed per-round budget",
                ));
            }
            let expected = args
                .get("expected_revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::StaleReference,
                        "fresh validation revision required",
                    )
                })?;
            let session = self.semantic_session(Some(&p.base.session_id)).await?;
            Self::ensure_plan_base(&session, p, false)?;
            Self::ensure_expected_revision(&session, Some(expected))?;
            let mut observed = BTreeSet::new();
            for root in roots {
                context.check_cancelled()?;
                let result=self.hub.execute(Some(&p.base.session_id),"composition.validate",Some(expected),json!({"root_node_id":root,"spec":p.spec,"max_findings":p.spec.budgets.max_findings_per_round})).await.map_err(|e|Self::bridge_error(e,false))?;
                for finding in result
                    .get("findings")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::ProtocolMismatch,
                            "native validation has no findings",
                        )
                    })?
                {
                    observed.insert(c::canonical_digest(finding).map_err(contract)?);
                }
            }
            for finding in findings {
                let id = finding
                    .get("subject_node_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| denied("repair target missing"))?;
                if !allowed.contains(id)
                    || finding.get("confidence_class").and_then(Value::as_str)
                        != Some("DETERMINISTIC")
                    || finding
                        .get("suggested_repairs")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        != Some(1)
                    || !observed.contains(&c::canonical_digest(finding).map_err(contract)?)
                {
                    return Err(denied("forged, ambiguous, stale or unowned repair finding"));
                }
            }
            self.composition_runtime
                .vault
                .record_observation(&owner, &p.digest, findings.len() as u32)
                .map_err(contract)?;
        }
        let mut permit = None;
        if matches!(
            command,
            "driver.figma.composition.apply" | "driver.figma.composition.repair.apply"
        ) {
            let p = source.as_ref().expect("apply source");
            let session = self.semantic_session(Some(&p.base.session_id)).await?;
            Self::ensure_plan_base(&session, p, true)?;
            permit = Some(
                self.composition_runtime
                    .vault
                    .begin(&owner, &p.digest, p, context.request_id())
                    .map_err(contract)?,
            );
        }
        context.check_cancelled()?;
        let result = self.execute_native(command, digest, args).await;
        let value = match result {
            Ok(value) => {
                if let Some(permit) = permit {
                    let p = source.as_ref().expect("apply source");
                    match self.composition_runtime.record(&owner, p, &value) {
                        Ok(effects) => self
                            .composition_runtime
                            .vault
                            .finish(permit, c::ExecutionStatus::Completed, effects)
                            .map_err(contract)?,
                        Err(e) => {
                            self.composition_runtime
                                .vault
                                .finish(permit, c::ExecutionStatus::Unknown, vec![])
                                .map_err(contract)?;
                            return Err(e);
                        }
                    }
                }
                value
            }
            Err(error) => {
                if let Some(permit) = permit {
                    self.composition_runtime
                        .vault
                        .finish(permit, c::ExecutionStatus::Unknown, vec![])
                        .map_err(contract)?;
                }
                return Err(error);
            }
        };
        if matches!(
            command,
            "driver.figma.composition.plan" | "driver.figma.composition.repair.plan"
        ) {
            let p = parse(&value)?;
            self.composition_runtime
                .issue(&owner, &p, source.as_ref())?;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_owner_is_not_a_plugin_session_or_payload_field() {
        let a = Owner {
            session: "broker-a".into(),
            principal: PrincipalBinding::HostSession,
        };
        let b = Owner {
            session: "broker-b".into(),
            principal: PrincipalBinding::HostSession,
        };
        assert_ne!(a, b);
        let mut runtime = FigmaCompositionRuntime::default();
        let p = json!({"base":{"session_id":"same-plugin"},"digest":"client-recomputed"});
        let budget = ConvergenceBudget {
            max_iterations: 2,
            max_operations: 4,
            max_findings: 8,
            max_observations: 4,
            max_elapsed_ms: 30000,
        };
        runtime
            .vault
            .issue(&a, "plan", &p, budget, 1, None, false)
            .unwrap();
        assert!(runtime.vault.matches(&b, "plan", &p).is_err());
        let changed = json!({"base":{"session_id":"same-plugin"},"target":"foreign","digest":"client-recomputed"});
        assert!(runtime.vault.matches(&a, "plan", &changed).is_err());
    }
}
