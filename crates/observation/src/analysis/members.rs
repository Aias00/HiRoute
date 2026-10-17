//! One physical-member resolver for conversation and monetary consumers.
use super::schema;
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
    let mut parents = BTreeMap::<String, (String, u64)>::new();
    let mut gaps = BTreeMap::<String, String>::new();
    let mut successors = BTreeMap::<String, Vec<String>>::new();
    for (rev, parent, child, request, ingress) in edges {
        if parent == child {
            gaps.insert(child, "lineage_cycle".into());
            continue;
        }
        let origin:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND request_id=?2 AND session_id=?3 AND started_at_ms<=?4)",params![w,request,child,ingress],|r|r.get(0))?;
        let parent_exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND session_id=?2)",
            params![w, parent],
            |r| r.get(0),
        )?;
        // A newer observer timestamp cannot resurrect historical ancestry after
        // a full deletion. Only event-origin evidence can establish a new edge;
        // this transport does not provide a new incarnation for a reused ID.
        let cut:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM observation_analysis_cuts_v1 WHERE workspace_id=?1 AND session_id IN(?2,?3))",params![w,parent,child],|r|r.get(0))?;
        if !origin || !parent_exists || cut {
            gaps.insert(child, "predecessor_unavailable".into());
            continue;
        }
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
    let mut members = Vec::with_capacity(physical.len());
    let mut used = BTreeSet::new();
    for id in physical {
        let mut node = id.clone();
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        let mut gap = None;
        while let Some((parent, rev)) = parents.get(&node) {
            if let Some(reason) = gaps.get(&node) {
                gap = Some(reason.clone());
                break;
            }
            if !seen.insert(node.clone()) || seen.len() > 32 {
                gap = Some("lineage_cycle_or_bound".into());
                break;
            }
            path.push(*rev);
            node = parent.clone();
        }
        if gap.is_some() {
            node = id.clone();
            path.clear()
        }
        used.extend(path.iter().copied());
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
            edge_versions: path,
        });
    }
    let _ = schema::FORMAT;
    Ok((members, used.into_iter().collect()))
}
