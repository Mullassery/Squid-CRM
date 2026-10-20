//! Task domain model for the Phase 5 module slice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Actor, MaybeUndefined, RichText};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, async_graphql::Enum,
)]
#[graphql(name = "TaskStatusEnum")]
pub enum TaskStatus {
    #[default]
    #[serde(rename = "TODO")]
    Todo,
    #[serde(rename = "IN_PROGRESS")]
    InProgress,
    #[serde(rename = "DONE")]
    Done,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "TODO",
            Self::InProgress => "IN_PROGRESS",
            Self::Done => "DONE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "TODO" => Some(Self::Todo),
            "IN_PROGRESS" => Some(Self::InProgress),
            "DONE" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub title: Option<String>,
    #[serde(rename = "bodyV2")]
    pub body_v2: RichText,
    #[serde(rename = "dueAt")]
    pub due_at: Option<DateTime<Utc>>,
    pub status: Option<TaskStatus>,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "assigneeId")]
    pub assignee_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CreateTaskInput {
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "bodyV2", default)]
    pub body_v2: Option<RichText>,
    #[serde(rename = "dueAt", default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(rename = "assigneeId", default)]
    pub assignee_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateTaskInput {
    #[serde(default)]
    pub position: MaybeUndefined<f64>,
    #[serde(default)]
    pub title: MaybeUndefined<String>,
    #[serde(rename = "bodyV2", default)]
    pub body_v2: MaybeUndefined<RichText>,
    #[serde(rename = "dueAt", default)]
    pub due_at: MaybeUndefined<DateTime<Utc>>,
    #[serde(default)]
    pub status: MaybeUndefined<TaskStatus>,
    #[serde(rename = "assigneeId", default)]
    pub assignee_id: MaybeUndefined<Uuid>,
}
