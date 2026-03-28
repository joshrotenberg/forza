//! Plan helpers shared between the CLI and the REST API.
//!
//! Provides DAG parsing and topological sorting for plan issues, as well as
//! prompt-building utilities for plan creation.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::config::{Route, SubjectType};
use crate::github::IssueCandidate;

/// Parse the mermaid dependency graph from a plan issue body.
///
/// Expects a mermaid block like:
/// ```text
/// graph TD
///     401["#401 CI workflow"] --> 403["#403 auto-fix-ci"]
///     402["#402 auto-rebase"]
/// ```
///
/// Returns a map of issue_number -> Vec<dependency_numbers>.
/// An issue with no dependencies has an empty vec.
pub fn parse_plan_dag(body: &str) -> Result<HashMap<u64, Vec<u64>>, String> {
    let mut dag: HashMap<u64, Vec<u64>> = HashMap::new();

    // Find the mermaid block.
    let mermaid_start = body
        .find("```mermaid")
        .ok_or("no mermaid dependency graph found in plan issue")?;
    let mermaid_body = &body[mermaid_start + "```mermaid".len()..];
    let mermaid_end = mermaid_body
        .find("```")
        .ok_or("unterminated mermaid block")?;
    let mermaid = &mermaid_body[..mermaid_end];

    for line in mermaid.lines() {
        let line = line.trim();

        // Parse edges: 401["..."] --> 403["..."]
        if line.contains("-->") {
            let parts: Vec<&str> = line.split("-->").collect();
            if parts.len() == 2
                && let (Some(from), Some(to)) =
                    (extract_node_id(parts[0]), extract_node_id(parts[1]))
            {
                dag.entry(to).or_default().push(from);
                dag.entry(from).or_default();
            }
        } else if line.contains('[')
            && let Some(id) = extract_node_id(line)
        {
            dag.entry(id).or_default();
        }
    }

    Ok(dag)
}

/// Extract the numeric node ID from a mermaid node like `401["#401 CI workflow"]`.
pub fn extract_node_id(s: &str) -> Option<u64> {
    let s = s.trim();
    // The node ID is the number before the bracket.
    let id_part = if let Some(bracket) = s.find('[') {
        &s[..bracket]
    } else {
        s
    };
    id_part.trim().parse().ok()
}

/// Group the DAG into parallel execution levels using Kahn's algorithm.
///
/// Level 0 contains root issues (no dependencies). Level N contains issues whose
/// dependencies are all satisfied by levels 0..N-1. Issues within the same level
/// have no dependencies on each other and can run concurrently.
///
/// DAG format: `dag[node] = [dependencies]` — the issues that `node` depends on.
pub fn topological_levels(dag: &HashMap<u64, Vec<u64>>) -> Result<Vec<Vec<u64>>, String> {
    let mut in_degree: HashMap<u64, usize> =
        dag.iter().map(|(node, deps)| (*node, deps.len())).collect();

    for deps in dag.values() {
        for dep in deps {
            in_degree.entry(*dep).or_insert(0);
        }
    }

    let mut levels: Vec<Vec<u64>> = Vec::new();
    let mut resolved: std::collections::HashSet<u64> = std::collections::HashSet::new();

    loop {
        let mut level: Vec<u64> = in_degree
            .iter()
            .filter(|(node, deg)| **deg == 0 && !resolved.contains(*node))
            .map(|(node, _)| *node)
            .collect();

        if level.is_empty() {
            break;
        }

        level.sort();
        for &node in &level {
            resolved.insert(node);
        }

        // Decrement in-degree for dependents of nodes in this level.
        for &node in &level {
            for (dependent, deps) in dag {
                if deps.contains(&node)
                    && let Some(deg) = in_degree.get_mut(dependent)
                {
                    *deg = deg.saturating_sub(1);
                }
            }
        }

        levels.push(level);
    }

    let total: usize = levels.iter().map(|l| l.len()).sum();
    if total != in_degree.len() {
        return Err("circular dependency detected in plan".to_string());
    }

    Ok(levels)
}

/// Topological sort of the dependency DAG. Returns issue numbers in execution order.
///
/// DAG format: `dag[node] = [dependencies]` — the issues that `node` depends on.
/// Kahn's algorithm: process nodes with zero unresolved dependencies first.
pub fn topological_sort(dag: &HashMap<u64, Vec<u64>>) -> Result<Vec<u64>, String> {
    // in_degree[node] = number of unprocessed dependencies.
    let mut in_degree: HashMap<u64, usize> =
        dag.iter().map(|(node, deps)| (*node, deps.len())).collect();

    // Ensure dependency-only nodes (not keys in dag) are tracked.
    for deps in dag.values() {
        for dep in deps {
            in_degree.entry(*dep).or_insert(0);
        }
    }

    // Seed the queue with nodes that have no dependencies.
    let mut ready: Vec<u64> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(node, _)| *node)
        .collect();
    ready.sort(); // Deterministic order.

    let mut queue = std::collections::VecDeque::from(ready);
    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node);
        // Decrement in-degree for all nodes that depend on this one.
        // Collect newly-ready nodes and sort for deterministic order.
        let mut newly_ready = Vec::new();
        for (dependent, deps) in dag {
            if deps.contains(&node)
                && let Some(deg) = in_degree.get_mut(dependent)
            {
                *deg = deg.saturating_sub(1);
                if *deg == 0 {
                    newly_ready.push(*dependent);
                }
            }
        }
        newly_ready.sort();
        queue.extend(newly_ready);
    }

    if result.len() != in_degree.len() {
        return Err("circular dependency detected in plan".to_string());
    }

    Ok(result)
}

/// Build a route summary string for the plan prompt.
pub fn build_route_summary(routes: &IndexMap<String, Route>) -> String {
    routes
        .iter()
        .filter(|(_, r)| r.route_type == SubjectType::Issue)
        .map(|(name, r)| {
            let label = r.label.as_deref().unwrap_or("(none)");
            let workflow = r.workflow.as_deref().unwrap_or("(default)");
            format!("- **{name}**: label=`{label}`, workflow=`{workflow}`")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build issue summaries for the plan prompt.
pub fn build_issue_summaries(issues: &[IssueCandidate]) -> String {
    issues
        .iter()
        .map(|i| {
            let labels = if i.labels.is_empty() {
                "(none)".to_string()
            } else {
                i.labels.join(", ")
            };
            let body = if i.body.len() > 500 {
                format!("{}...", &i.body[..500])
            } else {
                i.body.clone()
            };
            format!(
                "### Issue #{}: {}\n\n**Labels**: {}\n\n{}\n",
                i.number, i.title, labels, body
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n\n")
}

/// Build a short issue reference string for the plan issue title.
///
/// Under 6 issues: explicit list (`#42, #45, #48`).
/// 6+: compact ranges with gaps (`#69..#73, #75, #77..#79`).
pub fn build_issue_refs(issues: &[IssueCandidate]) -> String {
    let mut numbers: Vec<u64> = issues.iter().map(|i| i.number).collect();
    numbers.sort();

    if numbers.len() < 6 {
        return numbers
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ");
    }

    compact_ranges(&numbers)
}

pub fn compact_ranges(numbers: &[u64]) -> String {
    if numbers.is_empty() {
        return String::new();
    }

    let mut ranges: Vec<String> = Vec::new();
    let mut start = numbers[0];
    let mut end = numbers[0];

    for &n in &numbers[1..] {
        if n == end + 1 {
            end = n;
        } else {
            ranges.push(format_range(start, end));
            start = n;
            end = n;
        }
    }
    ranges.push(format_range(start, end));
    ranges.join(", ")
}

fn format_range(start: u64, end: u64) -> String {
    if start == end {
        format!("#{start}")
    } else {
        format!("#{start}..#{end}")
    }
}
