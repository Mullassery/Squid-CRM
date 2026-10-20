//! GraphQL shapes for the frontend `UserQueryFragment` boot query.
//!
//! This slice returns live identity/workspace data plus static defaults for
//! enterprise surfaces (roles, billing, SSO, permissions). Unknown/absent
//! optional data is `null` or `[]`; no billing or SSO capability is claimed.

use async_graphql::{Json, SimpleObject};
use serde_json::Value;
use squid_core::{
    Actor, FullName, Role, User as CoreUser, Workspace as CoreWorkspace, WorkspaceMember,
};
use uuid::Uuid;

const NIL_UUID: Uuid = Uuid::nil();

#[derive(Debug, Clone, SimpleObject)]
pub struct User {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub is_email_verified: bool,
    pub disabled: Option<bool>,
    pub can_impersonate: bool,
    pub can_access_full_admin_panel: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub locale: String,
    pub workspace_member: Option<WorkspaceMemberGql>,
    pub user_workspaces: Vec<UserWorkspace>,
    pub onboarding_status: Option<String>,
    pub previous_onboarding_status: Option<String>,
    pub current_workspace: Option<Workspace>,
    pub current_user_workspace: Option<UserWorkspace>,
    pub user_vars: Option<Json<Value>>,
    pub workspace_members: Option<Vec<WorkspaceMemberGql>>,
    pub deleted_workspace_members: Option<Vec<DeletedWorkspaceMember>>,
    pub has_password: bool,
    pub support_user_hash: Option<String>,
    pub is_workspace_creator: Option<bool>,
    pub workspaces: Vec<UserWorkspace>,
    pub available_workspaces: AvailableWorkspaces,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "WorkspaceMember")]
pub struct WorkspaceMemberGql {
    pub id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub position: f64,
    pub name: FullName,
    pub user_email: String,
    pub color_scheme: String,
    pub ui_scale: String,
    pub open_record_in: String,
    pub avatar_url: Option<String>,
    pub locale: Option<String>,
    pub calendar_start_day: Option<i32>,
    pub time_zone: Option<String>,
    pub date_format: Option<String>,
    pub time_format: Option<String>,
    pub roles: Option<Vec<RoleGql>>,
    pub user_id: Uuid,
    pub user_workspace_id: Option<Uuid>,
    pub number_format: Option<String>,
    pub job_title: Option<String>,
    pub search_vector: Option<String>,
    pub created_by: Actor,
    pub updated_by: Actor,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct DeletedWorkspaceMember {
    pub id: Uuid,
    pub name: FullName,
    pub user_email: String,
    pub avatar_url: Option<String>,
    pub user_workspace_id: Option<Uuid>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct UserWorkspace {
    pub id: Uuid,
    pub user: Option<Box<User>>,
    pub user_id: Uuid,
    pub locale: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub permission_flags: Option<Vec<String>>,
    pub object_permissions: Option<Vec<ObjectPermission>>,
    pub objects_permissions: Option<Vec<ObjectPermission>>,
    pub two_factor_authentication_method_summary: Option<Vec<TwoFactorAuthenticationMethodSummary>>,
    pub is_impersonating: Option<bool>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct TwoFactorAuthenticationMethodSummary {
    pub two_factor_authentication_method_id: Uuid,
    pub status: String,
    pub strategy: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct ObjectPermission {
    pub object_metadata_id: Uuid,
    pub can_read_object_records: Option<bool>,
    pub can_update_object_records: Option<bool>,
    pub can_soft_delete_object_records: Option<bool>,
    pub can_destroy_object_records: Option<bool>,
    pub restricted_fields: Option<Json<Value>>,
    pub row_level_permission_predicates: Option<Vec<RowLevelPermissionPredicate>>,
    pub row_level_permission_predicate_groups: Option<Vec<RowLevelPermissionPredicateGroup>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct RowLevelPermissionPredicate {
    pub id: Uuid,
    pub field_metadata_id: Uuid,
    pub object_metadata_id: Uuid,
    pub operand: Option<String>,
    pub sub_field_name: Option<String>,
    pub workspace_member_field_metadata_id: Option<Uuid>,
    pub workspace_member_sub_field_name: Option<String>,
    pub row_level_permission_predicate_group_id: Option<Uuid>,
    pub position_in_row_level_permission_predicate_group: Option<f64>,
    pub role_id: Uuid,
    pub value: Option<Json<Value>>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct RowLevelPermissionPredicateGroup {
    pub id: Uuid,
    pub parent_row_level_permission_predicate_group_id: Option<Uuid>,
    pub logical_operator: Option<String>,
    pub position_in_row_level_permission_predicate_group: Option<f64>,
    pub role_id: Uuid,
    pub object_metadata_id: Uuid,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Role")]
pub struct RoleGql {
    pub id: Uuid,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub can_update_all_settings: bool,
    pub can_access_all_tools: bool,
    pub is_editable: bool,
    pub can_read_all_object_records: bool,
    pub can_update_all_object_records: bool,
    pub can_soft_delete_all_object_records: bool,
    pub can_destroy_all_object_records: bool,
    pub can_be_assigned_to_users: bool,
    pub can_be_assigned_to_agents: bool,
    pub can_be_assigned_to_api_keys: bool,
    pub application_id: Uuid,
    pub universal_identifier: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct WorkspaceUrls {
    pub custom_url: Option<String>,
    pub subdomain_url: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct AvailableWorkspaces {
    pub available_workspaces_for_sign_in: Vec<AvailableWorkspace>,
    pub available_workspaces_for_sign_up: Vec<AvailableWorkspace>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct AvailableWorkspace {
    pub id: Uuid,
    pub display_name: Option<String>,
    pub login_token: Option<String>,
    pub invite_hash: Option<String>,
    pub personal_invite_token: Option<String>,
    pub workspace_urls: WorkspaceUrls,
    pub logo: Option<String>,
    pub sso: Vec<SsoConnection>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SSOConnection")]
pub struct SsoConnection {
    #[graphql(name = "type")]
    pub connection_type: String,
    pub id: Uuid,
    pub issuer: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct Workspace {
    pub id: Uuid,
    pub display_name: Option<String>,
    pub logo: Option<String>,
    pub invite_hash: Option<String>,
    pub allow_impersonation: bool,
    pub activation_status: String,
    pub is_public_invite_link_enabled: bool,
    pub is_campaign_click_tracking_enabled: bool,
    pub is_campaign_open_tracking_enabled: bool,
    pub workspace_discoverability: String,
    pub is_google_auth_enabled: bool,
    pub is_microsoft_auth_enabled: bool,
    pub is_password_auth_enabled: bool,
    pub is_google_auth_bypass_enabled: bool,
    pub is_microsoft_auth_bypass_enabled: bool,
    pub is_password_auth_bypass_enabled: bool,
    pub subdomain: String,
    pub custom_domain: Option<String>,
    pub has_valid_signed_enterprise_key: bool,
    pub has_valid_enterprise_validity_token: bool,
    pub workspace_custom_application: Option<Application>,
    pub installed_applications: Vec<Application>,
    pub is_custom_domain_enabled: bool,
    pub workspace_urls: WorkspaceUrls,
    pub feature_flags: Option<Vec<FeatureFlag>>,
    pub current_billing_subscription: Option<BillingSubscription>,
    pub billing_customer: Option<BillingCustomer>,
    pub billing_subscriptions: Vec<BillingSubscription>,
    pub billing_entitlements: Vec<BillingEntitlement>,
    pub workspace_members_count: Option<f64>,
    pub default_role: Option<RoleGql>,
    pub ai_chat_model_tier: String,
    pub ai_agent_model_tier: String,
    pub is_auto_model_selection_enabled: bool,
    pub ai_model_id_by_tier: Json<Value>,
    pub ai_additional_instructions: Option<String>,
    pub is_two_factor_authentication_enforced: bool,
    pub trash_retention_days: f64,
    pub event_log_retention_days: f64,
    pub editable_profile_fields: Option<Vec<String>>,
    pub is_internal_messages_import_enabled: bool,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct Application {
    pub id: Uuid,
    pub name: String,
    pub universal_identifier: String,
    pub logo_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct FeatureFlag {
    pub key: String,
    pub value: bool,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingSubscription {
    pub id: Uuid,
    pub status: Option<String>,
    pub interval: Option<String>,
    pub metadata: Option<Json<Value>>,
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_at: Option<chrono::DateTime<chrono::Utc>>,
    pub collection_method: Option<String>,
    pub phases: Vec<BillingSubscriptionSchedulePhase>,
    pub billing_subscription_items: Vec<BillingSubscriptionItem>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingSubscriptionSchedulePhase {
    #[graphql(name = "start_date")]
    pub start_date: f64,
    #[graphql(name = "end_date")]
    pub end_date: f64,
    pub items: Vec<BillingSubscriptionSchedulePhaseItem>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingSubscriptionSchedulePhaseItem {
    pub price: String,
    pub quantity: Option<f64>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingSubscriptionItem {
    pub id: Uuid,
    pub has_reached_current_period_cap: Option<bool>,
    pub quantity: Option<f64>,
    pub stripe_price_id: Option<String>,
    pub unit_amount: Option<f64>,
    pub credit_amount: Option<f64>,
    pub billing_product: Option<BillingProduct>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingProduct {
    pub name: Option<String>,
    pub description: Option<String>,
    pub images: Option<Vec<String>>,
    pub metadata: Option<BillingProductMetadata>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingProductMetadata {
    pub product_key: Option<String>,
    pub plan_key: Option<String>,
    pub price_usage_based: Option<bool>,
    pub is_legacy: Option<bool>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingCustomer {
    pub id: Uuid,
    pub has_payment_method: Option<bool>,
    pub user_credits_fallback: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct BillingEntitlement {
    pub key: String,
    pub value: bool,
}

pub fn split_name(email: &str) -> (String, String) {
    let local = email.split('@').next().unwrap_or(email);
    let mut parts = local
        .split(['.', '_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        });
    let first = parts.next().unwrap_or_else(|| "User".to_string());
    let last = parts.next().unwrap_or_default();
    (first, last)
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Owner => "Owner",
        Role::Admin => "Admin",
        Role::Member => "Member",
    }
}

pub fn role_gql(workspace_id: Uuid, role: Role) -> RoleGql {
    let privileged = matches!(role, Role::Owner | Role::Admin);
    RoleGql {
        id: workspace_id,
        label: role_label(role).to_string(),
        description: None,
        icon: None,
        can_update_all_settings: privileged,
        can_access_all_tools: true,
        is_editable: false,
        can_read_all_object_records: true,
        can_update_all_object_records: true,
        can_soft_delete_all_object_records: true,
        can_destroy_all_object_records: privileged,
        can_be_assigned_to_users: true,
        can_be_assigned_to_agents: false,
        can_be_assigned_to_api_keys: true,
        application_id: NIL_UUID,
        universal_identifier: Some(workspace_id.to_string()),
    }
}

pub fn workspace(
    workspace: &CoreWorkspace,
    role: Role,
    frontend_url: &str,
    member_count: i64,
) -> Workspace {
    let urls = WorkspaceUrls {
        custom_url: None,
        subdomain_url: frontend_url.trim_end_matches('/').to_string(),
    };
    Workspace {
        id: workspace.id,
        display_name: Some(workspace.display_name.clone()),
        logo: None,
        invite_hash: None,
        allow_impersonation: false,
        activation_status: "ACTIVE".to_string(),
        is_public_invite_link_enabled: false,
        is_campaign_click_tracking_enabled: false,
        is_campaign_open_tracking_enabled: false,
        workspace_discoverability: "HIDDEN".to_string(),
        is_google_auth_enabled: false,
        is_microsoft_auth_enabled: false,
        is_password_auth_enabled: true,
        is_google_auth_bypass_enabled: false,
        is_microsoft_auth_bypass_enabled: false,
        is_password_auth_bypass_enabled: false,
        subdomain: workspace.subdomain.clone(),
        custom_domain: None,
        has_valid_signed_enterprise_key: false,
        has_valid_enterprise_validity_token: false,
        workspace_custom_application: None,
        installed_applications: Vec::new(),
        is_custom_domain_enabled: false,
        workspace_urls: urls,
        feature_flags: Some(Vec::new()),
        current_billing_subscription: None,
        billing_customer: None,
        billing_subscriptions: Vec::new(),
        billing_entitlements: Vec::new(),
        workspace_members_count: Some(member_count as f64),
        default_role: Some(role_gql(workspace.id, role)),
        ai_chat_model_tier: "balanced".to_string(),
        ai_agent_model_tier: "balanced".to_string(),
        is_auto_model_selection_enabled: false,
        ai_model_id_by_tier: Json(serde_json::json!({})),
        ai_additional_instructions: None,
        is_two_factor_authentication_enforced: false,
        trash_retention_days: 30.0,
        event_log_retention_days: 30.0,
        editable_profile_fields: Some(Vec::new()),
        is_internal_messages_import_enabled: false,
    }
}

pub fn workspace_member(
    member: &WorkspaceMember,
    user: &CoreUser,
    role: Role,
) -> WorkspaceMemberGql {
    let (first, last) = split_name(&user.email);
    WorkspaceMemberGql {
        id: member.id,
        created_at: member.created_at,
        updated_at: member.created_at,
        position: 0.0,
        name: FullName {
            first_name: first,
            last_name: last,
        },
        user_email: user.email.clone(),
        color_scheme: "Light".to_string(),
        ui_scale: "1".to_string(),
        open_record_in: "SIDE_PANEL".to_string(),
        avatar_url: None,
        locale: Some("en".to_string()),
        calendar_start_day: Some(1),
        time_zone: Some("UTC".to_string()),
        date_format: Some("SYSTEM".to_string()),
        time_format: Some("SYSTEM".to_string()),
        roles: Some(vec![role_gql(member.workspace_id, role)]),
        user_id: user.id,
        user_workspace_id: Some(member.id),
        number_format: Some("SYSTEM".to_string()),
        job_title: None,
        search_vector: None,
        created_by: Actor::default(),
        updated_by: Actor::default(),
    }
}
