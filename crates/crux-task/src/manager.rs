//! TaskManager -- high-level project task management.

use std::collections::HashMap;

use chrono::Utc;
use crux_runtime::registry::RegistryBackend;
use crux_types::id::TaskId;

use crate::error::TaskErr;
use crate::types::{ProjectTask, TaskFilter, TaskPatch, TaskSpec, TaskStats};

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
        todo!()
    }

    pub async fn blocked(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        todo!()
    }

    pub async fn by_priority(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        let mut tasks = self.all_tasks().await?;
        tasks.sort_by_key(|t| t.spec.priority);
        Ok(tasks)
    }

    pub async fn block(&self, _id: &TaskId, _blocker: &TaskId) -> Result<(), TaskErr> {
        todo!()
    }

    pub async fn unblock(&self, _id: &TaskId, _blocker: &TaskId) -> Result<(), TaskErr> {
        todo!()
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
}
