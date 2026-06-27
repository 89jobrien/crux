//! DAG resolution for Cruxfile targets.
//!
//! Builds a dependency graph from `CruxfileDef`, detects cycles, validates
//! dependency references, and produces a topologically sorted execution plan.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::schema::CruxfileDef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    UnknownTarget(String),
    CycleDetected(Vec<String>),
    UnknownDependency { target: String, dependency: String },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::UnknownTarget(t) => write!(f, "unknown target: {t}"),
            ResolveError::CycleDetected(cycle) => {
                write!(f, "dependency cycle detected: {}", cycle.join(" -> "))
            }
            ResolveError::UnknownDependency { target, dependency } => {
                write!(
                    f,
                    "target '{target}' depends on unknown target '{dependency}'"
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolves Cruxfile target dependencies into an execution plan.
#[derive(Debug)]
pub struct TargetResolver {
    /// target -> list of targets it depends on
    edges: HashMap<String, Vec<String>>,
}

impl TargetResolver {
    /// Build a resolver from a Cruxfile definition.
    ///
    /// Validates that all `depends` references point to existing targets and
    /// that no cycles exist.
    pub fn new(cruxfile: &CruxfileDef) -> Result<Self, ResolveError> {
        let target_names: HashSet<&str> = cruxfile.targets.keys().map(String::as_str).collect();

        let mut edges: HashMap<String, Vec<String>> = HashMap::new();

        for (name, target) in &cruxfile.targets {
            for dep in &target.depends {
                if !target_names.contains(dep.as_str()) {
                    return Err(ResolveError::UnknownDependency {
                        target: name.clone(),
                        dependency: dep.clone(),
                    });
                }
            }
            edges.insert(name.clone(), target.depends.clone());
        }

        let resolver = Self { edges };
        resolver.check_cycles()?;
        Ok(resolver)
    }

    /// Return the topologically sorted execution order for a target,
    /// including all transitive dependencies. The requested target is last.
    pub fn execution_order<'a>(&'a self, target: &'a str) -> Result<Vec<&'a str>, ResolveError> {
        if !self.edges.contains_key(target) {
            return Err(ResolveError::UnknownTarget(target.to_string()));
        }

        // BFS to collect all reachable targets.
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(target);
        visited.insert(target);

        while let Some(t) = queue.pop_front() {
            if let Some(deps) = self.edges.get(t) {
                for dep in deps {
                    if visited.insert(dep.as_str()) {
                        queue.push_back(dep.as_str());
                    }
                }
            }
        }

        // Kahn's algorithm on the subgraph.
        // in_deg[t] = number of deps of t that are in the subgraph.
        let mut in_deg: HashMap<&str, usize> = HashMap::new();
        for &t in &visited {
            let deps = self.edges.get(t).map(|v| v.as_slice()).unwrap_or(&[]);
            let count = deps.iter().filter(|d| visited.contains(d.as_str())).count();
            in_deg.insert(t, count);
        }

        let mut result = Vec::new();
        let mut ready: VecDeque<&str> = in_deg
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&t, _)| t)
            .collect();
        // Sort for deterministic output.
        let mut ready_vec: Vec<&str> = ready.drain(..).collect();
        ready_vec.sort();
        ready = ready_vec.into_iter().collect();

        while let Some(t) = ready.pop_front() {
            result.push(t);
            // Find all nodes in subgraph that depend on t.
            for &node in &visited {
                if let Some(deps) = self.edges.get(node)
                    && deps.iter().any(|d| d == t)
                    && let Some(deg) = in_deg.get_mut(node)
                {
                    *deg -= 1;
                    if *deg == 0 {
                        let pos = ready.iter().position(|&r| r > node).unwrap_or(ready.len());
                        ready.insert(pos, node);
                    }
                }
            }
        }

        Ok(result)
    }

    /// Check for cycles using DFS with 3-color marking.
    fn check_cycles(&self) -> Result<(), ResolveError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        let mut color: HashMap<&str, Color> = self
            .edges
            .keys()
            .map(|k| (k.as_str(), Color::White))
            .collect();
        let mut path: Vec<&str> = Vec::new();

        fn dfs<'a>(
            node: &'a str,
            edges: &'a HashMap<String, Vec<String>>,
            color: &mut HashMap<&'a str, Color>,
            path: &mut Vec<&'a str>,
        ) -> Result<(), ResolveError> {
            color.insert(node, Color::Gray);
            path.push(node);

            if let Some(deps) = edges.get(node) {
                for dep in deps {
                    match color.get(dep.as_str()) {
                        Some(Color::Gray) => {
                            // Found a cycle. Build the cycle path.
                            let start = path
                                .iter()
                                .position(|&n| n == dep)
                                .expect("dep must be in path when Gray");
                            let mut cycle: Vec<String> =
                                path[start..].iter().map(|s| s.to_string()).collect();
                            cycle.push(dep.clone());
                            return Err(ResolveError::CycleDetected(cycle));
                        }
                        Some(Color::White) | None => {
                            dfs(dep.as_str(), edges, color, path)?;
                        }
                        Some(Color::Black) => {}
                    }
                }
            }

            path.pop();
            color.insert(node, Color::Black);
            Ok(())
        }

        let keys: Vec<&str> = self.edges.keys().map(String::as_str).collect();
        for &node in &keys {
            if color.get(node) == Some(&Color::White) {
                dfs(node, &self.edges, &mut color, &mut path)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CruxfileDef, TargetDef};
    use indexmap::IndexMap;

    fn make_cruxfile(targets: Vec<(&str, Vec<&str>)>) -> CruxfileDef {
        let mut map = IndexMap::new();
        for (name, deps) in targets {
            map.insert(
                name.to_string(),
                TargetDef {
                    depends: deps.into_iter().map(String::from).collect(),
                    budget: None,
                    steps: vec![],
                },
            );
        }
        CruxfileDef {
            project: "test".to_string(),
            default: map.keys().next().unwrap().clone(),
            budget: None,
            targets: map,
        }
    }

    #[test]
    fn single_target_no_deps() {
        let cf = make_cruxfile(vec![("lint", vec![])]);
        let resolver = TargetResolver::new(&cf).unwrap();
        let order = resolver.execution_order("lint").unwrap();
        assert_eq!(order, vec!["lint"]);
    }

    #[test]
    fn linear_chain() {
        let cf = make_cruxfile(vec![("a", vec!["b"]), ("b", vec!["c"]), ("c", vec![])]);
        let resolver = TargetResolver::new(&cf).unwrap();
        let order = resolver.execution_order("a").unwrap();
        assert_eq!(order, vec!["c", "b", "a"]);
    }

    #[test]
    fn diamond_deps() {
        let cf = make_cruxfile(vec![
            ("a", vec!["b", "c"]),
            ("b", vec!["d"]),
            ("c", vec!["d"]),
            ("d", vec![]),
        ]);
        let resolver = TargetResolver::new(&cf).unwrap();
        let order = resolver.execution_order("a").unwrap();
        // d must come first, then b and c (alphabetical), then a
        assert_eq!(order, vec!["d", "b", "c", "a"]);
    }

    #[test]
    fn cycle_detected() {
        let cf = make_cruxfile(vec![("a", vec!["b"]), ("b", vec!["a"])]);
        let err = TargetResolver::new(&cf).unwrap_err();
        assert!(matches!(err, ResolveError::CycleDetected(_)));
    }

    #[test]
    fn unknown_target() {
        let cf = make_cruxfile(vec![("lint", vec![])]);
        let resolver = TargetResolver::new(&cf).unwrap();
        let err = resolver.execution_order("nonexistent").unwrap_err();
        assert!(matches!(err, ResolveError::UnknownTarget(_)));
    }

    #[test]
    fn unknown_dependency() {
        let cf = make_cruxfile(vec![("a", vec!["missing"])]);
        let err = TargetResolver::new(&cf).unwrap_err();
        assert!(matches!(err, ResolveError::UnknownDependency { .. }));
    }

    #[test]
    fn default_target_resolves() {
        let cf = make_cruxfile(vec![
            ("ci", vec!["lint", "test"]),
            ("lint", vec![]),
            ("test", vec!["lint"]),
        ]);
        let resolver = TargetResolver::new(&cf).unwrap();
        let order = resolver.execution_order(&cf.default).unwrap();
        assert_eq!(order[0], "lint");
        assert_eq!(*order.last().unwrap(), "ci");
    }
}
