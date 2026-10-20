//! Opportunity domain model for the Phase 5 module slice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Actor, Currency};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, async_graphql::Enum,
)]
#[graphql(name = "OpportunityStageEnum")]
pub enum OpportunityStage {
    #[default]
    #[serde(rename = "NEW")]
    New,
    #[serde(rename = "SCREENING")]
    Screening,
    #[serde(rename = "MEETING")]
    Meeting,
    #[serde(rename = "PROPOSAL")]
    Proposal,
    #[serde(rename = "CUSTOMER")]
    Customer,
}

impl OpportunityStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::Screening => "SCREENING",
            Self::Meeting => "MEETING",
            Self::Proposal => "PROPOSAL",
            Self::Customer => "CUSTOMER",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "NEW" => Some(Self::New),
            "SCREENING" => Some(Self::Screening),
            "MEETING" => Some(Self::Meeting),
            "PROPOSAL" => Some(Self::Proposal),
            "CUSTOMER" => Some(Self::Customer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Opportunity {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: Option<String>,
    pub amount: Currency,
    #[serde(rename = "closeDate")]
    pub close_date: Option<DateTime<Utc>>,
    pub stage: OpportunityStage,
    pub position: f64,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "companyId")]
    pub company_id: Option<Uuid>,
    #[serde(rename = "pointOfContactId")]
    pub point_of_contact_id: Option<Uuid>,
    #[serde(rename = "ownerId")]
    pub owner_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CreateOpportunityInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub amount: Option<Currency>,
    #[serde(rename = "closeDate", default)]
    pub close_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stage: Option<OpportunityStage>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(rename = "companyId", default)]
    pub company_id: Option<Uuid>,
    #[serde(rename = "pointOfContactId", default)]
    pub point_of_contact_id: Option<Uuid>,
    #[serde(rename = "ownerId", default)]
    pub owner_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateOpportunityInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub amount: Option<Currency>,
    #[serde(rename = "closeDate", default)]
    pub close_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stage: Option<OpportunityStage>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(rename = "companyId", default)]
    pub company_id: Option<Uuid>,
    #[serde(rename = "pointOfContactId", default)]
    pub point_of_contact_id: Option<Uuid>,
    #[serde(rename = "ownerId", default)]
    pub owner_id: Option<Uuid>,
}

pub fn opportunity_search_vector(name: Option<&str>) -> Option<String> {
    name.map(|name| name.to_lowercase())
}
