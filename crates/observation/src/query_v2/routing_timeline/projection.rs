use super::*;

fn held(request: &Request, code: LedgerReasonCodeV1) -> bool {
    request
        .decision
        .as_ref()
        .is_some_and(|d| d.reason_ledger.iter().any(|r| r.code == code))
}

pub(super) fn project(
    mut requests: BTreeMap<String, Request>,
    facts: &[Fact],
) -> Result<Vec<Projected>, ObservationV2Error> {
    for fact in facts {
        let Some(request) = requests.get_mut(&fact.request_id) else {
            continue;
        };
        match &fact.value {
            ExecutionFactV1::RouteDecision(decision) => {
                request.decision = Some((**decision).clone());
            }
            ExecutionFactV1::CandidateDecision(candidate) => {
                request.candidate = Some((
                    candidate.model_configuration_id.clone(),
                    candidate.reasoning_profile_id.clone(),
                ));
            }
            ExecutionFactV1::AttemptStarted {
                ordinal,
                request_model,
                ..
            } => {
                request.models.insert(*ordinal, request_model.clone());
            }
            ExecutionFactV1::RequestFinished {
                accepted_attempt_ordinal,
                ..
            } => {
                request.view.native_model =
                    accepted_attempt_ordinal.and_then(|n| request.models.get(&n).cloned());
            }
            _ => {}
        }
    }
    for request in requests.values_mut() {
        if request.view.native_model.is_none() {
            request.view.native_model = request
                .models
                .last_key_value()
                .map(|(_, model)| model.clone());
        }
    }
    let mut ordered: Vec<_> = requests.values().collect();
    ordered.sort_by_key(|r| (r.first_fact, &r.view.request_id));
    let positions: BTreeMap<_, _> = ordered
        .iter()
        .enumerate()
        .map(|(i, r)| (r.view.request_id.as_str(), i))
        .collect();
    let mut covered = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    let mut ordinals = BTreeMap::new();
    let mut finished = BTreeMap::new();
    for fact in facts {
        if let ExecutionFactV1::AgentTurnFinished { agent_turn_id, .. } = &fact.value {
            finished.insert(agent_turn_id, fact);
        }
    }
    for fact in finished.into_values() {
        let ExecutionFactV1::AgentTurnFinished {
            agent_turn_id,
            segment_id,
            ordinal,
            plan_id,
            plan_revision,
            selected_branch_id,
            executed_branch_id,
            model_configuration_id,
            branch_execution,
            started_at_ms,
            finished_at_ms,
            status,
            first_request_id,
            last_request_id,
            history_partial,
            ..
        } = &fact.value
        else {
            continue;
        };
        let Some((first, last)) = first_request_id
            .as_ref()
            .zip(last_request_id.as_ref())
            .and_then(|(first, last)| {
                positions
                    .get(first.as_str())
                    .zip(positions.get(last.as_str()))
            })
        else {
            continue;
        };
        if first > last {
            return Err(ObservationV2Error::Unavailable);
        }
        let members = &ordered[*first..=*last];
        if members
            .iter()
            .any(|r| covered.contains(r.view.request_id.as_str()))
        {
            // Concurrent or conflicting ranges cannot manufacture a second assignment.
            return Err(ObservationV2Error::Unavailable);
        }
        covered.extend(members.iter().map(|r| r.view.request_id.as_str()));
        let mut turn = make_turn(members)?;
        turn.position.facts = turn.position.facts.max(fact.position);
        turn.view.agent_turn_id = Some(agent_turn_id.clone());
        turn.view.segment_id = Some(segment_id.clone());
        turn.view.started_at_ms =
            i64::try_from(*started_at_ms).map_err(|_| ObservationV2Error::Unavailable)?;
        turn.view.finished_at_ms =
            Some(i64::try_from(*finished_at_ms).map_err(|_| ObservationV2Error::Unavailable)?);
        turn.view.plan_id = Some(plan_id.to_string());
        turn.view.plan_revision = Some(*plan_revision);
        turn.view.branch_id = executed_branch_id
            .clone()
            .or_else(|| Some(selected_branch_id.clone()));
        turn.view.model_configuration_id = model_configuration_id.clone();
        turn.view.group = branch_execution.as_ref().map(|e| e.group);
        turn.view.competence_floor_millis =
            branch_execution.as_ref().map(|e| e.policy.floor_millis);
        turn.view.status = serde_json::to_value(status)
            .map_err(|_| ObservationV2Error::Unavailable)?
            .as_str()
            .ok_or(ObservationV2Error::Unavailable)?
            .into();
        turn.view.evidence_partial |= *history_partial;
        ordinals.insert(turn.view.turn_id.clone(), *ordinal);
        result.push(turn);
    }
    // Requests arrive before AgentTurnFinished. Keep their opening-request ID stable.
    let mut pending = Vec::new();
    for request in &ordered {
        if covered.contains(request.view.request_id.as_str()) {
            if !pending.is_empty() {
                result.push(make_turn(&pending)?);
                pending.clear();
            }
            continue;
        }
        if !pending.is_empty() && !held(request, LedgerReasonCodeV1::ContextHoldApplied) {
            result.push(make_turn(&pending)?);
            pending.clear();
        }
        pending.push(*request);
    }
    if !pending.is_empty() {
        result.push(make_turn(&pending)?);
    }
    result.sort_by(|a, b| {
        (a.view.started_at_ms, &a.view.turn_id).cmp(&(b.view.started_at_ms, &b.view.turn_id))
    });
    for index in 0..result.len() {
        let first = requests
            .get(&result[index].view.turn_id)
            .ok_or(ObservationV2Error::Unavailable)?;
        if held(first, LedgerReasonCodeV1::ContextHoldInvalidated) {
            result[index].view.boundary = RoutingTurnBoundaryV1::ContextRebuild;
        } else if index > 0 && !held(first, LedgerReasonCodeV1::ContextHoldApplied) {
            let previous = requests
                .get(&result[index - 1].view.turn_id)
                .ok_or(ObservationV2Error::Unavailable)?;
            if first
                .native_turn
                .as_ref()
                .zip(previous.native_turn.as_ref())
                .is_some_and(|(a, b)| a != b)
                && first.native_session.is_some()
                && first.native_session == previous.native_session
                && first.run_id == previous.run_id
                && first
                    .decision
                    .as_ref()
                    .and_then(|d| d.complexity.as_ref())
                    .is_some_and(|s| s.decision_source != ComplexityDecisionSourceV1::Inherited)
            {
                result[index].view.boundary = RoutingTurnBoundaryV1::NewUserMessage;
            }
        }
        if index + 1 < result.len() {
            result[index].view.assessment_state = "unrated".into();
            let next = result[index + 1].position;
            result[index].position.include(next);
        } else {
            result[index].view.assessment_state =
                if matches!(result[index].view.status.as_str(), "failed" | "interrupted") {
                    "unrated"
                } else {
                    "pending"
                }
                .into();
        }
    }
    for fact in facts {
        let ExecutionFactV1::BranchAssessmentRecorded {
            segment_id,
            plan_id,
            plan_revision,
            model_configuration_id,
            trigger_request_id,
            target_from_turn_id,
            target_through_turn_id,
            target_from_ordinal,
            target_through_ordinal,
            assessed_at_ms,
            score,
            partial,
            ..
        } = &fact.value
        else {
            continue;
        };
        for turn in &mut result {
            if turn.view.segment_id.as_ref() != Some(segment_id)
                || turn.view.plan_id.as_deref() != Some(plan_id.as_str())
                || turn.view.plan_revision != Some(*plan_revision)
                || turn.view.model_configuration_id.as_ref() != Some(model_configuration_id)
                || !ordinals
                    .get(&turn.view.turn_id)
                    .is_some_and(|n| n >= target_from_ordinal && n <= target_through_ordinal)
            {
                continue;
            }
            if turn
                .view
                .assessment
                .as_ref()
                .is_some_and(|a| a.assessed_at_ms > *assessed_at_ms as i64)
            {
                continue;
            }
            turn.position.facts = turn.position.facts.max(fact.position);
            turn.view.assessment_state = if *partial { "partial" } else { "scored" }.into();
            turn.view.assessment = Some(PlanQualityAssessment {
                event_id: fact.event_id.clone(),
                trigger_request_id: trigger_request_id.to_string(),
                assessed_at_ms: *assessed_at_ms as i64,
                target_from_turn_id: target_from_turn_id.to_string(),
                target_through_turn_id: target_through_turn_id.to_string(),
                target_from_ordinal: *target_from_ordinal,
                target_through_ordinal: *target_through_ordinal,
                score: *score,
                partial: *partial,
                reason: None,
                evidence_available: requests.contains_key(trigger_request_id.as_str()),
            });
        }
    }
    let protections: Vec<_> = result
        .iter()
        .filter_map(|turn| {
            turn.view
                .selection
                .as_ref()?
                .competence_trigger
                .as_ref()
                .map(|trigger| {
                    (
                        trigger.segment_id.clone(),
                        turn.view.turn_id.clone(),
                        turn.position,
                    )
                })
        })
        .collect();
    for turn in &mut result {
        for (segment, id, position) in &protections {
            if turn.view.segment_id.as_ref() == Some(segment) {
                turn.view.protected_turn_ids.push(id.clone());
                turn.position.include(*position);
            }
        }
        turn.view.content_digest = CanonicalDigest::of(&turn.view)
            .map_err(|_| ObservationV2Error::Unavailable)?
            .to_string();
    }
    Ok(result)
}

fn make_turn(members: &[&Request]) -> Result<Projected, ObservationV2Error> {
    let first = members.first().ok_or(ObservationV2Error::Unavailable)?;
    let mut position = Position::default();
    let mut costs: BTreeMap<
        (Option<String>, Option<String>, Option<String>),
        RoutingTimelineCostV1,
    > = BTreeMap::new();
    let mut relays = Vec::new();
    for member in members {
        position.include(member.position);
        let models: Vec<_> = member.models.values().collect();
        for pair in models.windows(2).filter(|p| p[0] != p[1]) {
            relays.push(RoutingTimelineRelayV1 {
                request_id: member.view.request_id.clone(),
                from_model: Some(pair[0].clone()),
                to_model: Some(pair[1].clone()),
            });
        }
        for cost in &member.view.costs {
            let key = (
                cost.model_configuration_id.clone(),
                cost.currency.clone(),
                cost.valuation_kind.clone(),
            );
            match costs.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(cost.clone());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let value = entry.get_mut();
                    value.unknown_contributions = value
                        .unknown_contributions
                        .checked_add(cost.unknown_contributions)
                        .ok_or(ObservationV2Error::ScopeTooLarge)?;
                    value.known_micros = match (value.known_micros, cost.known_micros) {
                        (Some(a), Some(b)) => {
                            Some(a.checked_add(b).ok_or(ObservationV2Error::ScopeTooLarge)?)
                        }
                        (a, b) => a.or(b),
                    };
                }
            }
        }
    }
    let decision = first.decision.as_ref();
    let selection = decision.and_then(|d| d.complexity.clone());
    Ok(Projected {
        position,
        view: RoutingTimelineTurnV1 {
            turn_id: first.view.request_id.clone(),
            agent_turn_id: None,
            segment_id: None,
            started_at_ms: first.view.started_at_ms,
            finished_at_ms: None,
            boundary: RoutingTurnBoundaryV1::Unknown,
            plan_id: decision.and_then(|d| d.plan_id.as_ref().map(ToString::to_string)),
            plan_revision: decision.and_then(|d| d.route.plan_revision()),
            branch_id: selection.as_ref().map(|s| s.branch_id.clone()),
            model_configuration_id: first.candidate.as_ref().map(|c| c.0.clone()),
            reasoning_profile_id: first.candidate.as_ref().and_then(|c| c.1.clone()),
            group: selection.as_ref().map(|s| s.execution_group),
            competence_floor_millis: selection
                .as_ref()
                .and_then(|s| s.policy.as_ref().map(|p| p.floor_millis)),
            selection,
            tool_continuations: members
                .iter()
                .filter(|r| held(r, LedgerReasonCodeV1::ContextHoldApplied))
                .count() as u64,
            relays,
            assessment_state: "pending".into(),
            assessment: None,
            protected_turn_ids: Vec::new(),
            status: "running".into(),
            requests: members.iter().map(|r| r.view.clone()).collect(),
            cost_by_model: costs.into_values().collect(),
            evidence_partial: members.iter().any(|r| r.partial),
            content_digest: String::new(),
        },
    })
}
