//! One physical-member resolver for conversation and monetary consumers.
use crate::query_v2::ObservationV2Error as Error;
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) struct Member {
    pub id: String,
    pub anchor: String,
    pub predecessor: Option<String>,
    pub gap: Option<String>,
    pub edge_versions: Vec<u64>,
}

pub(super) fn resolve(
    db: &Connection,
    w: &str,
    watermark: u64,
    physical: &BTreeSet<String>,
    stable_from: i64,
) -> Result<(Vec<Member>, Vec<u64>), Error> {
    let mut stmt=db.prepare("SELECT e.revision,e.parent_id,e.child_id,e.request_id,e.ingress_ms FROM observation_native_relations_v1 e
      WHERE e.workspace_id=?1 AND e.revision<=?2 AND e.active=1
       AND NOT EXISTS(SELECT 1 FROM observation_native_relations_v1 newer WHERE newer.workspace_id=e.workspace_id AND newer.parent_id=e.parent_id AND newer.child_id=e.child_id AND newer.revision>e.revision AND newer.revision<=?2)")?;
    let edges = stmt
        .query_map(params![w, watermark], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if edges.is_empty() {
        return Ok((
            physical
                .iter()
                .map(|id| Member {
                    id: id.clone(),
                    anchor: id.clone(),
                    predecessor: None,
                    gap: None,
                    edge_versions: vec![],
                })
                .collect(),
            vec![],
        ));
    }
    let mut parents = BTreeMap::<String, (String, u64)>::new();
    let mut gaps = BTreeMap::<String, String>::new();
    let mut successors = BTreeMap::<String, Vec<String>>::new();
    for (rev, parent, child, request, ingress) in edges {
        if parent == child {
            gaps.insert(child, "lineage_cycle".into());
            continue;
        }
        if !edge_usable(db, w, &parent, &child, &request, stable_from)? {
            gaps.insert(child, "predecessor_unavailable".into());
            continue;
        }
        let _ = ingress;
        if parents.contains_key(&child) {
            gaps.insert(child.clone(), "competing_predecessors".into());
        }
        parents.insert(child.clone(), (parent.clone(), rev));
        successors.entry(parent).or_default().push(child);
    }
    for children in successors.values().filter(|children| children.len() > 1) {
        for child in children {
            gaps.insert(child.clone(), "competing_successors".into());
        }
    }
    let mut connected = physical.clone();
    let mut queue: std::collections::VecDeque<_> = physical.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        if !gaps.contains_key(&id) {
            if let Some((parent, _)) = parents.get(&id) {
                if connected.insert(parent.clone()) {
                    queue.push_back(parent.clone());
                }
            }
        }
        for child in successors.get(&id).into_iter().flatten() {
            if !gaps.contains_key(child) && connected.insert(child.clone()) {
                queue.push_back(child.clone());
            }
        }
    }
    let mut members = Vec::with_capacity(connected.len());
    let mut roots = BTreeMap::<String, (String, Option<String>)>::new();
    let mut group_edges = BTreeMap::<String, BTreeSet<u64>>::new();
    for id in &connected {
        let mut node = id.clone();
        let mut seen = BTreeSet::new();
        let mut path = Vec::<(String, u64)>::new();
        let mut gap = None;
        let mut cycle = false;
        loop {
            if let Some((root, reason)) = roots.get(&node) {
                node = root.clone();
                gap = reason.clone();
                break;
            }
            if let Some(reason) = gaps.get(&node) {
                gap = Some(reason.clone());
                break;
            }
            if !seen.insert(node.clone()) {
                cycle = true;
                gap = Some("lineage_cycle".into());
                break;
            }
            let Some((parent, rev)) = parents.get(&node) else {
                break;
            };
            path.push((node.clone(), *rev));
            node = parent.clone();
        }
        if cycle {
            for (id, _) in &path {
                roots.insert(id.clone(), (id.clone(), gap.clone()));
                gaps.insert(id.clone(), "lineage_cycle".into());
            }
            node = id.clone();
            path.clear();
        } else {
            roots.insert(node.clone(), (node.clone(), gap.clone()));
            for (child, rev) in path {
                roots.insert(child, (node.clone(), gap.clone()));
                group_edges.entry(node.clone()).or_default().insert(rev);
            }
        }
        let predecessor = if gaps.contains_key(id) {
            None
        } else {
            parents.get(id).map(|p| p.0.clone())
        };
        members.push(Member {
            id: id.clone(),
            anchor: node,
            predecessor,
            gap: gap.or_else(|| gaps.get(id).cloned()),
            edge_versions: vec![],
        });
    }
    let mut used = BTreeSet::new();
    // Every included physical group is selected as a whole. Keep its proof
    // set once, avoiding quadratic paths for long proven compression chains.
    for m in &mut members {
        if let Some(edges) = group_edges.remove(&m.anchor) {
            used.extend(edges.iter().copied());
            m.edge_versions = edges.into_iter().collect();
        }
    }
    Ok((members, used.into_iter().collect()))
}

pub(super) fn edge_usable(
    db: &Connection,
    w: &str,
    parent: &str,
    child: &str,
    request: &str,
    stable_from: i64,
) -> Result<bool, Error> {
    // Generic barriers do not prove user deletion: expiry produces them too.
    // When old origin is unavailable they only make ancestry unavailable. A
    // ContentOnly deletion retains its source requests and keeps grouping.
    let usable:bool=db.query_row("SELECT
      EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND request_id=?4 AND started_at_ms>=?5)
      AND EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND session_id=?2 AND started_at_ms>=?5)
      AND NOT EXISTS(SELECT 1 FROM observation_analysis_cuts_v1 WHERE workspace_id=?1 AND session_id IN(?2,?3))
      AND NOT EXISTS(SELECT 1 FROM observation_session_barriers_v2 b WHERE b.workspace_id=?1 AND b.session_id IN(?2,?3)
        AND NOT EXISTS(SELECT 1 FROM logical_requests r WHERE r.workspace_id=b.workspace_id AND r.session_id=b.session_id AND r.started_at_ms<=b.through_ms))",params![w,parent,child,request,stable_from],|r|r.get(0))?;
    Ok(usable)
}
