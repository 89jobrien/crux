//! TaskManager -- high-level project task management.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use crux_runtime::registry::RegistryBackend;
use crux_types::id::TaskId;
use crux_types::task::DependencyKind;

use crate::error::TaskErr;
use crate::types::{
    Dependency, ProjectTask, ProjectTaskStatus, TaskFilter, TaskPatch, TaskSpec, TaskStats,
};

/// High-level project task manager, generic over storage backend.
pub struct TaskManager<B: RegistryBackend> {
    backend: B,
}

impl<B: RegistryBackend> TaskManager<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub async fn add(&self, spec: TaskSpec) -> Result<TaskId, TaskErr> {
        let id = TaskId::new();
        let now = Utc::now();
        let task = ProjectTask {
            id: id.clone(),
            spec,
            created_at: now,
            updated_at: now,
        };
        let data = serde_json::to_vec(&task)?;
        self.backend
            .put(&id, data)
            .await
            .map_err(|e| TaskErr::Storage(e.to_string()))?;
        Ok(id)
    }

    pub async fn get(&self, id: &TaskId) -> Result<ProjectTask, TaskErr> {
        let data = self
            .backend
            .get(id)
            .await
            .map_err(|e| TaskErr::Storage(e.to_string()))?
            .ok_or_else(|| TaskErr::NotFound(id.to_string()))?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub async fn update(&self, id: &TaskId, patch: TaskPatch) -> Result<(), TaskErr> {
        let mut task = self.get(id).await?;
        if let Some(status) = patch.status {
            task.spec.status = status;
        }
        if let Some(priority) = patch.priority {
            task.spec.priority = priority;
        }
        if let Some(title) = patch.title {
            task.spec.title = title;
        }
        if let Some(description) = patch.description {
            task.spec.description = description;
        }
        for label in &patch.add_labels {
            if !task.spec.labels.contains(label) {
                task.spec.labels.push(label.clone());
            }
        }
        task.spec
            .labels
            .retain(|l| !patch.remove_labels.contains(l));
        for dep in patch.add_dependencies {
            if !task.spec.dependencies.contains(&dep) {
                task.spec.dependencies.push(dep);
            }
        }
        task.spec
            .dependencies
            .retain(|d| !patch.remove_dependencies.contains(&d.target));
        task.updated_at = Utc::now();
        let data = serde_json::to_vec(&task)?;
        self.backend
            .put(id, data)
            .await
            .map_err(|e| TaskErr::Storage(e.to_string()))?;
        Ok(())
    }

    pub async fn list(&self, filter: TaskFilter) -> Result<Vec<ProjectTask>, TaskErr> {
        let all = self.all_tasks().await?;
        Ok(all
            .into_iter()
            .filter(|t| {
                filter.status.as_ref().is_none_or(|s| &t.spec.status == s)
                    && filter
                        .priority
                        .as_ref()
                        .is_none_or(|p| &t.spec.priority == p)
                    && filter
                        .label
                        .as_ref()
                        .is_none_or(|l| t.spec.labels.contains(l))
            })
            .collect())
    }

    pub async fn ready(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        let all = self.all_tasks().await?;
        let mut result = Vec::new();
        for task in &all {
            if task.spec.status != ProjectTaskStatus::Open
                && task.spec.status != ProjectTaskStatus::InProgress
            {
                continue;
            }
            if task.spec.dependencies.is_empty() {
                result.push(task.clone());
                continue;
            }
            let all_resolved = task.spec.dependencies.iter().all(|dep| {
                all.iter()
                    .find(|t| t.id == dep.target)
                    .is_none_or(|t| t.spec.status == ProjectTaskStatus::Done)
            });
            if all_resolved {
                result.push(task.clone());
            }
        }
        Ok(result)
    }

    pub async fn blocked(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        let all = self.all_tasks().await?;
        let mut result = Vec::new();
        for task in &all {
            if task.spec.dependencies.is_empty() {
                continue;
            }
            let has_unresolved = task.spec.dependencies.iter().any(|dep| {
                all.iter()
                    .find(|t| t.id == dep.target)
                    .is_some_and(|t| t.spec.status != ProjectTaskStatus::Done)
            });
            if has_unresolved {
                result.push(task.clone());
            }
        }
        Ok(result)
    }

    pub async fn by_priority(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        let mut tasks = self.all_tasks().await?;
        tasks.sort_by_key(|t| t.spec.priority);
        Ok(tasks)
    }

    pub async fn block(&self, id: &TaskId, blocker: &TaskId) -> Result<(), TaskErr> {
        if id == blocker {
            return Err(TaskErr::CycleDetected);
        }
        self.check_cycle(id, blocker).await?;
        let dep = Dependency {
            target: blocker.clone(),
            kind: DependencyKind::BlockedBy,
        };
        self.update(
            id,
            TaskPatch {
                add_dependencies: vec![dep],
                ..Default::default()
            },
        )
        .await
    }

    pub async fn unblock(&self, id: &TaskId, blocker: &TaskId) -> Result<(), TaskErr> {
        self.update(
            id,
            TaskPatch {
                remove_dependencies: vec![blocker.clone()],
                ..Default::default()
            },
        )
        .await
    }

    pub async fn stats(&self) -> Result<TaskStats, TaskErr> {
        let tasks = self.all_tasks().await?;
        let mut by_status = HashMap::new();
        let mut by_priority = HashMap::new();
        for t in &tasks {
            *by_status.entry(t.spec.status.clone()).or_insert(0) += 1;
            *by_priority.entry(t.spec.priority).or_insert(0) += 1;
        }
        Ok(TaskStats {
            total: tasks.len(),
            by_status,
            by_priority,
        })
    }

    async fn check_cycle(&self, target: &TaskId, from: &TaskId) -> Result<(), TaskErr> {
        let mut visited = HashSet::new();
        let mut stack = vec![from.clone()];
        while let Some(current) = stack.pop() {
            if current == *target {
                return Err(TaskErr::CycleDetected);
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Ok(task) = self.get(&current).await {
                for dep in &task.spec.dependencies {
                    stack.push(dep.target.clone());
                }
            }
        }
        Ok(())
    }

    async fn all_tasks(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        let ids = self
            .backend
            .list("")
            .await
            .map_err(|e| TaskErr::Storage(e.to_string()))?;
        let mut tasks = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(data) = self
                .backend
                .get(id)
                .await
                .map_err(|e| TaskErr::Storage(e.to_string()))?
            {
                tasks.push(serde_json::from_slice(&data)?);
            }
        }
        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProjectTaskStatus;
    use crux_runtime::registry::InMemoryBackend;
    use crux_types::task::{Priority, TaskLabel};

    fn sample_spec() -> TaskSpec {
        TaskSpec {
            title: "Implement auth".into(),
            description: Some("Add JWT middleware".into()),
            priority: Priority::P1,
            status: ProjectTaskStatus::Open,
            labels: vec![TaskLabel("backend".into())],
            dependencies: vec![],
        }
    }

    fn make_manager() -> TaskManager<InMemoryBackend> {
        TaskManager::new(InMemoryBackend::new())
    }

    #[tokio::test]
    async fn add_and_get() {
        let mgr = make_manager();
        let id = mgr.add(sample_spec()).await.unwrap();
        let task = mgr.get(&id).await.unwrap();
        assert_eq!(task.spec.title, "Implement auth");
        assert_eq!(task.spec.priority, Priority::P1);
        assert_eq!(task.spec.status, ProjectTaskStatus::Open);
    }

    #[tokio::test]
    async fn get_missing_returns_not_found() {
        let mgr = make_manager();
        let id = TaskId::new();
        let err = mgr.get(&id).await.unwrap_err();
        assert!(matches!(err, TaskErr::NotFound(_)));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let _ = mgr.add(sample_spec()).await.unwrap();
        mgr.update(
            &id1,
            TaskPatch {
                status: Some(ProjectTaskStatus::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let open = mgr
            .list(TaskFilter {
                status: Some(ProjectTaskStatus::Open),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(open.len(), 1);
    }

    #[tokio::test]
    async fn list_filters_by_priority() {
        let mgr = make_manager();
        mgr.add(sample_spec()).await.unwrap(); // P1
        let mut p0_spec = sample_spec();
        p0_spec.priority = Priority::P0;
        mgr.add(p0_spec).await.unwrap();
        let p0_tasks = mgr
            .list(TaskFilter {
                priority: Some(Priority::P0),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(p0_tasks.len(), 1);
    }

    #[tokio::test]
    async fn list_filters_by_label() {
        let mgr = make_manager();
        mgr.add(sample_spec()).await.unwrap(); // "backend"
        let mut other = sample_spec();
        other.labels = vec![TaskLabel("frontend".into())];
        mgr.add(other).await.unwrap();
        let backend = mgr
            .list(TaskFilter {
                label: Some(TaskLabel("backend".into())),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(backend.len(), 1);
    }

    #[tokio::test]
    async fn update_changes_priority() {
        let mgr = make_manager();
        let id = mgr.add(sample_spec()).await.unwrap();
        mgr.update(
            &id,
            TaskPatch {
                priority: Some(Priority::P0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let task = mgr.get(&id).await.unwrap();
        assert_eq!(task.spec.priority, Priority::P0);
    }

    #[tokio::test]
    async fn update_adds_and_removes_labels() {
        let mgr = make_manager();
        let id = mgr.add(sample_spec()).await.unwrap();
        mgr.update(
            &id,
            TaskPatch {
                add_labels: vec![TaskLabel("urgent".into())],
                remove_labels: vec![TaskLabel("backend".into())],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let task = mgr.get(&id).await.unwrap();
        assert!(task.spec.labels.contains(&TaskLabel("urgent".into())));
        assert!(!task.spec.labels.contains(&TaskLabel("backend".into())));
    }

    #[tokio::test]
    async fn by_priority_sorts_p0_first() {
        let mgr = make_manager();
        let mut p3 = sample_spec();
        p3.priority = Priority::P3;
        mgr.add(p3).await.unwrap();
        let mut p0 = sample_spec();
        p0.priority = Priority::P0;
        mgr.add(p0).await.unwrap();
        let sorted = mgr.by_priority().await.unwrap();
        assert_eq!(sorted[0].spec.priority, Priority::P0);
        assert_eq!(sorted[1].spec.priority, Priority::P3);
    }

    #[tokio::test]
    async fn stats_counts() {
        let mgr = make_manager();
        mgr.add(sample_spec()).await.unwrap();
        let mut done_spec = sample_spec();
        done_spec.status = ProjectTaskStatus::Done;
        mgr.add(done_spec).await.unwrap();
        let s = mgr.stats().await.unwrap();
        assert_eq!(s.total, 2);
    }

    #[tokio::test]
    async fn block_adds_dependency() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        let task = mgr.get(&id2).await.unwrap();
        assert_eq!(task.spec.dependencies.len(), 1);
        assert_eq!(task.spec.dependencies[0].target, id1);
    }

    #[tokio::test]
    async fn unblock_removes_dependency() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        mgr.unblock(&id2, &id1).await.unwrap();
        let task = mgr.get(&id2).await.unwrap();
        assert!(task.spec.dependencies.is_empty());
    }

    #[tokio::test]
    async fn ready_returns_unblocked_open_tasks() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        let ready = mgr.ready().await.unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, id1);
    }

    #[tokio::test]
    async fn ready_includes_task_after_blocker_done() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        mgr.update(
            &id1,
            TaskPatch {
                status: Some(ProjectTaskStatus::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let ready = mgr.ready().await.unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, id2);
    }

    #[tokio::test]
    async fn blocked_returns_tasks_with_unresolved_deps() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        let blocked = mgr.blocked().await.unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].id, id2);
    }

    #[tokio::test]
    async fn cycle_detection_self_block() {
        let mgr = make_manager();
        let id = mgr.add(sample_spec()).await.unwrap();
        let err = mgr.block(&id, &id).await.unwrap_err();
        assert!(matches!(err, TaskErr::CycleDetected));
    }

    #[tokio::test]
    async fn cycle_detection_two_node() {
        let mgr = make_manager();
        let id1 = mgr.add(sample_spec()).await.unwrap();
        let id2 = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&id2, &id1).await.unwrap();
        let err = mgr.block(&id1, &id2).await.unwrap_err();
        assert!(matches!(err, TaskErr::CycleDetected));
    }

    #[tokio::test]
    async fn cycle_detection_three_node() {
        let mgr = make_manager();
        let a = mgr.add(sample_spec()).await.unwrap();
        let b = mgr.add(sample_spec()).await.unwrap();
        let c = mgr.add(sample_spec()).await.unwrap();
        mgr.block(&b, &a).await.unwrap();
        mgr.block(&c, &b).await.unwrap();
        let err = mgr.block(&a, &c).await.unwrap_err();
        assert!(matches!(err, TaskErr::CycleDetected));
    }
}

#[cfg(test)]
mod proptest_tasks {
    use super::*;
    use crux_runtime::registry::InMemoryBackend;
    use crux_types::task::Priority;
    use proptest::prelude::*;

    fn arb_priority() -> impl Strategy<Value = Priority> {
        prop_oneof![
            Just(Priority::P0),
            Just(Priority::P1),
            Just(Priority::P2),
            Just(Priority::P3),
        ]
    }

    fn arb_spec() -> impl Strategy<Value = TaskSpec> {
        ("[a-z]{3,20}", arb_priority()).prop_map(|(title, priority)| TaskSpec {
            title,
            description: None,
            priority,
            status: ProjectTaskStatus::Open,
            labels: vec![],
            dependencies: vec![],
        })
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    proptest! {
        #[test]
        fn task_without_deps_is_ready(spec in arb_spec()) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                let id = mgr.add(spec).await.unwrap();
                let ready = mgr.ready().await.unwrap();
                prop_assert!(ready.iter().any(|t| t.id == id));
                Ok(())
            })?;
        }

        #[test]
        fn self_block_always_fails(spec in arb_spec()) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                let id = mgr.add(spec).await.unwrap();
                let err = mgr.block(&id, &id).await.unwrap_err();
                prop_assert!(matches!(err, TaskErr::CycleDetected));
                Ok(())
            })?;
        }

        #[test]
        fn blocked_task_not_ready_unless_blocker_done(
            spec_a in arb_spec(),
            spec_b in arb_spec(),
        ) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                let a = mgr.add(spec_a).await.unwrap();
                let b = mgr.add(spec_b).await.unwrap();
                mgr.block(&b, &a).await.unwrap();

                let ready = mgr.ready().await.unwrap();
                prop_assert!(!ready.iter().any(|t| t.id == b));

                mgr.update(&a, TaskPatch {
                    status: Some(ProjectTaskStatus::Done),
                    ..Default::default()
                }).await.unwrap();
                let ready = mgr.ready().await.unwrap();
                prop_assert!(ready.iter().any(|t| t.id == b));

                Ok(())
            })?;
        }

        #[test]
        fn by_priority_is_sorted(
            specs in prop::collection::vec(arb_spec(), 2..10),
        ) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                for spec in specs {
                    mgr.add(spec).await.unwrap();
                }
                let sorted = mgr.by_priority().await.unwrap();
                for window in sorted.windows(2) {
                    prop_assert!(window[0].spec.priority <= window[1].spec.priority);
                }
                Ok(())
            })?;
        }

        #[test]
        fn stats_total_matches_count(
            specs in prop::collection::vec(arb_spec(), 0..20),
        ) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                let count = specs.len();
                for spec in specs {
                    mgr.add(spec).await.unwrap();
                }
                let stats = mgr.stats().await.unwrap();
                prop_assert_eq!(stats.total, count);
                Ok(())
            })?;
        }

        #[test]
        fn stats_by_status_sums_to_total(
            specs in prop::collection::vec(arb_spec(), 1..15),
        ) {
            let rt = rt();
            rt.block_on(async {
                let mgr = TaskManager::new(InMemoryBackend::new());
                for spec in specs {
                    mgr.add(spec).await.unwrap();
                }
                let stats = mgr.stats().await.unwrap();
                let sum: usize = stats.by_status.values().sum();
                prop_assert_eq!(sum, stats.total);
                Ok(())
            })?;
        }
    }
}
