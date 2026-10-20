//! Note domain model for the Phase 5 module slice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Actor, RichText};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Note {
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
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CreateNoteInput {
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "bodyV2", default)]
    pub body_v2: Option<RichText>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateNoteInput {
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "bodyV2", default)]
    pub body_v2: Option<RichText>,
}
