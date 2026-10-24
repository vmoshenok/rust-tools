//! Commit graph lane layout.
//!
//! Every row gets a node column and a list of segments. A segment is
//! `[x1, y1, x2, y2, color]` where x is a lane index and y is 0 (row top), 1 (node
//! middle) or 2 (row bottom). The frontend draws straight lines / curves from that.

use serde::Serialize;
use std::collections::HashMap;

pub struct LogCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub subject: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RefLabel {
    pub name: String,
    /// "branch" | "remote" | "tag" | "head"
    pub kind: String,
    pub current: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GraphRow {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub subject: String,
    pub refs: Vec<RefLabel>,
    pub col: u16,
    pub color: u16,
    pub segs: Vec<[u16; 5]>,
    pub head: bool,
    pub wip: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GraphData {
    pub rows: Vec<GraphRow>,
    pub max_lanes: u16,
}

#[derive(Clone)]
struct Lane {
    sha: String,
    color: u16,
}

const TOP: u16 = 0;
const MID: u16 = 1;
const BOTTOM: u16 = 2;

/// First free lane at index >= `min` (lanes below `min` are reserved for priority branches).
fn free_slot(lanes: &mut Vec<Option<Lane>>, min: usize) -> usize {
    ensure_len(lanes, min);
    match lanes.iter().skip(min).position(|l| l.is_none()) {
        Some(i) => min + i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

fn ensure_len(lanes: &mut Vec<Option<Lane>>, len: usize) {
    while lanes.len() < len {
        lanes.push(None);
    }
}

fn waiting_for(lanes: &[Option<Lane>], sha: &str) -> Option<usize> {
    lanes.iter().position(|l| l.as_ref().is_some_and(|l| l.sha == sha))
}

/// `prio_tips`: tip commits of prioritized branches, highest priority first. Each one claims
/// its first-parent chain, which is drawn in its own reserved lane (0, 1, 2, ...).
pub fn layout(
    commits: Vec<LogCommit>,
    labels: &mut HashMap<String, Vec<RefLabel>>,
    head: Option<&str>,
    prio_tips: &[String],
) -> GraphData {
    // Lane ownership for priority branches.
    let first_parent: HashMap<&str, Option<&str>> = commits
        .iter()
        .map(|c| (c.sha.as_str(), c.parents.first().map(String::as_str)))
        .collect();
    let mut owner: HashMap<String, usize> = HashMap::new();
    let mut reserved = 0usize;
    for tip in prio_tips {
        if !first_parent.contains_key(tip.as_str()) || owner.contains_key(tip) {
            continue; // not loaded, or part of a higher-priority chain
        }
        let lane = reserved;
        reserved += 1;
        let mut cur = Some(tip.as_str());
        while let Some(s) = cur {
            if owner.contains_key(s) || !first_parent.contains_key(s) {
                break;
            }
            owner.insert(s.to_string(), lane);
            cur = first_parent[s];
        }
    }
    // The WIP row sits in the lane of the commit it is based on.
    if let Some(wip) = commits.iter().find(|c| c.sha == "WIP") {
        if let Some(&k) = wip.parents.first().and_then(|p| owner.get(p)) {
            owner.insert("WIP".into(), k);
        }
    }

    // A reserved lane only takes commits once its branch tip has been drawn.
    let mut started = vec![false; reserved];
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut next_color: u16 = reserved as u16; // colors 0..reserved belong to priority lanes
    let mut max_lanes: usize = 1;
    let mut rows = Vec::with_capacity(commits.len());

    for c in commits {
        ensure_len(&mut lanes, reserved);
        let before = lanes.clone();
        let mut segs: Vec<[u16; 5]> = Vec::new();

        // Column: own reserved lane if it is free (or waiting for this commit), else the first
        // lane waiting for it, else a free slot.
        let existing = waiting_for(&lanes, &c.sha);
        let reserved_lane = owner.get(&c.sha).copied().filter(|&k| match &lanes[k] {
            None => true,
            Some(l) => l.sha == c.sha,
        });
        let col = match (reserved_lane, existing) {
            (Some(k), _) => k,
            (None, Some(i)) => i,
            (None, None) => free_slot(&mut lanes, reserved),
        };
        let color = match (reserved_lane, existing) {
            (Some(k), _) => k as u16,
            (None, Some(i)) => lanes[i].as_ref().unwrap().color,
            (None, None) => {
                next_color += 1;
                next_color - 1
            }
        };

        if reserved_lane.is_some() {
            started[col] = true;
        }
        let in_chain = reserved_lane.is_some();

        // Incoming lines from every lane that was waiting for this commit.
        for (i, l) in before.iter().enumerate() {
            if let Some(l) = l {
                if l.sha == c.sha {
                    segs.push([i as u16, TOP, col as u16, MID, l.color]);
                }
            }
        }

        // Lanes after this row: waiting lanes are consumed.
        let mut after: Vec<Option<Lane>> = lanes
            .iter()
            .map(|l| l.clone().filter(|l| l.sha != c.sha))
            .collect();
        ensure_len(&mut after, lanes.len().max(col + 1));

        for (pi, p) in c.parents.iter().enumerate() {
            // The first parent of a priority-chain commit always continues in its lane
            // (other lanes waiting for it converge there later).
            if pi == 0 && in_chain && owner.get(p) == Some(&col) {
                after[col] = Some(Lane { sha: p.clone(), color });
                segs.push([col as u16, MID, col as u16, BOTTOM, color]);
                continue;
            }
            let waiting = waiting_for(&after, p);
            // A parent owned by a started priority lane goes into that lane when it is free.
            let own = owner
                .get(p)
                .copied()
                .filter(|&k| waiting.is_none() && started[k] && after[k].is_none());
            match (waiting, own) {
                (Some(j), _) => {
                    // Parent already has a lane (another child): join it.
                    let lc = if pi == 0 { color } else { after[j].as_ref().unwrap().color };
                    segs.push([col as u16, MID, j as u16, BOTTOM, lc]);
                }
                (None, Some(k)) => {
                    after[k] = Some(Lane { sha: p.clone(), color: k as u16 });
                    let lc = if pi == 0 && k == col { color } else { k as u16 };
                    segs.push([col as u16, MID, k as u16, BOTTOM, lc]);
                }
                (None, None) if pi == 0 && (col >= reserved || owner.get(&c.sha) == Some(&col)) => {
                    after[col] = Some(Lane { sha: p.clone(), color });
                    segs.push([col as u16, MID, col as u16, BOTTOM, color]);
                }
                (None, None) => {
                    let j = free_slot(&mut after, reserved);
                    let nc = if pi == 0 { color } else { next_color };
                    if pi != 0 {
                        next_color += 1;
                    }
                    after[j] = Some(Lane { sha: p.clone(), color: nc });
                    segs.push([col as u16, MID, j as u16, BOTTOM, nc]);
                }
            }
        }

        // Lanes passing straight through this row.
        for (i, l) in before.iter().enumerate() {
            if let Some(l) = l {
                if l.sha != c.sha {
                    segs.push([i as u16, TOP, i as u16, BOTTOM, l.color]);
                }
            }
        }

        while after.len() > reserved && after.last().is_some_and(|l| l.is_none()) {
            after.pop();
        }
        max_lanes = max_lanes.max(after.len()).max(col + 1);
        lanes = after;

        let wip = c.sha == "WIP";
        rows.push(GraphRow {
            refs: labels.remove(&c.sha).unwrap_or_default(),
            head: head == Some(c.sha.as_str()),
            wip,
            sha: c.sha,
            parents: c.parents,
            author: c.author,
            email: c.email,
            time: c.time,
            subject: c.subject,
            col: col as u16,
            color,
            segs,
        });
    }

    GraphData { rows, max_lanes: max_lanes as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(sha: &str, parents: &[&str]) -> LogCommit {
        LogCommit {
            sha: sha.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            subject: String::new(),
        }
    }

    // m1 <- m2 <- m3 (master), m1 <- d1 <- d2 (develop), f1 feature off m2; newest first.
    fn history() -> Vec<LogCommit> {
        vec![
            c("f1", &["m2"]),
            c("d2", &["d1"]),
            c("m3", &["m2"]),
            c("d1", &["m1"]),
            c("m2", &["m1"]),
            c("m1", &[]),
        ]
    }

    fn cols(g: &GraphData) -> Vec<(String, u16)> {
        g.rows.iter().map(|r| (r.sha.clone(), r.col)).collect()
    }

    #[test]
    fn priorities_reserve_leftmost_lanes() {
        let g = layout(history(), &mut HashMap::new(), None, &["d2".into(), "m3".into()]);
        let col = |s: &str| g.rows.iter().find(|r| r.sha == s).unwrap().col;
        // develop (priority 1) owns lane 0 including the shared root m1
        assert_eq!((col("d2"), col("d1"), col("m1")), (0, 0, 0), "{:?}", cols(&g));
        // master (priority 2) owns lane 1
        assert_eq!((col("m3"), col("m2")), (1, 1), "{:?}", cols(&g));
        // unprioritized branch goes right of the reserved lanes
        assert!(col("f1") >= 2, "{:?}", cols(&g));
        // every row stays within max_lanes
        assert!(g.rows.iter().all(|r| r.segs.iter().all(|s| s[0] < g.max_lanes && s[2] < g.max_lanes)));
    }

    #[test]
    fn no_priorities_keeps_default_layout() {
        let g = layout(history(), &mut HashMap::new(), None, &[]);
        assert_eq!(g.rows[0].col, 0);
        assert!(g.rows.iter().all(|r| (r.col as u16) < g.max_lanes));
    }

    #[test]
    fn missing_priority_tip_reserves_nothing() {
        let g = layout(history(), &mut HashMap::new(), None, &["nope".into()]);
        assert_eq!(g.rows[0].col, 0);
    }
}
