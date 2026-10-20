//! Polymorphic task/note target relations for the Phase 5 module slice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Actor;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct TaskTarget {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "taskId")]
    pub task_id: Uuid,
    #[serde(rename = "targetCompanyId")]
    pub target_company_id: Option<Uuid>,
    #[serde(rename = "targetOpportunityId")]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(rename = "targetPersonId")]
    pub target_person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct NoteTarget {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "noteId")]
    pub note_id: Uuid,
    #[serde(rename = "targetCompanyId")]
    pub target_company_id: Option<Uuid>,
    #[serde(rename = "targetOpportunityId")]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(rename = "targetPersonId")]
    pub target_person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskTargetInput {
    pub task_id: Uuid,
    #[serde(default)]
    pub target_company_id: Option<Uuid>,
    #[serde(default)]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(default)]
    pub target_person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNoteTargetInput {
    pub note_id: Uuid,
    #[serde(default)]
    pub target_company_id: Option<Uuid>,
    #[serde(default)]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(default)]
    pub target_person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskTargetInput {
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub target_company_id: Option<Uuid>,
    #[serde(default)]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(default)]
    pub target_person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNoteTargetInput {
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub target_company_id: Option<Uuid>,
    #[serde(default)]
    pub target_opportunity_id: Option<Uuid>,
    #[serde(default)]
    pub target_person_id: Option<Uuid>,
}
