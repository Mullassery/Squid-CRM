//! Company domain models for the Phase 5 module slice.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Actor, Links, MaybeUndefined};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    #[serde(default)]
    pub address_street_1: Option<String>,
    #[serde(default)]
    pub address_street_2: Option<String>,
    #[serde(default)]
    pub address_city: Option<String>,
    #[serde(default)]
    pub address_postcode: Option<String>,
    #[serde(default)]
    pub address_state: Option<String>,
    #[serde(default)]
    pub address_country: Option<String>,
    #[serde(default)]
    pub address_lat: Option<f64>,
    #[serde(default)]
    pub address_lng: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Currency {
    #[serde(default)]
    pub amount_micros: Option<i64>,
    #[serde(default)]
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub name: Option<String>,
    #[serde(rename = "domainName")]
    pub domain_name: Links,
    pub address: Address,
    #[serde(rename = "linkedinLink")]
    pub linkedin_link: Links,
    #[serde(rename = "annualRevenue")]
    pub annual_revenue: Currency,
    pub position: f64,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "accountOwnerId")]
    pub account_owner_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CreateCompanyInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "domainName", default)]
    pub domain_name: Option<Links>,
    #[serde(default)]
    pub address: Option<Address>,
    #[serde(rename = "linkedinLink", default)]
    pub linkedin_link: Option<Links>,
    #[serde(rename = "annualRevenue", default)]
    pub annual_revenue: Option<Currency>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(rename = "accountOwnerId", default)]
    pub account_owner_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateCompanyInput {
    #[serde(default)]
    pub name: MaybeUndefined<String>,
    #[serde(rename = "domainName", default)]
    pub domain_name: MaybeUndefined<Links>,
    #[serde(default)]
    pub address: MaybeUndefined<Address>,
    #[serde(rename = "linkedinLink", default)]
    pub linkedin_link: MaybeUndefined<Links>,
    #[serde(rename = "annualRevenue", default)]
    pub annual_revenue: MaybeUndefined<Currency>,
    #[serde(default)]
    pub position: MaybeUndefined<f64>,
    #[serde(rename = "accountOwnerId", default)]
    pub account_owner_id: MaybeUndefined<Uuid>,
}

pub fn company_search_vector(name: Option<&str>, domain: &Links) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(name) = name {
        parts.extend(name.split_whitespace().map(str::to_lowercase));
    }
    if !domain.primary_link_url.is_empty() {
        parts.push(domain.primary_link_url.to_lowercase());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

pub fn company_json(value: &impl Serialize) -> serde_json::Result<Value> {
    serde_json::to_value(value)
}
