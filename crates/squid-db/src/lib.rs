//! PostgreSQL access layer: connection pool, core migrations, per-workspace
//! schema management, and the Phase 4 repositories.
//!
//! Tenant isolation invariant: every workspace-scoped query interpolates a
//! *validated* schema name derived from a UUID. No caller-supplied SQL fragment
//! ever reaches the database.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::de::DeserializeOwned;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use squid_core::{
    Actor, AggregateOperations, ApiKey, Company, CreateCompanyInput, CreateFieldMetadataInput,
    CreateIndexInput, CreateNoteInput, CreateNoteTargetInput, CreateObjectMetadataInput,
    CreateOpportunityInput, CreatePersonInput, CreateTaskInput, CreateTaskTargetInput,
    CreateViewFieldGroupInput, CreateViewFieldInput, CreateViewFilterGroupInput,
    CreateViewFilterInput, CreateViewGroupInput, CreateViewInput, CreateViewSortInput, DateOp,
    Emails, FieldMetadata, FieldMetadataType, FieldSummary, FileFolder, FileItemInput, FileObject,
    FilterExpr, FullName, IndexFieldMetadata, IndexMetadata, IndexType, MaybeUndefined,
    MetadataReadability, MetadataWritability, Note, NoteTarget, ObjectMetadata, ObjectOpenRecordIn,
    ObjectSharingReach, ObjectSummary, Opportunity, OpportunityStage, Person, RelationMetadata,
    RelationType, Role, SearchFieldMetadata, StringOp, Task, TaskStatus, TaskTarget,
    UpdateCompanyInput, UpdateFieldMetadataInput, UpdateNoteInput, UpdateNoteTargetInput,
    UpdateObjectMetadataInput, UpdateOpportunityInput, UpdatePersonInput, UpdateTaskInput,
    UpdateTaskTargetInput, UpdateViewFieldGroupInput, UpdateViewFieldInput,
    UpdateViewFilterGroupInput, UpdateViewFilterInput, UpdateViewGroupInput, UpdateViewInput,
    UpdateViewSortInput, User, UuidOp, View, ViewCalendarLayout, ViewField, ViewFieldGroup,
    ViewFilter, ViewFilterGroup, ViewFilterGroupLogicalOperator, ViewFilterOperand, ViewGroup,
    ViewKey, ViewOpenRecordIn, ViewSort, ViewSortDirection, ViewType, ViewVisibility, Workspace,
};
use squid_core::{RolePatch, RoleRecord, RoleWrite, WebhookRecord};
use uuid::Uuid;

const PERMISSION_FLAG_KEYS: &[&str] = &[
    "API_KEYS_AND_WEBHOOKS",
    "WORKSPACE",
    "WORKSPACE_MEMBERS",
    "ROLES",
    "DATA_MODEL",
    "SECURITY",
    "WORKFLOWS",
    "IMPERSONATE",
    "SSO_BYPASS",
    "APPLICATIONS",
    "MARKETPLACE_APPS",
    "LAYOUTS",
    "BILLING",
    "AI_SETTINGS",
    "AI",
    "VIEWS",
    "UPLOAD_FILE",
    "DOWNLOAD_FILE",
    "SEND_EMAIL_TOOL",
    "CREATE_CALENDAR_EVENT_TOOL",
    "HTTP_REQUEST_TOOL",
    "CODE_INTERPRETER_TOOL",
    "IMPORT_CSV",
    "EXPORT_CSV",
    "CONNECTED_ACCOUNTS",
    "PROFILE_INFORMATION",
];

const MEMBER_PERMISSION_FLAG_KEYS: &[&str] = &[
    "VIEWS",
    "UPLOAD_FILE",
    "DOWNLOAD_FILE",
    "PROFILE_INFORMATION",
];

const CORE_SCHEMA_SQL: &str = include_str!("../migrations/core/0001_core.sql");
const WORKSPACE_PERSON_SQL: &str = include_str!("../migrations/workspace/0001_person.sql");
const WORKSPACE_PERSON_CONTRACT_SQL: &str =
    include_str!("../migrations/workspace/0002_person_contract.sql");
const WORKSPACE_COMPANY_SQL: &str = include_str!("../migrations/workspace/0003_company.sql");
const WORKSPACE_OPPORTUNITY_SQL: &str =
    include_str!("../migrations/workspace/0004_opportunity.sql");
const WORKSPACE_TASK_SQL: &str = include_str!("../migrations/workspace/0005_task.sql");
const WORKSPACE_NOTE_SQL: &str = include_str!("../migrations/workspace/0006_note.sql");
const WORKSPACE_TASK_TARGET_SQL: &str =
    include_str!("../migrations/workspace/0007_task_target.sql");
const WORKSPACE_NOTE_TARGET_SQL: &str =
    include_str!("../migrations/workspace/0008_note_target.sql");
const CORE_API_KEYS_SQL: &str = include_str!("../migrations/core/0002_api_keys.sql");
const CORE_METADATA_SQL: &str = include_str!("../migrations/core/0003_metadata.sql");
const CORE_VIEWS_SQL: &str = include_str!("../migrations/core/0004_views.sql");
const CORE_REFRESH_TOKENS_SQL: &str = include_str!("../migrations/core/0005_refresh_tokens.sql");
const CORE_FILES_SQL: &str = include_str!("../migrations/core/0006_files.sql");
const CORE_INDEXES_SQL: &str = include_str!("../migrations/core/0007_indexes.sql");
const CORE_ROLES_SQL: &str = include_str!("../migrations/core/0008_roles.sql");
const CORE_WEBHOOKS_SQL: &str = include_str!("../migrations/core/0009_webhooks.sql");
const CORE_ROLE_PERMISSION_FLAGS_SQL: &str =
    include_str!("../migrations/core/0010_role_permission_flags.sql");
const CORE_IDEMPOTENCY_KEYS_SQL: &str =
    include_str!("../migrations/core/0011_idempotency_keys.sql");
const CORE_WEBHOOK_DELIVERIES_SQL: &str =
    include_str!("../migrations/core/0012_webhook_deliveries.sql");
const CORE_WEBHOOK_DELIVERY_REPLAY_SQL: &str =
    include_str!("../migrations/core/0013_webhook_delivery_replay.sql");
const CORE_JOBS_SQL: &str = include_str!("../migrations/core/0014_jobs.sql");

#[derive(Debug, Clone)]
pub struct WebhookDeliveryRecord {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event: String,
    pub event_id: Uuid,
    pub state: String,
    pub attempts: i32,
    pub last_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub target_url: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub signature: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub kind: String,
    pub payload: serde_json::Value,
    pub state: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub filename: String,
    pub path: String,
    pub size: i64,
    pub content_type: String,
    pub file_folder: String,
    pub field_metadata_id: Option<Uuid>,
    pub uploaded: bool,
    pub created_at: DateTime<Utc>,
}

impl FileRecord {
    pub fn extension(&self) -> Option<String> {
        self.filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .filter(|extension| !extension.is_empty())
    }

    pub fn url(&self) -> String {
        format!("/files/{}", self.id)
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    pub api_key_id: Uuid,
    pub workspace_id: Uuid,
    pub role: Role,
    pub role_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum IdempotencyClaim {
    /// The caller owns the key and must execute the request.
    Claimed,
    /// A completed response exists for the same request.
    Replay {
        status: u16,
        content_type: String,
        body: Vec<u8>,
    },
    /// The key exists for a different request.
    Conflict,
    /// Another request with the same key is still running.
    InProgress,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub record_id: Uuid,
    pub object_name_singular: String,
    pub object_label_singular: String,
    pub label: String,
    pub image_url: Option<String>,
    pub ts_rank_cd: f32,
    pub ts_rank: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RefreshTokenRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
enum Bind {
    Uuid(Uuid),
    Uuids(Vec<Uuid>),
    Text(String),
    Texts(Vec<String>),
    Timestamp(DateTime<Utc>),
    Timestamps(Vec<DateTime<Utc>>),
}

struct FilterSql {
    sql: String,
    binds: Vec<Bind>,
}

fn compile_filter(expr: &FilterExpr, schema: &str, start: usize) -> FilterSql {
    let mut binds = Vec::new();
    let sql = compile_expr(expr, schema, &mut binds, start);
    FilterSql { sql, binds }
}

fn placeholder(binds: &mut Vec<Bind>, value: Bind, start: usize) -> String {
    binds.push(value);
    format!("${}", start + binds.len() - 1)
}

fn compile_expr(expr: &FilterExpr, schema: &str, binds: &mut Vec<Bind>, start: usize) -> String {
    match expr {
        FilterExpr::Id(op) => compile_uuid("id", op, binds, start),
        FilterExpr::CreatedAt(op) => compile_date("created_at", op, binds, start),
        FilterExpr::UpdatedAt(op) => compile_date("updated_at", op, binds, start),
        FilterExpr::FirstName(op) => compile_string("name_first", op, binds, start),
        FilterExpr::LastName(op) => compile_string("name_last", op, binds, start),
        FilterExpr::PrimaryEmail(op) => compile_string("primary_email", op, binds, start),
        FilterExpr::JobTitle(op) => compile_string("job_title", op, binds, start),
        FilterExpr::CompanyId(op) => compile_uuid("company_id", op, binds, start),
        FilterExpr::CompanyName(op) => compile_string("name", op, binds, start),
        FilterExpr::CompanyCreatedAt(op) => compile_date("created_at", op, binds, start),
        FilterExpr::CompanyUpdatedAt(op) => compile_date("updated_at", op, binds, start),
        FilterExpr::CompanyAccountOwnerId(op) => {
            compile_uuid("account_owner_id", op, binds, start)
        }
        FilterExpr::RelatedCompany(inner) => format!(
            "company_id IN (SELECT id FROM {schema}.company WHERE deleted_at IS NULL AND ({}))",
            compile_expr(inner, schema, binds, start)
        ),
        FilterExpr::RelatedPerson(inner) => format!(
            "point_of_contact_id IN (SELECT id FROM {schema}.person WHERE deleted_at IS NULL AND ({}))",
            compile_expr(inner, schema, binds, start)
        ),
        FilterExpr::And(exprs) => compile_join("AND", exprs, "TRUE", schema, binds, start),
        FilterExpr::Or(exprs) => compile_join("OR", exprs, "FALSE", schema, binds, start),
        FilterExpr::Not(expr) => format!("NOT ({})", compile_expr(expr, schema, binds, start)),
    }
}

fn compile_join(
    operator: &str,
    exprs: &[FilterExpr],
    empty: &str,
    schema: &str,
    binds: &mut Vec<Bind>,
    start: usize,
) -> String {
    if exprs.is_empty() {
        return empty.to_string();
    }
    let parts: Vec<String> = exprs
        .iter()
        .map(|expr| compile_expr(expr, schema, binds, start))
        .collect();
    format!("({})", parts.join(&format!(" {operator} ")))
}

fn compile_string(column: &str, op: &StringOp, binds: &mut Vec<Bind>, start: usize) -> String {
    match op {
        StringOp::Eq(value) => format!(
            "{column} = {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Neq(value) => format!(
            "{column} <> {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Gt(value) => format!(
            "{column} > {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Gte(value) => format!(
            "{column} >= {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Lt(value) => format!(
            "{column} < {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Lte(value) => format!(
            "{column} <= {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::In(values) if values.is_empty() => "FALSE".to_string(),
        StringOp::In(values) => format!(
            "{column} = ANY({})",
            placeholder(binds, Bind::Texts(values.clone()), start)
        ),
        StringOp::StartsWith(value) => format!(
            "{column} LIKE {}",
            placeholder(binds, Bind::Text(format!("{}%", escape_like(value))), start)
        ),
        StringOp::EndsWith(value) => format!(
            "{column} LIKE {}",
            placeholder(binds, Bind::Text(format!("%{}", escape_like(value))), start)
        ),
        StringOp::Like(value) => format!(
            "{column} LIKE {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::ILike(value) => format!(
            "{column} ILIKE {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::Regex(value) => format!(
            "{column} ~ {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::IRegex(value) => format!(
            "{column} ~* {}",
            placeholder(binds, Bind::Text(value.clone()), start)
        ),
        StringOp::IsNull(true) => format!("{column} IS NULL"),
        StringOp::IsNull(false) => format!("{column} IS NOT NULL"),
    }
}

fn compile_uuid(column: &str, op: &UuidOp, binds: &mut Vec<Bind>, start: usize) -> String {
    match op {
        UuidOp::Eq(value) => format!(
            "{column} = {}",
            placeholder(binds, Bind::Uuid(*value), start)
        ),
        UuidOp::Neq(value) => format!(
            "{column} <> {}",
            placeholder(binds, Bind::Uuid(*value), start)
        ),
        UuidOp::In(values) if values.is_empty() => "FALSE".to_string(),
        UuidOp::In(values) => format!(
            "{column} = ANY({})",
            placeholder(binds, Bind::Uuids(values.clone()), start)
        ),
        UuidOp::IsNull(true) => format!("{column} IS NULL"),
        UuidOp::IsNull(false) => format!("{column} IS NOT NULL"),
    }
}

fn compile_date(column: &str, op: &DateOp, binds: &mut Vec<Bind>, start: usize) -> String {
    match op {
        DateOp::Eq(value) => format!(
            "{column} = {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::Neq(value) => format!(
            "{column} <> {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::Gt(value) => format!(
            "{column} > {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::Gte(value) => format!(
            "{column} >= {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::Lt(value) => format!(
            "{column} < {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::Lte(value) => format!(
            "{column} <= {}",
            placeholder(binds, Bind::Timestamp(*value), start)
        ),
        DateOp::In(values) if values.is_empty() => "FALSE".to_string(),
        DateOp::In(values) => format!(
            "{column} = ANY({})",
            placeholder(binds, Bind::Timestamps(values.clone()), start)
        ),
        DateOp::IsNull(true) => format!("{column} IS NULL"),
        DateOp::IsNull(false) => format!("{column} IS NOT NULL"),
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

type PgQuery<'a> = sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>;

fn bind_filter<'a>(mut query: PgQuery<'a>, binds: &[Bind]) -> PgQuery<'a> {
    for value in binds {
        query = match value {
            Bind::Uuid(value) => query.bind(*value),
            Bind::Uuids(values) => query.bind(values.clone()),
            Bind::Text(value) => query.bind(value.clone()),
            Bind::Texts(values) => query.bind(values.clone()),
            Bind::Timestamp(value) => query.bind(*value),
            Bind::Timestamps(values) => query.bind(values.clone()),
        };
    }
    query
}

type PgScalarQuery<'a> =
    sqlx::query::QueryScalar<'a, sqlx::Postgres, i64, sqlx::postgres::PgArguments>;

fn bind_filter_scalar<'a>(mut query: PgScalarQuery<'a>, binds: &[Bind]) -> PgScalarQuery<'a> {
    for value in binds {
        query = match value {
            Bind::Uuid(value) => query.bind(*value),
            Bind::Uuids(values) => query.bind(values.clone()),
            Bind::Text(value) => query.bind(value.clone()),
            Bind::Texts(values) => query.bind(values.clone()),
            Bind::Timestamp(value) => query.bind(*value),
            Bind::Timestamps(values) => query.bind(values.clone()),
        };
    }
    query
}

const API_KEY_SELECT: &str =
    "id, name, role, role_id, expires_at, revoked_at, created_at, updated_at";

fn row_to_api_key(row: &sqlx::postgres::PgRow) -> ApiKey {
    ApiKey {
        id: row.get("id"),
        name: row.get("name"),
        role: Role::parse(row.get("role")).unwrap_or(Role::Member),
        role_id: row.get("role_id"),
        expires_at: row.get("expires_at"),
        revoked_at: row.get("revoked_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

impl Db {
    pub async fn connect(database_url: &str, max_connections: u32) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Idempotently apply core migrations. Workspace schemas are created on
    /// workspace bootstrap, not here.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        // Serialise DDL across processes/tests: concurrent `CREATE SCHEMA IF
        // NOT EXISTS` can still race in PostgreSQL.
        const MIGRATION_LOCK: i64 = 0x5351_4944_4d49_4700;
        let mut conn = self.pool.acquire().await?;

        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(MIGRATION_LOCK)
            .execute(&mut *conn)
            .await?;

        let mut result = Ok(());
        for ddl in [
            CORE_SCHEMA_SQL,
            CORE_API_KEYS_SQL,
            CORE_METADATA_SQL,
            CORE_VIEWS_SQL,
            CORE_REFRESH_TOKENS_SQL,
            CORE_FILES_SQL,
            CORE_INDEXES_SQL,
            CORE_ROLES_SQL,
            CORE_WEBHOOKS_SQL,
            CORE_ROLE_PERMISSION_FLAGS_SQL,
            CORE_IDEMPOTENCY_KEYS_SQL,
            CORE_WEBHOOK_DELIVERIES_SQL,
            CORE_WEBHOOK_DELIVERY_REPLAY_SQL,
            CORE_JOBS_SQL,
        ] {
            result = sqlx::raw_sql(ddl).execute(&mut *conn).await.map(|_| ());
            if result.is_err() {
                break;
            }
        }

        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(MIGRATION_LOCK)
            .execute(&mut *conn)
            .await;

        result?;
        Ok(())
    }

    // ----- idempotency ---------------------------------------------------

    /// Claim a replay key, replay a completed response, or reject reuse with
    /// a different request. In-progress claims expire after two minutes so a
    /// crashed process cannot hold a key forever.
    pub async fn claim_idempotency(
        &self,
        workspace_id: Uuid,
        key: &str,
        method: &str,
        path: &str,
        request_hash: &str,
    ) -> anyhow::Result<IdempotencyClaim> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM core.idempotency_keys
             WHERE workspace_id = $1 AND key = $2 AND expires_at <= now()",
        )
        .bind(workspace_id)
        .bind(key)
        .execute(&mut *tx)
        .await?;

        let inserted = sqlx::query(
            "INSERT INTO core.idempotency_keys
                (workspace_id, key, method, path, request_hash, state, expires_at)
             VALUES ($1, $2, $3, $4, $5, 'in_progress', now() + interval '2 minutes')
             ON CONFLICT (workspace_id, key) DO NOTHING
             RETURNING key",
        )
        .bind(workspace_id)
        .bind(key)
        .bind(method)
        .bind(path)
        .bind(request_hash)
        .fetch_optional(&mut *tx)
        .await?;

        if inserted.is_some() {
            tx.commit().await?;
            return Ok(IdempotencyClaim::Claimed);
        }

        let row = sqlx::query(
            "SELECT method, path, request_hash, state, response_status,
                    response_content_type, response_body
             FROM core.idempotency_keys
             WHERE workspace_id = $1 AND key = $2
             FOR UPDATE",
        )
        .bind(workspace_id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;

        let Some(row) = row else {
            // Lost a race with expiry cleanup; let the caller retry the
            // request with the same key rather than treating it as success.
            return Ok(IdempotencyClaim::InProgress);
        };

        let same_request = row.get::<String, _>("method") == method
            && row.get::<String, _>("path") == path
            && row.get::<String, _>("request_hash") == request_hash;
        if !same_request {
            return Ok(IdempotencyClaim::Conflict);
        }

        if row.get::<String, _>("state") == "completed" {
            let status = row.get::<Option<i16>, _>("response_status");
            let body = row.get::<Option<Vec<u8>>, _>("response_body");
            if let (Some(status), Some(body)) = (status, body) {
                return Ok(IdempotencyClaim::Replay {
                    status: status as u16,
                    content_type: row
                        .get::<Option<String>, _>("response_content_type")
                        .unwrap_or_else(|| "application/json".to_string()),
                    body,
                });
            }
        }

        Ok(IdempotencyClaim::InProgress)
    }

    pub async fn complete_idempotency(
        &self,
        workspace_id: Uuid,
        key: &str,
        status: u16,
        content_type: &str,
        body: &[u8],
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE core.idempotency_keys
             SET state = 'completed', response_status = $3,
                 response_content_type = $4, response_body = $5,
                 completed_at = now(), expires_at = now() + interval '24 hours'
             WHERE workspace_id = $1 AND key = $2 AND state = 'in_progress'",
        )
        .bind(workspace_id)
        .bind(key)
        .bind(status as i16)
        .bind(content_type)
        .bind(body)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn release_idempotency(&self, workspace_id: Uuid, key: &str) -> anyhow::Result<()> {
        sqlx::query(
            "DELETE FROM core.idempotency_keys
             WHERE workspace_id = $1 AND key = $2 AND state = 'in_progress'",
        )
        .bind(workspace_id)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ----- users ---------------------------------------------------------

    pub async fn create_user(&self, email: &str, password_hash: &str) -> sqlx::Result<User> {
        let row = sqlx::query(
            "INSERT INTO core.users (email, password_hash) VALUES ($1, $2)
             RETURNING id, email, created_at",
        )
        .bind(email)
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(User {
            id: row.get("id"),
            email: row.get("email"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn find_user_by_email(&self, email: &str) -> sqlx::Result<Option<(User, String)>> {
        let row = sqlx::query(
            "SELECT id, email, created_at, password_hash
             FROM core.users WHERE lower(email) = lower($1)",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| {
            (
                User {
                    id: row.get("id"),
                    email: row.get("email"),
                    created_at: row.get("created_at"),
                },
                row.get::<String, _>("password_hash"),
            )
        }))
    }

    pub async fn find_user_by_id(&self, user_id: Uuid) -> sqlx::Result<Option<User>> {
        let row = sqlx::query("SELECT id, email, created_at FROM core.users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|row| User {
            id: row.get("id"),
            email: row.get("email"),
            created_at: row.get("created_at"),
        }))
    }

    // ----- workspaces ----------------------------------------------------

    pub async fn create_workspace(
        &self,
        display_name: &str,
        subdomain: &str,
    ) -> sqlx::Result<Workspace> {
        let row = sqlx::query(
            "INSERT INTO core.workspaces (display_name, subdomain) VALUES ($1, $2)
             RETURNING id, display_name, subdomain, created_at",
        )
        .bind(display_name)
        .bind(subdomain)
        .fetch_one(&self.pool)
        .await?;

        Ok(Workspace {
            id: row.get("id"),
            display_name: row.get("display_name"),
            subdomain: row.get("subdomain"),
            created_at: row.get("created_at"),
        })
    }

    /// Atomically create a user, a workspace and the owner membership.
    /// The per-workspace schema is created separately (DDL), so callers must
    /// clean up on failure.
    pub async fn bootstrap_user_workspace(
        &self,
        email: &str,
        password_hash: &str,
        display_name: &str,
        subdomain: &str,
    ) -> sqlx::Result<(User, Workspace)> {
        let mut tx = self.pool.begin().await?;

        let user_row = sqlx::query(
            "INSERT INTO core.users (email, password_hash) VALUES ($1, $2)
             RETURNING id, email, created_at",
        )
        .bind(email)
        .bind(password_hash)
        .fetch_one(&mut *tx)
        .await?;

        let workspace_row = sqlx::query(
            "INSERT INTO core.workspaces (display_name, subdomain) VALUES ($1, $2)
             RETURNING id, display_name, subdomain, created_at",
        )
        .bind(display_name)
        .bind(subdomain)
        .fetch_one(&mut *tx)
        .await?;

        let user_id: Uuid = user_row.get("id");
        let workspace_id: Uuid = workspace_row.get("id");

        sqlx::query(
            "INSERT INTO core.workspace_members (workspace_id, user_id, role)
             VALUES ($1, $2, 'OWNER')",
        )
        .bind(workspace_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok((
            User {
                id: user_id,
                email: user_row.get("email"),
                created_at: user_row.get("created_at"),
            },
            Workspace {
                id: workspace_id,
                display_name: workspace_row.get("display_name"),
                subdomain: workspace_row.get("subdomain"),
                created_at: workspace_row.get("created_at"),
            },
        ))
    }

    pub async fn delete_workspace(&self, workspace_id: Uuid) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM core.workspaces WHERE id = $1")
            .bind(workspace_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_user(&self, user_id: Uuid) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM core.users WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_workspace(&self, workspace_id: Uuid) -> sqlx::Result<Option<Workspace>> {
        let row = sqlx::query(
            "SELECT id, display_name, subdomain, created_at FROM core.workspaces WHERE id = $1",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Workspace {
            id: row.get("id"),
            display_name: row.get("display_name"),
            subdomain: row.get("subdomain"),
            created_at: row.get("created_at"),
        }))
    }

    pub async fn add_workspace_member(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        role: Role,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO core.workspace_members (workspace_id, user_id, role)
             VALUES ($1, $2, $3)
             ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(role.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// First workspace the user belongs to, used for login bootstrap.
    pub async fn find_workspace_for_user(
        &self,
        user_id: Uuid,
    ) -> sqlx::Result<Option<(Workspace, Role)>> {
        let row = sqlx::query(
            "SELECT w.id, w.display_name, w.subdomain, w.created_at, m.role
             FROM core.workspace_members m
             JOIN core.workspaces w ON w.id = m.workspace_id
             WHERE m.user_id = $1
             ORDER BY m.created_at ASC
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| {
            let role = Role::parse(&row.get::<String, _>("role")).unwrap_or(Role::Member);
            (
                Workspace {
                    id: row.get("id"),
                    display_name: row.get("display_name"),
                    subdomain: row.get("subdomain"),
                    created_at: row.get("created_at"),
                },
                role,
            )
        }))
    }

    pub async fn find_membership_role(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Option<Role>> {
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM core.workspace_members WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(role.and_then(|r| Role::parse(&r)))
    }

    pub async fn find_membership_role_id(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Option<Uuid>> {
        sqlx::query_scalar(
            "SELECT role_id FROM core.workspace_members WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map(|value| value.flatten())
    }

    pub async fn find_workspace_member(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Option<squid_core::WorkspaceMember>> {
        let row = sqlx::query(
            "SELECT id, workspace_id, user_id, role, role_id, created_at
             FROM core.workspace_members
             WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|row| {
            let role: String = row.get("role");
            Role::parse(&role).map(|role| squid_core::WorkspaceMember {
                id: row.get("id"),
                workspace_id: row.get("workspace_id"),
                user_id: row.get("user_id"),
                role,
                role_id: row.get("role_id"),
                created_at: row.get("created_at"),
            })
        }))
    }

    pub async fn list_workspace_members_with_users(
        &self,
        workspace_id: Uuid,
    ) -> sqlx::Result<Vec<(squid_core::WorkspaceMember, squid_core::User)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.workspace_id, m.user_id, m.role, m.role_id, m.created_at,
                    u.email AS user_email, u.created_at AS user_created_at
             FROM core.workspace_members m
             JOIN core.users u ON u.id = m.user_id
             WHERE m.workspace_id = $1
             ORDER BY m.created_at ASC, m.id ASC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        let mut members = Vec::with_capacity(rows.len());
        for row in &rows {
            let role: String = row.get("role");
            let Some(role) = Role::parse(&role) else {
                continue;
            };
            members.push((
                squid_core::WorkspaceMember {
                    id: row.get("id"),
                    workspace_id: row.get("workspace_id"),
                    user_id: row.get("user_id"),
                    role,
                    role_id: row.get("role_id"),
                    created_at: row.get("created_at"),
                },
                squid_core::User {
                    id: row.get("user_id"),
                    email: row.get("user_email"),
                    created_at: row.get("user_created_at"),
                },
            ));
        }
        Ok(members)
    }

    pub async fn count_workspace_members(&self, workspace_id: Uuid) -> sqlx::Result<i64> {
        sqlx::query_scalar("SELECT count(*) FROM core.workspace_members WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_one(&self.pool)
            .await
    }

    // ----- sessions ------------------------------------------------------

    pub async fn create_session(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<Uuid> {
        sqlx::query_scalar(
            "INSERT INTO core.user_sessions (user_id, workspace_id, token_hash, expires_at)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(workspace_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn find_session_by_hash(&self, token_hash: &str) -> sqlx::Result<Option<SessionRow>> {
        let row = sqlx::query(
            "SELECT id, user_id, workspace_id, expires_at
             FROM core.user_sessions
             WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| SessionRow {
            id: row.get("id"),
            user_id: row.get("user_id"),
            workspace_id: row.get("workspace_id"),
            expires_at: row.get("expires_at"),
        }))
    }

    pub async fn find_session_by_id(&self, session_id: Uuid) -> sqlx::Result<Option<SessionRow>> {
        let row = sqlx::query(
            "SELECT id, user_id, workspace_id, expires_at
             FROM core.user_sessions
             WHERE id = $1 AND expires_at > now()",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| SessionRow {
            id: row.get("id"),
            user_id: row.get("user_id"),
            workspace_id: row.get("workspace_id"),
            expires_at: row.get("expires_at"),
        }))
    }

    pub async fn create_refresh_token(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        session_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<Uuid> {
        sqlx::query_scalar(
            "INSERT INTO core.refresh_tokens
                 (user_id, workspace_id, session_id, token_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(user_id)
        .bind(workspace_id)
        .bind(session_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn find_refresh_token_by_hash(
        &self,
        token_hash: &str,
    ) -> sqlx::Result<Option<RefreshTokenRow>> {
        let row = sqlx::query(
            "SELECT id, user_id, workspace_id, session_id, expires_at
             FROM core.refresh_tokens WHERE token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| RefreshTokenRow {
            id: row.get("id"),
            user_id: row.get("user_id"),
            workspace_id: row.get("workspace_id"),
            session_id: row.get("session_id"),
            expires_at: row.get("expires_at"),
        }))
    }

    /// Atomically consume a refresh token if its session is still alive, then
    /// store its replacement. Returns `None` for expired/revoked/replayed
    /// tokens or dead sessions.
    pub async fn rotate_refresh_token(
        &self,
        old_hash: &str,
        new_hash: &str,
        new_expires_at: DateTime<Utc>,
    ) -> sqlx::Result<Option<RefreshTokenRow>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE core.refresh_tokens rt
             SET revoked_at = now(), replaced_by_token_hash = $2
             FROM core.user_sessions s
             WHERE rt.token_hash = $1
               AND rt.session_id = s.id
               AND rt.revoked_at IS NULL
               AND rt.expires_at > now()
               AND s.expires_at > now()
             RETURNING rt.id, rt.user_id, rt.workspace_id, rt.session_id, rt.expires_at",
        )
        .bind(old_hash)
        .bind(new_hash)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };

        let rotated = RefreshTokenRow {
            id: row.get("id"),
            user_id: row.get("user_id"),
            workspace_id: row.get("workspace_id"),
            session_id: row.get("session_id"),
            expires_at: row.get("expires_at"),
        };

        sqlx::query(
            "INSERT INTO core.refresh_tokens
                 (user_id, workspace_id, session_id, token_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(rotated.user_id)
        .bind(rotated.workspace_id)
        .bind(rotated.session_id)
        .bind(new_hash)
        .bind(new_expires_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(rotated))
    }

    pub async fn delete_session(&self, session_id: Uuid) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM core.user_sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ----- tenant schema -------------------------------------------------

    /// Deterministic, validated Postgres schema name for a workspace.
    pub fn workspace_schema_name(workspace_id: Uuid) -> String {
        format!("workspace_{}", workspace_id.simple())
    }

    pub async fn create_workspace_schema(&self, workspace_id: Uuid) -> anyhow::Result<String> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        sqlx::raw_sql(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&self.pool)
            .await?;

        for ddl in [
            WORKSPACE_PERSON_SQL,
            WORKSPACE_PERSON_CONTRACT_SQL,
            WORKSPACE_COMPANY_SQL,
            WORKSPACE_OPPORTUNITY_SQL,
            WORKSPACE_TASK_SQL,
            WORKSPACE_NOTE_SQL,
            WORKSPACE_TASK_TARGET_SQL,
            WORKSPACE_NOTE_TARGET_SQL,
        ] {
            let ddl = ddl.replace("{{schema}}", &schema);
            sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        }

        Ok(schema)
    }

    pub async fn drop_workspace_schema(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        sqlx::raw_sql(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ----- api keys ------------------------------------------------------

    pub async fn create_api_key(
        &self,
        workspace_id: Uuid,
        created_by: Option<Uuid>,
        name: &str,
        role: Role,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<ApiKey> {
        let row = sqlx::query(&format!(
            "INSERT INTO core.api_keys (workspace_id, created_by, name, role, expires_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING {API_KEY_SELECT}"
        ))
        .bind(workspace_id)
        .bind(created_by)
        .bind(name)
        .bind(role.as_str())
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_api_key(&row))
    }

    pub async fn list_api_keys(&self, workspace_id: Uuid) -> sqlx::Result<Vec<ApiKey>> {
        let rows = sqlx::query(&format!(
            "SELECT {API_KEY_SELECT} FROM core.api_keys
             WHERE workspace_id = $1 ORDER BY created_at ASC"
        ))
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_api_key).collect())
    }

    pub async fn get_api_key(&self, workspace_id: Uuid, id: Uuid) -> sqlx::Result<Option<ApiKey>> {
        let row = sqlx::query(&format!(
            "SELECT {API_KEY_SELECT} FROM core.api_keys
             WHERE id = $1 AND workspace_id = $2"
        ))
        .bind(id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_api_key))
    }

    pub async fn set_api_key_token(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<Option<ApiKey>> {
        let row = sqlx::query(&format!(
            "UPDATE core.api_keys
             SET token_hash = $3, expires_at = $4, updated_at = now()
             WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL
             RETURNING {API_KEY_SELECT}"
        ))
        .bind(id)
        .bind(workspace_id)
        .bind(token_hash)
        .bind(expires_at)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_api_key))
    }

    pub async fn revoke_api_key(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<Option<ApiKey>> {
        let row = sqlx::query(&format!(
            "UPDATE core.api_keys
             SET revoked_at = now(), token_hash = NULL, updated_at = now()
             WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL
             RETURNING {API_KEY_SELECT}"
        ))
        .bind(id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_api_key))
    }

    pub async fn find_api_key_by_token_hash(
        &self,
        token_hash: &str,
    ) -> sqlx::Result<Option<ApiKeyAuth>> {
        let row = sqlx::query(
            "SELECT id, workspace_id, role, role_id, expires_at FROM core.api_keys
             WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| ApiKeyAuth {
            api_key_id: row.get("id"),
            workspace_id: row.get("workspace_id"),
            role: Role::parse(row.get("role")).unwrap_or(Role::Member),
            role_id: row.get("role_id"),
            expires_at: row.get("expires_at"),
        }))
    }

    // ----- person --------------------------------------------------------

    /// One page of people plus connection metadata.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_persons_page(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
        starting_after: Option<Uuid>,
        ending_before: Option<Uuid>,
        order: &[OrderTerm],
        filter: Option<&FilterExpr>,
    ) -> anyhow::Result<PersonPage> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let total = self.count_persons_filtered(workspace_id, filter).await?;
        let limit = limit.clamp(1, 1000);
        let fetch = limit + 1;

        let has_next_page;
        let has_previous_page;
        let mut rows = if let Some(cursor_id) = starting_after {
            let cursor_created_at = self.person_created_at(&schema, cursor_id).await?;
            let filter = filter.map(|filter| compile_filter(filter, &schema, 3));
            let filter_sql = filter
                .as_ref()
                .map(|filter| format!(" AND ({})", filter.sql))
                .unwrap_or_default();
            let limit_index = 3 + filter.as_ref().map_or(0, |filter| filter.binds.len());
            has_previous_page = true;
            let sql = format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL AND (created_at, id) > ($1, $2){filter_sql}
                 ORDER BY created_at ASC, id ASC
                 LIMIT ${limit_index}"
            );
            let mut query = sqlx::query(&sql).bind(cursor_created_at).bind(cursor_id);
            if let Some(filter) = &filter {
                query = bind_filter(query, &filter.binds);
            }
            let rows = query.bind(fetch).fetch_all(&self.pool).await?;
            has_next_page = rows.len() as i64 > limit;
            rows
        } else if let Some(cursor_id) = ending_before {
            let cursor_created_at = self.person_created_at(&schema, cursor_id).await?;
            let filter = filter.map(|filter| compile_filter(filter, &schema, 3));
            let filter_sql = filter
                .as_ref()
                .map(|filter| format!(" AND ({})", filter.sql))
                .unwrap_or_default();
            let limit_index = 3 + filter.as_ref().map_or(0, |filter| filter.binds.len());
            let sql = format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL AND (created_at, id) < ($1, $2){filter_sql}
                 ORDER BY created_at DESC, id DESC
                 LIMIT ${limit_index}"
            );
            let mut query = sqlx::query(&sql).bind(cursor_created_at).bind(cursor_id);
            if let Some(filter) = &filter {
                query = bind_filter(query, &filter.binds);
            }
            let mut rows = query.bind(fetch).fetch_all(&self.pool).await?;
            has_previous_page = rows.len() as i64 > limit;
            has_next_page = true;
            rows.truncate(limit as usize);
            rows.reverse();
            rows
        } else {
            let filter = filter.map(|filter| compile_filter(filter, &schema, 1));
            let filter_sql = filter
                .as_ref()
                .map(|filter| format!(" AND ({})", filter.sql))
                .unwrap_or_default();
            let limit_index = 1 + filter.as_ref().map_or(0, |filter| filter.binds.len());
            let offset_index = limit_index + 1;
            let order_sql = if order.is_empty() {
                "created_at ASC NULLS FIRST, id ASC".to_string()
            } else {
                format!(
                    "{}, id ASC",
                    order
                        .iter()
                        .map(|term| term.sql(&schema))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let sql = format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL{filter_sql}
                 ORDER BY {order_sql}
                 LIMIT ${limit_index} OFFSET ${offset_index}"
            );
            let mut query = sqlx::query(&sql);
            if let Some(filter) = &filter {
                query = bind_filter(query, &filter.binds);
            }
            let rows = query.bind(limit).bind(offset).fetch_all(&self.pool).await?;
            // Upstream leaves hasPreviousPage false for pure offset paging.
            has_previous_page = false;
            has_next_page = offset + (rows.len() as i64) < total;
            rows
        };

        rows.truncate(limit as usize);

        Ok(PersonPage {
            persons: rows.iter().map(row_to_person).collect(),
            total,
            has_next_page,
            has_previous_page,
        })
    }

    async fn person_created_at(
        &self,
        schema: &str,
        cursor_id: Uuid,
    ) -> anyhow::Result<DateTime<Utc>> {
        let row: Option<(DateTime<Utc>,)> = sqlx::query_as(&format!(
            "SELECT created_at FROM {schema}.person WHERE id = $1"
        ))
        .bind(cursor_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(created_at,)| created_at)
            .ok_or_else(|| anyhow::anyhow!("cursor not found"))
    }

    pub async fn count_persons(&self, workspace_id: Uuid) -> anyhow::Result<i64> {
        self.count_persons_filtered(workspace_id, None).await
    }

    pub async fn count_persons_filtered(
        &self,
        workspace_id: Uuid,
        filter: Option<&FilterExpr>,
    ) -> anyhow::Result<i64> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let filter = filter.map(|filter| compile_filter(filter, &schema, 1));
        let filter_sql = filter
            .as_ref()
            .map(|filter| format!(" AND ({})", filter.sql))
            .unwrap_or_default();
        let sql =
            format!("SELECT count(*) FROM {schema}.person WHERE deleted_at IS NULL{filter_sql}");
        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        if let Some(filter) = &filter {
            query = bind_filter_scalar(query, &filter.binds);
        }
        let count = query.fetch_one(&self.pool).await?;
        Ok(count)
    }

    pub async fn live_person_ids(
        &self,
        workspace_id: Uuid,
        filter: Option<&FilterExpr>,
    ) -> anyhow::Result<Vec<Uuid>> {
        self.person_ids(workspace_id, filter, false).await
    }

    pub async fn deleted_person_ids(
        &self,
        workspace_id: Uuid,
        filter: Option<&FilterExpr>,
    ) -> anyhow::Result<Vec<Uuid>> {
        self.person_ids(workspace_id, filter, true).await
    }

    async fn person_ids(
        &self,
        workspace_id: Uuid,
        filter: Option<&FilterExpr>,
        deleted: bool,
    ) -> anyhow::Result<Vec<Uuid>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let filter = filter.map(|filter| compile_filter(filter, &schema, 1));
        let filter_sql = filter
            .as_ref()
            .map(|filter| format!(" AND ({})", filter.sql))
            .unwrap_or_default();
        let deleted_sql = if deleted {
            "deleted_at IS NOT NULL"
        } else {
            "deleted_at IS NULL"
        };
        let sql = format!(
            "SELECT id FROM {schema}.person WHERE {deleted_sql}{filter_sql} ORDER BY created_at ASC"
        );
        let mut query = sqlx::query(&sql);
        if let Some(filter) = &filter {
            query = bind_filter(query, &filter.binds);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get("id")).collect())
    }

    pub async fn search_records(
        &self,
        workspace_id: Uuid,
        search_input: &str,
        included_object_name_singulars: Option<&[String]>,
        excluded_object_name_singulars: Option<&[String]>,
    ) -> anyhow::Result<Vec<SearchHit>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let pattern = format!(
            "%{}%",
            search_input
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let query_text = search_input.replace(['%', '_'], " ");

        let definitions: &[(&str, &str, &str)] = &[
            (
                "person",
                "Person",
                &format!(
                    "SELECT 'person'::text AS object_name_singular, 'Person'::text AS object_label_singular, id AS record_id, concat_ws(' ', name_first, name_last) AS label, avatar_url AS image_url, ts_rank_cd(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)) AS ts_rank_cd, ts_rank(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)) AS ts_rank, created_at, updated_at FROM {schema}.person WHERE deleted_at IS NULL AND search_vector ILIKE $1"
                ),
            ),
            (
                "company",
                "Company",
                &format!(
                    "SELECT 'company'::text, 'Company'::text, id, coalesce(name, ''), NULL::text, ts_rank_cd(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), ts_rank(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), created_at, updated_at FROM {schema}.company WHERE deleted_at IS NULL AND search_vector ILIKE $1"
                ),
            ),
            (
                "opportunity",
                "Opportunity",
                &format!(
                    "SELECT 'opportunity'::text, 'Opportunity'::text, id, coalesce(name, ''), NULL::text, ts_rank_cd(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), ts_rank(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), created_at, updated_at FROM {schema}.opportunity WHERE deleted_at IS NULL AND search_vector ILIKE $1"
                ),
            ),
            (
                "task",
                "Task",
                &format!(
                    "SELECT 'task'::text, 'Task'::text, id, coalesce(title, ''), NULL::text, ts_rank_cd(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), ts_rank(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), created_at, updated_at FROM {schema}.task WHERE deleted_at IS NULL AND search_vector ILIKE $1"
                ),
            ),
            (
                "note",
                "Note",
                &format!(
                    "SELECT 'note'::text, 'Note'::text, id, coalesce(title, ''), NULL::text, ts_rank_cd(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), ts_rank(to_tsvector('simple', coalesce(search_vector, '')), plainto_tsquery('simple', $2)), created_at, updated_at FROM {schema}.note WHERE deleted_at IS NULL AND search_vector ILIKE $1"
                ),
            ),
        ];

        let included: Option<std::collections::HashSet<&str>> = included_object_name_singulars
            .filter(|names| !names.is_empty())
            .map(|names| names.iter().map(String::as_str).collect());
        let excluded: std::collections::HashSet<&str> = excluded_object_name_singulars
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect();

        let parts: Vec<&str> = definitions
            .iter()
            .filter(|(name, _, _)| {
                included
                    .as_ref()
                    .is_none_or(|included| included.contains(name))
                    && !excluded.contains(name)
            })
            .map(|(_, _, sql)| *sql)
            .collect();
        if parts.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!(
            "SELECT * FROM ({}) hits ORDER BY ts_rank_cd DESC, ts_rank DESC, record_id ASC LIMIT 1000",
            parts.join(" UNION ALL ")
        );
        let rows = sqlx::query(&sql)
            .bind(&pattern)
            .bind(&query_text)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|row| SearchHit {
                record_id: row.get("record_id"),
                object_name_singular: row.get("object_name_singular"),
                object_label_singular: row.get("object_label_singular"),
                label: row.get("label"),
                image_url: row.get("image_url"),
                ts_rank_cd: row.get("ts_rank_cd"),
                ts_rank: row.get("ts_rank"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect())
    }

    pub async fn get_person(
        &self,
        workspace_id: Uuid,
        person_id: Uuid,
    ) -> anyhow::Result<Option<Person>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let row = sqlx::query(&format!(
            "SELECT {PERSON_SELECT} FROM {schema}.person
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(person_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(row_to_person))
    }

    pub async fn create_person(
        &self,
        workspace_id: Uuid,
        input: &CreatePersonInput,
    ) -> anyhow::Result<Person> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let links = input.linkedin_link.clone().unwrap_or_default();
        let phones = input.phones.clone().unwrap_or_default();
        let actor = Actor::default();
        let search_vector = build_search_vector(
            &input.name.first_name,
            &input.name.last_name,
            &input.emails.primary_email,
            input.job_title.as_deref(),
        );
        let avatar_file = match &input.avatar_file {
            Some(items) => Some(serde_json::to_string(
                &self.resolve_file_items(workspace_id, items).await?,
            )?),
            None => None,
        };

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.person
                (name_first, name_last, primary_email, additional_emails, job_title,
                 linkedin_link, phones, avatar_url, avatar_file, position, created_by,
                 updated_by, search_vector, company_id)
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, $8, $9::jsonb, $10,
                     $11::jsonb, $12::jsonb, $13, $14)
             RETURNING {PERSON_SELECT}"
        ))
        .bind(&input.name.first_name)
        .bind(&input.name.last_name)
        .bind(&input.emails.primary_email)
        .bind(&input.emails.additional_emails)
        .bind(&input.job_title)
        .bind(serde_json::to_string(&links)?)
        .bind(serde_json::to_string(&phones)?)
        .bind(input.avatar_url.clone().unwrap_or_default())
        .bind(avatar_file)
        .bind(input.position.unwrap_or(0.0))
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(search_vector)
        .bind(input.company_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row_to_person(&row))
    }

    pub async fn update_person(
        &self,
        workspace_id: Uuid,
        person_id: Uuid,
        input: &UpdatePersonInput,
    ) -> anyhow::Result<Option<Person>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let Some(current) = self.get_person(workspace_id, person_id).await? else {
            return Ok(None);
        };

        let name = input.name.value().cloned().unwrap_or(current.name);
        let emails = input.emails.value().cloned().unwrap_or(current.emails);
        let job_title = match &input.job_title {
            MaybeUndefined::Value(value) => Some(value.clone()),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.job_title.clone(),
        };
        let linkedin_link = match &input.linkedin_link {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.linkedin_link.clone(),
        };
        let phones = match &input.phones {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.phones.clone(),
        };
        let avatar_url = match &input.avatar_url {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => String::new(),
            MaybeUndefined::Undefined => current.avatar_url.clone(),
        };
        let avatar_file = match &input.avatar_file {
            MaybeUndefined::Value(items) => Some(serde_json::to_string(
                &self.resolve_file_items(workspace_id, items).await?,
            )?),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current
                .avatar_file
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        };
        let position = input.position.value().copied().unwrap_or(current.position);
        let company_id = match input.company_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.company_id,
        };
        let search_vector = build_search_vector(
            &name.first_name,
            &name.last_name,
            &emails.primary_email,
            job_title.as_deref(),
        );

        let row = sqlx::query(&format!(
            "UPDATE {schema}.person
             SET name_first = $2, name_last = $3, primary_email = $4,
                 additional_emails = $5, job_title = $6, linkedin_link = $7::jsonb,
                 phones = $8::jsonb, avatar_url = $9, avatar_file = $10::jsonb,
                 position = $11, search_vector = $12, company_id = $13,
                 updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {PERSON_SELECT}"
        ))
        .bind(person_id)
        .bind(&name.first_name)
        .bind(&name.last_name)
        .bind(&emails.primary_email)
        .bind(&emails.additional_emails)
        .bind(&job_title)
        .bind(serde_json::to_string(&linkedin_link)?)
        .bind(serde_json::to_string(&phones)?)
        .bind(&avatar_url)
        .bind(avatar_file)
        .bind(position)
        .bind(search_vector)
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(row_to_person))
    }

    /// Permanently remove a person (upstream `soft_delete=false`).
    pub async fn destroy_person(
        &self,
        workspace_id: Uuid,
        person_id: Uuid,
    ) -> anyhow::Result<bool> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.person WHERE id = $1"))
            .bind(person_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Undo a soft delete.
    pub async fn restore_person(
        &self,
        workspace_id: Uuid,
        person_id: Uuid,
    ) -> anyhow::Result<Option<Person>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.person SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {PERSON_SELECT}"
        ))
        .bind(person_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_person))
    }

    /// Soft-delete, matching upstream's recoverable deletion semantics.
    pub async fn soft_delete_person(
        &self,
        workspace_id: Uuid,
        person_id: Uuid,
    ) -> anyhow::Result<bool> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.person SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(person_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Merge people into the record at `conflict_priority_index`.
    ///
    /// Relations (opportunities, task/note targets) are re-pointed at the
    /// surviving record and the other records are soft-deleted. `dry_run`
    /// returns the merged preview without writing anything.
    pub async fn merge_people(
        &self,
        workspace_id: Uuid,
        ids: &[Uuid],
        conflict_priority_index: usize,
        dry_run: bool,
    ) -> anyhow::Result<Option<Person>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        if ids.is_empty() || conflict_priority_index >= ids.len() {
            anyhow::bail!("conflictPriorityIndex is out of range");
        }
        let mut unique_ids = Vec::with_capacity(ids.len());
        for id in ids {
            if !unique_ids.contains(id) {
                unique_ids.push(*id);
            }
        }
        let target_id = unique_ids[conflict_priority_index];
        let Some(target) = self.get_person(workspace_id, target_id).await? else {
            return Ok(None);
        };
        let mut sources = Vec::new();
        for id in unique_ids.iter().filter(|id| **id != target_id) {
            match self.get_person(workspace_id, *id).await? {
                Some(person) => sources.push(person),
                None => return Ok(None),
            }
        }

        let mut merged = target.clone();
        for source in &sources {
            if merged.name.first_name.is_empty() {
                merged.name.first_name = source.name.first_name.clone();
            }
            if merged.name.last_name.is_empty() {
                merged.name.last_name = source.name.last_name.clone();
            }
            if merged.emails.primary_email.is_empty() {
                merged.emails.primary_email = source.emails.primary_email.clone();
            }
            for email in std::iter::once(&source.emails.primary_email)
                .chain(source.emails.additional_emails.iter())
            {
                if email.is_empty()
                    || email.eq_ignore_ascii_case(&merged.emails.primary_email)
                    || merged
                        .emails
                        .additional_emails
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(email))
                {
                    continue;
                }
                merged.emails.additional_emails.push(email.clone());
            }
            if merged.job_title.is_none() {
                merged.job_title = source.job_title.clone();
            }
            if merged.company_id.is_none() {
                merged.company_id = source.company_id;
            }
            if merged.avatar_url.is_empty() {
                merged.avatar_url = source.avatar_url.clone();
            }
            if merged.linkedin_link.primary_link_url.is_empty() {
                merged.linkedin_link.primary_link_url =
                    source.linkedin_link.primary_link_url.clone();
                merged.linkedin_link.primary_link_label =
                    source.linkedin_link.primary_link_label.clone();
            }
            if merged.phones.primary_phone_number.is_empty() {
                merged.phones.primary_phone_number = source.phones.primary_phone_number.clone();
                merged.phones.primary_phone_country_code =
                    source.phones.primary_phone_country_code.clone();
                merged.phones.primary_phone_calling_code =
                    source.phones.primary_phone_calling_code.clone();
            }
            for phone in source.phones.additional_phones.iter() {
                if !merged.phones.additional_phones.iter().any(|p| p == phone) {
                    merged.phones.additional_phones.push(phone.clone());
                }
            }
        }
        merged.search_vector = build_search_vector(
            &merged.name.first_name,
            &merged.name.last_name,
            &merged.emails.primary_email,
            merged.job_title.as_deref(),
        );
        if dry_run {
            return Ok(Some(merged));
        }

        let mut tx = self.pool.begin().await?;
        for source in &sources {
            let source_id = source.id;
            sqlx::query(&format!(
                "DELETE FROM {schema}.task_target loser
                 USING {schema}.task_target winner
                 WHERE loser.target_person_id = $1 AND winner.target_person_id = $2
                   AND loser.task_id = winner.task_id
                   AND loser.deleted_at IS NULL AND winner.deleted_at IS NULL
                   AND loser.id <> winner.id"
            ))
            .bind(source_id)
            .bind(target_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "UPDATE {schema}.task_target SET target_person_id = $1, updated_at = now()
                 WHERE target_person_id = $2"
            ))
            .bind(target_id)
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "DELETE FROM {schema}.note_target loser
                 USING {schema}.note_target winner
                 WHERE loser.target_person_id = $1 AND winner.target_person_id = $2
                   AND loser.note_id = winner.note_id
                   AND loser.deleted_at IS NULL AND winner.deleted_at IS NULL
                   AND loser.id <> winner.id"
            ))
            .bind(source_id)
            .bind(target_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "UPDATE {schema}.note_target SET target_person_id = $1, updated_at = now()
                 WHERE target_person_id = $2"
            ))
            .bind(target_id)
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "UPDATE {schema}.opportunity SET point_of_contact_id = $1, updated_at = now()
                 WHERE point_of_contact_id = $2"
            ))
            .bind(target_id)
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&format!(
            "UPDATE {schema}.person SET
                name_first = $2, name_last = $3, primary_email = $4,
                additional_emails = $5, job_title = $6, linkedin_link = $7::jsonb,
                phones = $8::jsonb, avatar_url = $9, search_vector = $10,
                company_id = $11, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(target_id)
        .bind(&merged.name.first_name)
        .bind(&merged.name.last_name)
        .bind(&merged.emails.primary_email)
        .bind(&merged.emails.additional_emails)
        .bind(&merged.job_title)
        .bind(serde_json::to_string(&merged.linkedin_link)?)
        .bind(serde_json::to_string(&merged.phones)?)
        .bind(&merged.avatar_url)
        .bind(&merged.search_vector)
        .bind(merged.company_id)
        .execute(&mut *tx)
        .await?;
        for source in &sources {
            sqlx::query(&format!(
                "UPDATE {schema}.person SET deleted_at = now(), updated_at = now()
                 WHERE id = $1 AND deleted_at IS NULL"
            ))
            .bind(source.id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.get_person(workspace_id, target_id).await
    }

    /// Idempotently seed the standard `person` object and its fields.
    pub async fn ensure_person_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, 'person', 'people', 'Person', 'People', NULL, 'IconUser',
                     'blue', 'p', false, true, true, true, true, true, 'SIDE_PANEL',
                     'OPEN', 'ROLE_ACCESS', 'OPEN', false)
             ON CONFLICT (workspace_id, name_singular) DO NOTHING",
        )
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;

        let object_id: Uuid = sqlx::query(
            "SELECT id FROM core.object_metadata
             WHERE workspace_id = $1 AND name_singular = 'person'",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for field in PERSON_METADATA_FIELDS {
            sqlx::query(
                "INSERT INTO core.field_metadata
                    (workspace_id, object_metadata_id, type, name, label, is_system,
                     is_ui_editable, is_nullable, is_unique, is_searchable, is_audit_logged)
                 VALUES ($1, $2, $3, $4, $5, true, true, $6, $7, $8, true)
                 ON CONFLICT (object_metadata_id, name) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(object_id)
            .bind(field.field_type.as_str())
            .bind(field.name)
            .bind(field.label)
            .bind(field.nullable)
            .bind(field.unique)
            .bind(field.searchable)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE core.object_metadata
             SET label_identifier_field_metadata_id = (
                    SELECT id FROM core.field_metadata
                    WHERE object_metadata_id = $1 AND name = 'name'),
                 image_identifier_field_metadata_id = (
                    SELECT id FROM core.field_metadata
                    WHERE object_metadata_id = $1 AND name = 'avatarFile')
             WHERE id = $1
               AND (label_identifier_field_metadata_id IS NULL
                    OR image_identifier_field_metadata_id IS NULL)",
        )
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO core.views
                (workspace_id, name, object_metadata_id, type, key, icon, position,
                 is_compact, is_custom, open_record_in, should_hide_empty_groups,
                 visibility, is_active)
             VALUES ($1, 'All people', $2, 'TABLE', 'INDEX', 'IconList', 0,
                     false, false, 'SIDE_PANEL', false, 'WORKSPACE', true)
             ON CONFLICT (workspace_id, object_metadata_id, key) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        let view_id: Uuid = sqlx::query(
            "SELECT id FROM core.views
             WHERE workspace_id = $1 AND object_metadata_id = $2 AND key = 'INDEX'",
        )
        .bind(workspace_id)
        .bind(object_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for (position, (field_name, size)) in DEFAULT_VIEW_FIELDS.iter().enumerate() {
            sqlx::query(
                "INSERT INTO core.view_fields
                    (workspace_id, view_id, field_metadata_id, position, size)
                 SELECT $1, $2, id, $3, $4 FROM core.field_metadata
                 WHERE object_metadata_id = $5 AND name = $6
                 ON CONFLICT (view_id, field_metadata_id) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(view_id)
            .bind(position as f64)
            .bind(*size)
            .bind(object_id)
            .bind(*field_name)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_webhooks(&self, workspace_id: Uuid) -> anyhow::Result<Vec<WebhookRecord>> {
        let rows = sqlx::query(
            "SELECT id, target_url, operations, description, secret, application_id,
                    created_at, updated_at, deleted_at
             FROM core.webhooks
             WHERE workspace_id = $1 AND deleted_at IS NULL
             ORDER BY created_at ASC, id ASC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_webhook).collect())
    }

    pub async fn get_webhook(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<WebhookRecord>> {
        let row = sqlx::query(
            "SELECT id, target_url, operations, description, secret, application_id,
                    created_at, updated_at, deleted_at
             FROM core.webhooks
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_webhook))
    }

    pub async fn create_webhook(
        &self,
        workspace_id: Uuid,
        id: Option<Uuid>,
        target_url: &str,
        operations: &[String],
        description: Option<&str>,
        secret: Option<&str>,
    ) -> anyhow::Result<WebhookRecord> {
        let secret = secret
            .map(ToString::to_string)
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let row = sqlx::query(
            "INSERT INTO core.webhooks
                (id, workspace_id, target_url, operations, description, secret, application_id)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7)
             RETURNING id, target_url, operations, description, secret, application_id,
                       created_at, updated_at, deleted_at",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(target_url)
        .bind(operations)
        .bind(description)
        .bind(secret)
        .bind(Uuid::nil())
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_webhook(&row))
    }

    pub async fn update_webhook(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        target_url: Option<&str>,
        operations: Option<&[String]>,
        description: Option<&str>,
        secret: Option<&str>,
    ) -> anyhow::Result<Option<WebhookRecord>> {
        let row = sqlx::query(
            "UPDATE core.webhooks SET
                target_url = COALESCE($3, target_url),
                operations = COALESCE($4, operations),
                description = COALESCE($5, description),
                secret = COALESCE($6, secret),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL
             RETURNING id, target_url, operations, description, secret, application_id,
                       created_at, updated_at, deleted_at",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(target_url)
        .bind(operations)
        .bind(description)
        .bind(secret)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_webhook))
    }

    pub async fn delete_webhook(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<WebhookRecord>> {
        let row = sqlx::query(
            "UPDATE core.webhooks SET deleted_at = now(), updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL
             RETURNING id, target_url, operations, description, secret, application_id,
                       created_at, updated_at, deleted_at",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_webhook))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_webhook_delivery(
        &self,
        workspace_id: Uuid,
        webhook_id: Uuid,
        event: &str,
        event_id: Uuid,
        target_url: &str,
        request_body: &[u8],
        signature: &str,
    ) -> anyhow::Result<Uuid> {
        Ok(sqlx::query_scalar(
            "INSERT INTO core.webhook_deliveries
                (workspace_id, webhook_id, event, event_id, target_url, request_body, signature)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(workspace_id)
        .bind(webhook_id)
        .bind(event)
        .bind(event_id)
        .bind(target_url)
        .bind(request_body)
        .bind(signature)
        .fetch_one(&self.pool)
        .await?)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_webhook_delivery(
        &self,
        workspace_id: Uuid,
        delivery_id: Uuid,
        attempts: i32,
        last_status_code: Option<i32>,
        last_error: Option<&str>,
        state: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE core.webhook_deliveries SET
                attempts = $3, last_status_code = $4, last_error = $5, state = $6,
                updated_at = now(), completed_at = CASE WHEN $6 = 'pending' THEN NULL ELSE now() END
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(delivery_id)
        .bind(attempts)
        .bind(last_status_code)
        .bind(last_error)
        .bind(state)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_webhook_deliveries(
        &self,
        workspace_id: Uuid,
        webhook_id: Option<Uuid>,
        state: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<WebhookDeliveryRecord>> {
        let rows = sqlx::query(
            "SELECT id, webhook_id, event, event_id, state, attempts, last_status_code,
                    last_error, target_url, request_body, signature, created_at,
                    updated_at, completed_at
             FROM core.webhook_deliveries
             WHERE workspace_id = $1
               AND ($2::uuid IS NULL OR webhook_id = $2)
               AND ($3::text IS NULL OR state = $3)
             ORDER BY created_at DESC LIMIT $4",
        )
        .bind(workspace_id)
        .bind(webhook_id)
        .bind(state)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_webhook_delivery).collect())
    }

    pub async fn list_replayable_webhook_deliveries(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<(Uuid, Uuid)>> {
        let rows = sqlx::query(
            "SELECT workspace_id, id FROM core.webhook_deliveries
             WHERE state = 'failed' AND attempts < 8
               AND updated_at < now() - interval '1 minute'
             ORDER BY updated_at ASC LIMIT $1",
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| (row.get("workspace_id"), row.get("id")))
            .collect())
    }

    pub async fn get_webhook_delivery(
        &self,
        workspace_id: Uuid,
        delivery_id: Uuid,
    ) -> anyhow::Result<Option<WebhookDeliveryRecord>> {
        let row = sqlx::query(
            "SELECT id, webhook_id, event, event_id, state, attempts, last_status_code,
                    last_error, target_url, request_body, signature, created_at,
                    updated_at, completed_at
             FROM core.webhook_deliveries
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(delivery_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_webhook_delivery))
    }

    pub async fn prune_expired_state(&self) -> anyhow::Result<(u64, u64)> {
        let idempotency =
            sqlx::query("DELETE FROM core.idempotency_keys WHERE expires_at <= now()")
                .execute(&self.pool)
                .await?
                .rows_affected();
        let deliveries = sqlx::query(
            "DELETE FROM core.webhook_deliveries
             WHERE state IN ('succeeded', 'failed') AND updated_at < now() - interval '30 days'",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok((idempotency, deliveries))
    }

    pub async fn enqueue_job(
        &self,
        workspace_id: Option<Uuid>,
        kind: &str,
        payload: &serde_json::Value,
        run_at: DateTime<Utc>,
        max_attempts: i32,
    ) -> anyhow::Result<Uuid> {
        Ok(sqlx::query_scalar(
            "INSERT INTO core.jobs (workspace_id, kind, payload, run_at, max_attempts)
             VALUES ($1, $2, $3::text::jsonb, $4, $5) RETURNING id",
        )
        .bind(workspace_id)
        .bind(kind)
        .bind(payload.to_string())
        .bind(run_at)
        .bind(max_attempts)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn claim_due_jobs(&self, limit: i64) -> anyhow::Result<Vec<JobRecord>> {
        let rows = sqlx::query(
            "UPDATE core.jobs SET state = 'running', attempts = attempts + 1, updated_at = now()
             WHERE id IN (
                 SELECT id FROM core.jobs
                 WHERE state = 'pending' AND run_at <= now()
                 ORDER BY run_at ASC
                 FOR UPDATE SKIP LOCKED
                 LIMIT $1
             )
             RETURNING id, workspace_id, kind, payload, state, attempts, max_attempts,
                       run_at, last_error, created_at, updated_at, completed_at",
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_job).collect())
    }

    pub async fn complete_job(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE core.jobs SET state = 'succeeded', last_error = NULL,
                    updated_at = now(), completed_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn fail_job(&self, id: Uuid, error: &str) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE core.jobs SET
                state = CASE WHEN attempts >= max_attempts THEN 'failed' ELSE 'pending' END,
                last_error = $2,
                run_at = CASE WHEN attempts >= max_attempts THEN run_at
                              ELSE now() + (interval '30 seconds' * attempts) END,
                updated_at = now(),
                completed_at = CASE WHEN attempts >= max_attempts THEN now() ELSE NULL END
             WHERE id = $1",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_jobs(
        &self,
        workspace_id: Option<Uuid>,
        state: Option<&str>,
        limit: i64,
    ) -> anyhow::Result<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, kind, payload, state, attempts, max_attempts,
                    run_at, last_error, created_at, updated_at, completed_at
             FROM core.jobs
             WHERE ($1::uuid IS NULL OR workspace_id = $1)
               AND ($2::text IS NULL OR state = $2)
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(workspace_id)
        .bind(state)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_job).collect())
    }

    pub async fn get_job(&self, id: Uuid) -> anyhow::Result<Option<JobRecord>> {
        let row = sqlx::query(
            "SELECT id, workspace_id, kind, payload, state, attempts, max_attempts,
                    run_at, last_error, created_at, updated_at, completed_at
             FROM core.jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_job))
    }

    pub async fn ensure_roles(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        for (role, label) in [
            (Role::Owner, "Owner"),
            (Role::Admin, "Admin"),
            (Role::Member, "Member"),
        ] {
            let privileged = matches!(role, Role::Owner | Role::Admin);
            let id = Uuid::new_v5(&workspace_id, label.as_bytes());
            sqlx::query(
                "INSERT INTO core.roles
                    (id, workspace_id, label, description, icon, is_editable,
                     can_update_all_settings, can_access_all_tools,
                     can_read_all_object_records, can_update_all_object_records,
                     can_soft_delete_all_object_records, can_destroy_all_object_records,
                     can_be_assigned_to_users, can_be_assigned_to_agents,
                     can_be_assigned_to_api_keys, application_id, universal_identifier)
                 VALUES ($1, $2, $3, NULL, NULL, false, $4, true, true, true, true, $5,
                         true, false, true, $6, $7)
                 ON CONFLICT (workspace_id, label) DO NOTHING",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(label)
            .bind(privileged)
            .bind(privileged)
            .bind(Uuid::nil())
            .bind(workspace_id.to_string())
            .execute(&self.pool)
            .await?;
        }
        for (label, flags) in [
            ("Owner", PERMISSION_FLAG_KEYS),
            ("Admin", PERMISSION_FLAG_KEYS),
            ("Member", MEMBER_PERMISSION_FLAG_KEYS),
        ] {
            let role_id = Uuid::new_v5(&workspace_id, label.as_bytes());
            for flag in flags {
                sqlx::query(
                    "INSERT INTO core.role_permission_flags (workspace_id, role_id, flag)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (role_id, flag) DO NOTHING",
                )
                .bind(workspace_id)
                .bind(role_id)
                .bind(flag)
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            "UPDATE core.workspace_members wm
             SET role_id = r.id
             FROM core.roles r
             WHERE wm.workspace_id = $1 AND wm.role_id IS NULL
               AND r.workspace_id = $1
               AND r.label = CASE wm.role
                   WHEN 'OWNER' THEN 'Owner'
                   WHEN 'ADMIN' THEN 'Admin'
                   ELSE 'Member'
               END",
        )
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE core.api_keys ak
             SET role_id = r.id
             FROM core.roles r
             WHERE ak.workspace_id = $1 AND ak.role_id IS NULL
               AND r.workspace_id = $1
               AND r.label = CASE ak.role
                   WHEN 'OWNER' THEN 'Owner'
                   WHEN 'ADMIN' THEN 'Admin'
                   ELSE 'Member'
               END",
        )
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_roles(&self, workspace_id: Uuid) -> anyhow::Result<Vec<RoleRecord>> {
        self.ensure_roles(workspace_id).await?;
        let rows = sqlx::query(
            "SELECT id, label, description, icon, is_editable, can_update_all_settings,
                    can_access_all_tools, can_read_all_object_records,
                    can_update_all_object_records, can_soft_delete_all_object_records,
                    can_destroy_all_object_records, can_be_assigned_to_users,
                    can_be_assigned_to_agents, can_be_assigned_to_api_keys,
                    application_id, universal_identifier
             FROM core.roles WHERE workspace_id = $1
             ORDER BY created_at ASC, label ASC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        let mut roles: Vec<RoleRecord> = rows.iter().map(row_to_role).collect();
        for role in &mut roles {
            role.permission_flags = self.role_permission_flags(workspace_id, role.id).await?;
        }
        Ok(roles)
    }

    pub async fn role_permission_flags(
        &self,
        workspace_id: Uuid,
        role_id: Uuid,
    ) -> anyhow::Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT flag FROM core.role_permission_flags
             WHERE workspace_id = $1 AND role_id = $2 ORDER BY flag ASC",
        )
        .bind(workspace_id)
        .bind(role_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn set_role_permission_flags(
        &self,
        workspace_id: Uuid,
        role_id: Uuid,
        flags: &[String],
    ) -> anyhow::Result<()> {
        self.ensure_roles(workspace_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM core.role_permission_flags WHERE workspace_id = $1 AND role_id = $2",
        )
        .bind(workspace_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
        for flag in flags {
            if !PERMISSION_FLAG_KEYS.contains(&flag.as_str()) {
                continue;
            }
            sqlx::query(
                "INSERT INTO core.role_permission_flags (workspace_id, role_id, flag)
                 VALUES ($1, $2, $3) ON CONFLICT (role_id, flag) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(role_id)
            .bind(flag)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_role(
        &self,
        workspace_id: Uuid,
        role_id: Uuid,
    ) -> anyhow::Result<Option<RoleRecord>> {
        self.ensure_roles(workspace_id).await?;
        let row = sqlx::query(
            "SELECT id, label, description, icon, is_editable, can_update_all_settings,
                    can_access_all_tools, can_read_all_object_records,
                    can_update_all_object_records, can_soft_delete_all_object_records,
                    can_destroy_all_object_records, can_be_assigned_to_users,
                    can_be_assigned_to_agents, can_be_assigned_to_api_keys,
                    application_id, universal_identifier
             FROM core.roles WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(role_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(mut role) = row.as_ref().map(row_to_role) else {
            return Ok(None);
        };
        role.permission_flags = self.role_permission_flags(workspace_id, role.id).await?;
        Ok(Some(role))
    }

    pub async fn create_role(
        &self,
        workspace_id: Uuid,
        id: Option<Uuid>,
        input: &RoleWrite,
    ) -> anyhow::Result<RoleRecord> {
        self.ensure_roles(workspace_id).await?;
        let id = id.unwrap_or_else(Uuid::new_v4);
        let row = sqlx::query(
            "INSERT INTO core.roles
                (id, workspace_id, label, description, icon, is_editable,
                 can_update_all_settings, can_access_all_tools,
                 can_read_all_object_records, can_update_all_object_records,
                 can_soft_delete_all_object_records, can_destroy_all_object_records,
                 can_be_assigned_to_users, can_be_assigned_to_agents,
                 can_be_assigned_to_api_keys, application_id, universal_identifier)
             VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16)
             RETURNING id, label, description, icon, is_editable, can_update_all_settings,
                       can_access_all_tools, can_read_all_object_records,
                       can_update_all_object_records, can_soft_delete_all_object_records,
                       can_destroy_all_object_records, can_be_assigned_to_users,
                       can_be_assigned_to_agents, can_be_assigned_to_api_keys,
                       application_id, universal_identifier",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(&input.label)
        .bind(&input.description)
        .bind(&input.icon)
        .bind(input.can_update_all_settings)
        .bind(input.can_access_all_tools)
        .bind(input.can_read_all_object_records)
        .bind(input.can_update_all_object_records)
        .bind(input.can_soft_delete_all_object_records)
        .bind(input.can_destroy_all_object_records)
        .bind(input.can_be_assigned_to_users)
        .bind(input.can_be_assigned_to_agents)
        .bind(input.can_be_assigned_to_api_keys)
        .bind(Uuid::nil())
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_role(&row))
    }

    pub async fn update_role(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        patch: &RolePatch,
    ) -> anyhow::Result<Option<RoleRecord>> {
        self.ensure_roles(workspace_id).await?;
        let row = sqlx::query(
            "UPDATE core.roles SET
                label = COALESCE($3, label),
                description = COALESCE($4, description),
                icon = COALESCE($5, icon),
                can_update_all_settings = COALESCE($6, can_update_all_settings),
                can_access_all_tools = COALESCE($7, can_access_all_tools),
                can_read_all_object_records = COALESCE($8, can_read_all_object_records),
                can_update_all_object_records = COALESCE($9, can_update_all_object_records),
                can_soft_delete_all_object_records =
                    COALESCE($10, can_soft_delete_all_object_records),
                can_destroy_all_object_records =
                    COALESCE($11, can_destroy_all_object_records),
                can_be_assigned_to_users = COALESCE($12, can_be_assigned_to_users),
                can_be_assigned_to_agents = COALESCE($13, can_be_assigned_to_agents),
                can_be_assigned_to_api_keys = COALESCE($14, can_be_assigned_to_api_keys),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND is_editable = true
             RETURNING id, label, description, icon, is_editable, can_update_all_settings,
                       can_access_all_tools, can_read_all_object_records,
                       can_update_all_object_records, can_soft_delete_all_object_records,
                       can_destroy_all_object_records, can_be_assigned_to_users,
                       can_be_assigned_to_agents, can_be_assigned_to_api_keys,
                       application_id, universal_identifier",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(&patch.label)
        .bind(&patch.description)
        .bind(&patch.icon)
        .bind(patch.can_update_all_settings)
        .bind(patch.can_access_all_tools)
        .bind(patch.can_read_all_object_records)
        .bind(patch.can_update_all_object_records)
        .bind(patch.can_soft_delete_all_object_records)
        .bind(patch.can_destroy_all_object_records)
        .bind(patch.can_be_assigned_to_users)
        .bind(patch.can_be_assigned_to_agents)
        .bind(patch.can_be_assigned_to_api_keys)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_role))
    }

    pub async fn delete_role(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
        self.ensure_roles(workspace_id).await?;
        let result = sqlx::query(
            "DELETE FROM core.roles
             WHERE workspace_id = $1 AND id = $2 AND is_editable = true",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_workspace_member_role(
        &self,
        workspace_id: Uuid,
        member_id: Uuid,
        role_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_roles(workspace_id).await?;
        let label: Option<String> =
            sqlx::query_scalar("SELECT label FROM core.roles WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(role_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(label) = label else {
            return Ok(false);
        };
        let legacy = match label.as_str() {
            "Owner" => Role::Owner.as_str(),
            "Admin" => Role::Admin.as_str(),
            _ => Role::Member.as_str(),
        };
        let result = sqlx::query(
            "UPDATE core.workspace_members SET role_id = $3, role = $4
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(member_id)
        .bind(role_id)
        .bind(legacy)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn assign_role_to_api_key(
        &self,
        workspace_id: Uuid,
        api_key_id: Uuid,
        role_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_roles(workspace_id).await?;
        let label: Option<String> =
            sqlx::query_scalar("SELECT label FROM core.roles WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(role_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(label) = label else {
            return Ok(false);
        };
        let legacy = match label.as_str() {
            "Owner" => Role::Owner.as_str(),
            "Admin" => Role::Admin.as_str(),
            _ => Role::Member.as_str(),
        };
        let result = sqlx::query(
            "UPDATE core.api_keys SET role_id = $3, role = $4, updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND revoked_at IS NULL",
        )
        .bind(workspace_id)
        .bind(api_key_id)
        .bind(role_id)
        .bind(legacy)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn ensure_standard_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.ensure_roles(workspace_id).await?;
        self.ensure_person_metadata(workspace_id).await?;
        self.ensure_company_metadata(workspace_id).await?;
        self.ensure_opportunity_metadata(workspace_id).await?;
        self.ensure_task_metadata(workspace_id).await?;
        self.ensure_note_metadata(workspace_id).await?;
        self.ensure_task_target_metadata(workspace_id).await?;
        self.ensure_note_target_metadata(workspace_id).await?;
        self.ensure_workspace_member_metadata(workspace_id).await?;
        Ok(())
    }

    pub async fn ensure_company_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, 'company', 'companies', 'Company', 'Companies', NULL,
                     'IconBuildingSkyscraper', 'turquoise', 'c', false, true, true, true,
                     true, true, 'SIDE_PANEL', 'OPEN', 'ROLE_ACCESS', 'OPEN', false)
             ON CONFLICT (workspace_id, name_singular) DO NOTHING",
        )
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;

        let object_id: Uuid = sqlx::query(
            "SELECT id FROM core.object_metadata
             WHERE workspace_id = $1 AND name_singular = 'company'",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for field in COMPANY_METADATA_FIELDS {
            sqlx::query(
                "INSERT INTO core.field_metadata
                    (workspace_id, object_metadata_id, type, name, label, is_system,
                     is_ui_editable, is_nullable, is_unique, is_searchable, is_audit_logged)
                 VALUES ($1, $2, $3, $4, $5, true, true, $6, $7, $8, true)
                 ON CONFLICT (object_metadata_id, name) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(object_id)
            .bind(field.field_type.as_str())
            .bind(field.name)
            .bind(field.label)
            .bind(field.nullable)
            .bind(field.unique)
            .bind(field.searchable)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE core.object_metadata
             SET label_identifier_field_metadata_id = (
                    SELECT id FROM core.field_metadata
                    WHERE object_metadata_id = $1 AND name = 'name')
             WHERE id = $1 AND label_identifier_field_metadata_id IS NULL",
        )
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO core.views
                (workspace_id, name, object_metadata_id, type, key, icon, position,
                 is_compact, is_custom, open_record_in, should_hide_empty_groups,
                 visibility, is_active)
             VALUES ($1, 'All companies', $2, 'TABLE', 'INDEX', 'IconList', 0,
                     false, false, 'SIDE_PANEL', false, 'WORKSPACE', true)
             ON CONFLICT (workspace_id, object_metadata_id, key) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        let view_id: Uuid = sqlx::query(
            "SELECT id FROM core.views
             WHERE workspace_id = $1 AND object_metadata_id = $2 AND key = 'INDEX'",
        )
        .bind(workspace_id)
        .bind(object_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for (position, (field_name, size)) in DEFAULT_COMPANY_VIEW_FIELDS.iter().enumerate() {
            sqlx::query(
                "INSERT INTO core.view_fields
                    (workspace_id, view_id, field_metadata_id, position, size)
                 SELECT $1, $2, id, $3, $4 FROM core.field_metadata
                 WHERE object_metadata_id = $5 AND name = $6
                 ON CONFLICT (view_id, field_metadata_id) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(view_id)
            .bind(position as f64)
            .bind(*size)
            .bind(object_id)
            .bind(*field_name)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn ensure_workspace_company_table(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_COMPANY_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn ensure_opportunity_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, 'opportunity', 'opportunities', 'Opportunity', 'Opportunities', NULL,
                     'IconTargetArrow', 'yellow', 'o', false, true, true, true,
                     true, true, 'SIDE_PANEL', 'OPEN', 'ROLE_ACCESS', 'OPEN', false)
             ON CONFLICT (workspace_id, name_singular) DO NOTHING",
        )
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;

        let object_id: Uuid = sqlx::query(
            "SELECT id FROM core.object_metadata
             WHERE workspace_id = $1 AND name_singular = 'opportunity'",
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for field in OPPORTUNITY_METADATA_FIELDS {
            sqlx::query(
                "INSERT INTO core.field_metadata
                    (workspace_id, object_metadata_id, type, name, label, is_system,
                     is_ui_editable, is_nullable, is_unique, is_searchable, is_audit_logged)
                 VALUES ($1, $2, $3, $4, $5, true, true, $6, $7, $8, true)
                 ON CONFLICT (object_metadata_id, name) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(object_id)
            .bind(field.field_type.as_str())
            .bind(field.name)
            .bind(field.label)
            .bind(field.nullable)
            .bind(field.unique)
            .bind(field.searchable)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE core.object_metadata
             SET label_identifier_field_metadata_id = (
                    SELECT id FROM core.field_metadata
                    WHERE object_metadata_id = $1 AND name = 'name')
             WHERE id = $1 AND label_identifier_field_metadata_id IS NULL",
        )
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO core.views
                (workspace_id, name, object_metadata_id, type, key, icon, position,
                 is_compact, is_custom, open_record_in, should_hide_empty_groups,
                 visibility, is_active)
             VALUES ($1, 'All opportunities', $2, 'TABLE', 'INDEX', 'IconList', 0,
                     false, false, 'SIDE_PANEL', false, 'WORKSPACE', true)
             ON CONFLICT (workspace_id, object_metadata_id, key) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        let view_id: Uuid = sqlx::query(
            "SELECT id FROM core.views
             WHERE workspace_id = $1 AND object_metadata_id = $2 AND key = 'INDEX'",
        )
        .bind(workspace_id)
        .bind(object_id)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        for (position, (field_name, size)) in DEFAULT_OPPORTUNITY_VIEW_FIELDS.iter().enumerate() {
            sqlx::query(
                "INSERT INTO core.view_fields
                    (workspace_id, view_id, field_metadata_id, position, size)
                 SELECT $1, $2, id, $3, $4 FROM core.field_metadata
                 WHERE object_metadata_id = $5 AND name = $6
                 ON CONFLICT (view_id, field_metadata_id) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(view_id)
            .bind(position as f64)
            .bind(*size)
            .bind(object_id)
            .bind(*field_name)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn ensure_workspace_opportunity_table(
        &self,
        workspace_id: Uuid,
    ) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_OPPORTUNITY_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    // ----- opportunities -------------------------------------------------

    pub async fn create_opportunity(
        &self,
        workspace_id: Uuid,
        input: &CreateOpportunityInput,
    ) -> anyhow::Result<Opportunity> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let amount = input.amount.clone().unwrap_or_default();
        let stage = input.stage.unwrap_or_default();
        let actor = Actor::default();
        let search_vector = squid_core::opportunity_search_vector(input.name.as_deref());

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.opportunity
                (name, amount, close_date, stage, position, created_by, updated_by,
                 search_vector, company_id, point_of_contact_id, owner_id)
             VALUES ($1, $2::jsonb, $3, $4, $5, $6::jsonb, $7::jsonb, $8, $9, $10, $11)
             RETURNING {OPPORTUNITY_SELECT}"
        ))
        .bind(&input.name)
        .bind(serde_json::to_string(&amount)?)
        .bind(input.close_date)
        .bind(stage.as_str())
        .bind(input.position.unwrap_or(0.0))
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(search_vector)
        .bind(input.company_id)
        .bind(input.point_of_contact_id)
        .bind(input.owner_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_opportunity(&row))
    }

    pub async fn get_opportunity(
        &self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> anyhow::Result<Option<Opportunity>> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {OPPORTUNITY_SELECT} FROM {schema}.opportunity
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(opportunity_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_opportunity))
    }

    pub async fn update_opportunity(
        &self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        input: &UpdateOpportunityInput,
    ) -> anyhow::Result<Option<Opportunity>> {
        let Some(current) = self.get_opportunity(workspace_id, opportunity_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let name = match &input.name {
            MaybeUndefined::Value(value) => Some(value.clone()),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.name.clone(),
        };
        let amount = match &input.amount {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => current.amount.clone(),
            MaybeUndefined::Undefined => current.amount.clone(),
        };
        let close_date = match input.close_date {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.close_date,
        };
        let stage = input.stage.value().copied().unwrap_or(current.stage);
        let position = input.position.value().copied().unwrap_or(current.position);
        let company_id = match input.company_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.company_id,
        };
        let point_of_contact_id = match input.point_of_contact_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.point_of_contact_id,
        };
        let owner_id = match input.owner_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.owner_id,
        };
        let search_vector = squid_core::opportunity_search_vector(name.as_deref());

        let row = sqlx::query(&format!(
            "UPDATE {schema}.opportunity
             SET name = $2, amount = $3::jsonb, close_date = $4, stage = $5,
                 position = $6, search_vector = $7, company_id = $8,
                 point_of_contact_id = $9, owner_id = $10, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {OPPORTUNITY_SELECT}"
        ))
        .bind(opportunity_id)
        .bind(&name)
        .bind(serde_json::to_string(&amount)?)
        .bind(close_date)
        .bind(stage.as_str())
        .bind(position)
        .bind(search_vector)
        .bind(company_id)
        .bind(point_of_contact_id)
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_opportunity))
    }

    pub async fn soft_delete_opportunity(
        &self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.opportunity SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(opportunity_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn destroy_opportunity(
        &self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.opportunity WHERE id = $1"))
            .bind(opportunity_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_opportunity(
        &self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> anyhow::Result<Option<Opportunity>> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.opportunity SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {OPPORTUNITY_SELECT}"
        ))
        .bind(opportunity_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_opportunity))
    }

    pub async fn list_opportunities(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(Vec<Opportunity>, i64)> {
        self.ensure_workspace_opportunity_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.opportunity WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(&format!(
            "SELECT {OPPORTUNITY_SELECT} FROM {schema}.opportunity
             WHERE deleted_at IS NULL
             ORDER BY name NULLS LAST, created_at ASC
             LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows.iter().map(row_to_opportunity).collect(), total))
    }

    async fn seed_standard_object(
        &self,
        workspace_id: Uuid,
        definition: StandardObjectSeed<'_>,
        table_sql: &str,
    ) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        sqlx::raw_sql(&table_sql.replace("{{schema}}", &schema))
            .execute(&self.pool)
            .await?;

        let is_ui_creatable = definition.name_singular != "workspaceMember";
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, false, true, true, true,
                     $9, true, 'SIDE_PANEL', 'OPEN', 'ROLE_ACCESS', 'OPEN', false)
             ON CONFLICT (workspace_id, name_singular) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(definition.name_singular)
        .bind(definition.name_plural)
        .bind(definition.label_singular)
        .bind(definition.label_plural)
        .bind(definition.icon)
        .bind(definition.color)
        .bind(definition.shortcut)
        .bind(is_ui_creatable)
        .execute(&mut *tx)
        .await?;

        let object_id: Uuid = sqlx::query(
            "SELECT id FROM core.object_metadata
             WHERE workspace_id = $1 AND name_singular = $2",
        )
        .bind(workspace_id)
        .bind(definition.name_singular)
        .fetch_one(&mut *tx)
        .await?
        .get("id");

        let target_morph_id = Uuid::new_v5(&object_id, b"targetMorphId");
        for field in definition.fields {
            let morph_id =
                (field.field_type == FieldMetadataType::MorphRelation).then_some(target_morph_id);
            sqlx::query(
                "INSERT INTO core.field_metadata
                    (workspace_id, object_metadata_id, type, name, label, is_system,
                     is_ui_editable, is_nullable, is_unique, is_searchable, is_audit_logged,
                     morph_id)
                 VALUES ($1, $2, $3, $4, $5, true, true, $6, $7, $8, true, $9)
                 ON CONFLICT (object_metadata_id, name) DO UPDATE
                 SET morph_id = COALESCE(core.field_metadata.morph_id, EXCLUDED.morph_id)",
            )
            .bind(workspace_id)
            .bind(object_id)
            .bind(field.field_type.as_str())
            .bind(field.name)
            .bind(field.label)
            .bind(field.nullable)
            .bind(field.unique)
            .bind(field.searchable)
            .bind(morph_id)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE core.object_metadata
             SET label_identifier_field_metadata_id = (
                    SELECT id FROM core.field_metadata
                    WHERE object_metadata_id = $1 AND name = 'name')
             WHERE id = $1 AND label_identifier_field_metadata_id IS NULL",
        )
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        if definition.create_view {
            sqlx::query(
                "INSERT INTO core.views
                    (workspace_id, name, object_metadata_id, type, key, icon, position,
                     is_compact, is_custom, open_record_in, should_hide_empty_groups,
                     visibility, is_active)
                 VALUES ($1, $2, $3, 'TABLE', 'INDEX', 'IconList', 0,
                         false, false, 'SIDE_PANEL', false, 'WORKSPACE', true)
                 ON CONFLICT (workspace_id, object_metadata_id, key) DO NOTHING",
            )
            .bind(workspace_id)
            .bind(definition.view_name)
            .bind(object_id)
            .execute(&mut *tx)
            .await?;

            let view_id: Uuid = sqlx::query(
                "SELECT id FROM core.views
                 WHERE workspace_id = $1 AND object_metadata_id = $2 AND key = 'INDEX'",
            )
            .bind(workspace_id)
            .bind(object_id)
            .fetch_one(&mut *tx)
            .await?
            .get("id");

            for (position, (field_name, size)) in definition.view_fields.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO core.view_fields
                        (workspace_id, view_id, field_metadata_id, position, size)
                     SELECT $1, $2, id, $3, $4 FROM core.field_metadata
                     WHERE object_metadata_id = $5 AND name = $6
                     ON CONFLICT (view_id, field_metadata_id) DO NOTHING",
                )
                .bind(workspace_id)
                .bind(view_id)
                .bind(position as f64)
                .bind(*size)
                .bind(object_id)
                .bind(*field_name)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn ensure_task_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.seed_standard_object(
            workspace_id,
            StandardObjectSeed {
                name_singular: "task",
                name_plural: "tasks",
                label_singular: "Task",
                label_plural: "Tasks",
                icon: "IconCheckbox",
                color: "green",
                shortcut: "t",
                fields: TASK_METADATA_FIELDS,
                view_name: "All tasks",
                view_fields: DEFAULT_TASK_VIEW_FIELDS,
                create_view: true,
            },
            WORKSPACE_TASK_SQL,
        )
        .await
    }

    pub async fn ensure_note_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.seed_standard_object(
            workspace_id,
            StandardObjectSeed {
                name_singular: "note",
                name_plural: "notes",
                label_singular: "Note",
                label_plural: "Notes",
                icon: "IconNotes",
                color: "blue",
                shortcut: "n",
                fields: NOTE_METADATA_FIELDS,
                view_name: "All notes",
                view_fields: DEFAULT_NOTE_VIEW_FIELDS,
                create_view: true,
            },
            WORKSPACE_NOTE_SQL,
        )
        .await
    }

    pub async fn ensure_task_target_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.seed_standard_object(
            workspace_id,
            StandardObjectSeed {
                name_singular: "taskTarget",
                name_plural: "taskTargets",
                label_singular: "Task target",
                label_plural: "Task targets",
                icon: "IconTarget",
                color: "turquoise",
                shortcut: "tt",
                fields: TASK_TARGET_METADATA_FIELDS,
                view_name: "",
                view_fields: &[],
                create_view: false,
            },
            WORKSPACE_TASK_TARGET_SQL,
        )
        .await
    }

    pub async fn ensure_note_target_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.seed_standard_object(
            workspace_id,
            StandardObjectSeed {
                name_singular: "noteTarget",
                name_plural: "noteTargets",
                label_singular: "Note target",
                label_plural: "Note targets",
                icon: "IconTarget",
                color: "turquoise",
                shortcut: "nt",
                fields: NOTE_TARGET_METADATA_FIELDS,
                view_name: "",
                view_fields: &[],
                create_view: false,
            },
            WORKSPACE_NOTE_TARGET_SQL,
        )
        .await
    }

    pub async fn ensure_workspace_member_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.seed_standard_object(
            workspace_id,
            StandardObjectSeed {
                name_singular: "workspaceMember",
                name_plural: "workspaceMembers",
                label_singular: "Workspace Member",
                label_plural: "Workspace Members",
                icon: "IconUserCircle",
                color: "blue",
                shortcut: "",
                fields: WORKSPACE_MEMBER_METADATA_FIELDS,
                view_name: "",
                view_fields: &[],
                create_view: false,
            },
            "SELECT 1;",
        )
        .await
    }

    // ----- tasks ---------------------------------------------------------

    pub async fn create_task(
        &self,
        workspace_id: Uuid,
        input: &CreateTaskInput,
    ) -> anyhow::Result<Task> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let body = input.body_v2.clone().unwrap_or_default();
        let status = input.status.unwrap_or_default();
        let actor = Actor::default();
        let search_vector = input.title.as_ref().map(|title| title.to_lowercase());

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.task
                (position, title, body_v2, due_at, status, created_by, updated_by,
                 search_vector, assignee_id)
             VALUES ($1, $2, $3::jsonb, $4, $5, $6::jsonb, $7::jsonb, $8, $9)
             RETURNING {TASK_SELECT}"
        ))
        .bind(input.position.unwrap_or(0.0))
        .bind(&input.title)
        .bind(serde_json::to_string(&body)?)
        .bind(input.due_at)
        .bind(status.as_str())
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(search_vector)
        .bind(input.assignee_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_task(&row))
    }

    pub async fn get_task(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> anyhow::Result<Option<Task>> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {TASK_SELECT} FROM {schema}.task
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task))
    }

    pub async fn update_task(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        input: &UpdateTaskInput,
    ) -> anyhow::Result<Option<Task>> {
        let Some(current) = self.get_task(workspace_id, task_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let position = input.position.value().copied().unwrap_or(current.position);
        let title = match &input.title {
            MaybeUndefined::Value(value) => Some(value.clone()),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.title.clone(),
        };
        let body = match &input.body_v2 {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.body_v2.clone(),
        };
        let due_at = match input.due_at {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.due_at,
        };
        let status = input
            .status
            .value()
            .copied()
            .unwrap_or_else(|| current.status.unwrap_or_default());
        let assignee_id = match input.assignee_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.assignee_id,
        };
        let search_vector = title.as_ref().map(|title| title.to_lowercase());

        let row = sqlx::query(&format!(
            "UPDATE {schema}.task
             SET position = $2, title = $3, body_v2 = $4::jsonb, due_at = $5,
                 status = $6, assignee_id = $7, search_vector = $8, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {TASK_SELECT}"
        ))
        .bind(task_id)
        .bind(position)
        .bind(&title)
        .bind(serde_json::to_string(&body)?)
        .bind(due_at)
        .bind(status.as_str())
        .bind(assignee_id)
        .bind(search_vector)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task))
    }

    pub async fn soft_delete_task(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.task SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn destroy_task(&self, workspace_id: Uuid, task_id: Uuid) -> anyhow::Result<bool> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.task WHERE id = $1"))
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_task(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> anyhow::Result<Option<Task>> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.task SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {TASK_SELECT}"
        ))
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task))
    }

    pub async fn list_tasks(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(Vec<Task>, i64)> {
        self.ensure_workspace_task_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.task WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(&format!(
            "SELECT {TASK_SELECT} FROM {schema}.task
             WHERE deleted_at IS NULL
             ORDER BY position ASC, created_at ASC
             LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows.iter().map(row_to_task).collect(), total))
    }

    pub async fn ensure_workspace_task_table(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_TASK_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    // ----- notes ---------------------------------------------------------

    pub async fn create_note(
        &self,
        workspace_id: Uuid,
        input: &CreateNoteInput,
    ) -> anyhow::Result<Note> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let body = input.body_v2.clone().unwrap_or_default();
        let actor = Actor::default();
        let search_vector = input.title.as_ref().map(|title| title.to_lowercase());

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.note
                (position, title, body_v2, created_by, updated_by, search_vector)
             VALUES ($1, $2, $3::jsonb, $4::jsonb, $5::jsonb, $6)
             RETURNING {NOTE_SELECT}"
        ))
        .bind(input.position.unwrap_or(0.0))
        .bind(&input.title)
        .bind(serde_json::to_string(&body)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(search_vector)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_note(&row))
    }

    pub async fn get_note(
        &self,
        workspace_id: Uuid,
        note_id: Uuid,
    ) -> anyhow::Result<Option<Note>> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {NOTE_SELECT} FROM {schema}.note
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(note_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note))
    }

    pub async fn update_note(
        &self,
        workspace_id: Uuid,
        note_id: Uuid,
        input: &UpdateNoteInput,
    ) -> anyhow::Result<Option<Note>> {
        let Some(current) = self.get_note(workspace_id, note_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let position = input.position.value().copied().unwrap_or(current.position);
        let title = match &input.title {
            MaybeUndefined::Value(value) => Some(value.clone()),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.title.clone(),
        };
        let body = match &input.body_v2 {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.body_v2.clone(),
        };
        let search_vector = title.as_ref().map(|title| title.to_lowercase());

        let row = sqlx::query(&format!(
            "UPDATE {schema}.note
             SET position = $2, title = $3, body_v2 = $4::jsonb,
                 search_vector = $5, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {NOTE_SELECT}"
        ))
        .bind(note_id)
        .bind(position)
        .bind(&title)
        .bind(serde_json::to_string(&body)?)
        .bind(search_vector)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note))
    }

    pub async fn soft_delete_note(
        &self,
        workspace_id: Uuid,
        note_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.note SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(note_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn destroy_note(&self, workspace_id: Uuid, note_id: Uuid) -> anyhow::Result<bool> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.note WHERE id = $1"))
            .bind(note_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_note(
        &self,
        workspace_id: Uuid,
        note_id: Uuid,
    ) -> anyhow::Result<Option<Note>> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.note SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {NOTE_SELECT}"
        ))
        .bind(note_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note))
    }

    pub async fn list_notes(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(Vec<Note>, i64)> {
        self.ensure_workspace_note_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.note WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(&format!(
            "SELECT {NOTE_SELECT} FROM {schema}.note
             WHERE deleted_at IS NULL
             ORDER BY position ASC, created_at ASC
             LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows.iter().map(row_to_note).collect(), total))
    }

    pub async fn ensure_workspace_note_table(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_NOTE_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    // ----- task targets --------------------------------------------------

    pub async fn create_task_target(
        &self,
        workspace_id: Uuid,
        input: &CreateTaskTargetInput,
    ) -> anyhow::Result<TaskTarget> {
        self.ensure_workspace_task_table(workspace_id).await?;
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let actor = Actor::default();
        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.task_target
                (created_by, updated_by, task_id, target_company_id,
                 target_opportunity_id, target_person_id)
             VALUES ($1::jsonb, $2::jsonb, $3, $4, $5, $6)
             RETURNING {TASK_TARGET_SELECT}"
        ))
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(input.task_id)
        .bind(input.target_company_id)
        .bind(input.target_opportunity_id)
        .bind(input.target_person_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_task_target(&row))
    }

    pub async fn get_task_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<Option<TaskTarget>> {
        self.ensure_workspace_task_table(workspace_id).await?;
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {TASK_TARGET_SELECT} FROM {schema}.task_target
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task_target))
    }

    pub async fn update_task_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
        input: &UpdateTaskTargetInput,
    ) -> anyhow::Result<Option<TaskTarget>> {
        let Some(current) = self.get_task_target(workspace_id, target_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let position = input.position.value().copied().unwrap_or(current.position);
        let company = match input.target_company_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_company_id,
        };
        let opportunity = match input.target_opportunity_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_opportunity_id,
        };
        let person = match input.target_person_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_person_id,
        };
        let row = sqlx::query(&format!(
            "UPDATE {schema}.task_target
             SET position = $2, target_company_id = $3, target_opportunity_id = $4,
                 target_person_id = $5, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {TASK_TARGET_SELECT}"
        ))
        .bind(target_id)
        .bind(position)
        .bind(company)
        .bind(opportunity)
        .bind(person)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task_target))
    }

    pub async fn destroy_task_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.task_target WHERE id = $1"))
            .bind(target_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_task_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<Option<TaskTarget>> {
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.task_target SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {TASK_TARGET_SELECT}"
        ))
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_task_target))
    }

    pub async fn list_task_targets(
        &self,
        workspace_id: Uuid,
        task_id: Option<Uuid>,
        company_id: Option<Uuid>,
        opportunity_id: Option<Uuid>,
        person_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<TaskTarget>> {
        self.ensure_workspace_task_table(workspace_id).await?;
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let rows = sqlx::query(&format!(
            "SELECT {TASK_TARGET_SELECT} FROM {schema}.task_target
             WHERE deleted_at IS NULL
               AND ($1::uuid IS NULL OR task_id = $1)
               AND ($2::uuid IS NULL OR target_company_id = $2)
               AND ($3::uuid IS NULL OR target_opportunity_id = $3)
               AND ($4::uuid IS NULL OR target_person_id = $4)
             ORDER BY position ASC, created_at ASC"
        ))
        .bind(task_id)
        .bind(company_id)
        .bind(opportunity_id)
        .bind(person_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_task_target).collect())
    }

    pub async fn delete_task_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_task_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.task_target SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(target_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn ensure_workspace_task_target_table(
        &self,
        workspace_id: Uuid,
    ) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_TASK_TARGET_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    // ----- note targets --------------------------------------------------

    pub async fn create_note_target(
        &self,
        workspace_id: Uuid,
        input: &CreateNoteTargetInput,
    ) -> anyhow::Result<NoteTarget> {
        self.ensure_workspace_note_table(workspace_id).await?;
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let actor = Actor::default();
        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.note_target
                (created_by, updated_by, note_id, target_company_id,
                 target_opportunity_id, target_person_id)
             VALUES ($1::jsonb, $2::jsonb, $3, $4, $5, $6)
             RETURNING {NOTE_TARGET_SELECT}"
        ))
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(input.note_id)
        .bind(input.target_company_id)
        .bind(input.target_opportunity_id)
        .bind(input.target_person_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_note_target(&row))
    }

    pub async fn get_note_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<Option<NoteTarget>> {
        self.ensure_workspace_note_table(workspace_id).await?;
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {NOTE_TARGET_SELECT} FROM {schema}.note_target
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note_target))
    }

    pub async fn update_note_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
        input: &UpdateNoteTargetInput,
    ) -> anyhow::Result<Option<NoteTarget>> {
        let Some(current) = self.get_note_target(workspace_id, target_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let position = input.position.value().copied().unwrap_or(current.position);
        let company = match input.target_company_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_company_id,
        };
        let opportunity = match input.target_opportunity_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_opportunity_id,
        };
        let person = match input.target_person_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.target_person_id,
        };
        let row = sqlx::query(&format!(
            "UPDATE {schema}.note_target
             SET position = $2, target_company_id = $3, target_opportunity_id = $4,
                 target_person_id = $5, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {NOTE_TARGET_SELECT}"
        ))
        .bind(target_id)
        .bind(position)
        .bind(company)
        .bind(opportunity)
        .bind(person)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note_target))
    }

    pub async fn destroy_note_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.note_target WHERE id = $1"))
            .bind(target_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_note_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<Option<NoteTarget>> {
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.note_target SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {NOTE_TARGET_SELECT}"
        ))
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_note_target))
    }

    pub async fn list_note_targets(
        &self,
        workspace_id: Uuid,
        note_id: Option<Uuid>,
        company_id: Option<Uuid>,
        opportunity_id: Option<Uuid>,
        person_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<NoteTarget>> {
        self.ensure_workspace_note_table(workspace_id).await?;
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let rows = sqlx::query(&format!(
            "SELECT {NOTE_TARGET_SELECT} FROM {schema}.note_target
             WHERE deleted_at IS NULL
               AND ($1::uuid IS NULL OR note_id = $1)
               AND ($2::uuid IS NULL OR target_company_id = $2)
               AND ($3::uuid IS NULL OR target_opportunity_id = $3)
               AND ($4::uuid IS NULL OR target_person_id = $4)
             ORDER BY position ASC, created_at ASC"
        ))
        .bind(note_id)
        .bind(company_id)
        .bind(opportunity_id)
        .bind(person_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_note_target).collect())
    }

    pub async fn delete_note_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_note_target_table(workspace_id)
            .await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.note_target SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(target_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn ensure_workspace_note_target_table(
        &self,
        workspace_id: Uuid,
    ) -> anyhow::Result<()> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let ddl = WORKSPACE_NOTE_TARGET_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        Ok(())
    }

    // ----- companies -----------------------------------------------------

    pub async fn create_company(
        &self,
        workspace_id: Uuid,
        input: &CreateCompanyInput,
    ) -> anyhow::Result<Company> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let domain = input.domain_name.clone().unwrap_or_default();
        let address = input.address.clone().unwrap_or_default();
        let linkedin = input.linkedin_link.clone().unwrap_or_default();
        let revenue = input.annual_revenue.clone().unwrap_or_default();
        let actor = Actor::default();
        let search_vector = squid_core::company_search_vector(input.name.as_deref(), &domain);

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.company
                (name, domain_name, address, linkedin_link, annual_revenue, position,
                 created_by, updated_by, search_vector, account_owner_id)
             VALUES ($1, $2::jsonb, $3::jsonb, $4::jsonb, $5::jsonb, $6,
                     $7::jsonb, $8::jsonb, $9, $10)
             RETURNING {COMPANY_SELECT}"
        ))
        .bind(&input.name)
        .bind(serde_json::to_string(&domain)?)
        .bind(serde_json::to_string(&address)?)
        .bind(serde_json::to_string(&linkedin)?)
        .bind(serde_json::to_string(&revenue)?)
        .bind(input.position.unwrap_or(0.0))
        .bind(serde_json::to_string(&actor)?)
        .bind(serde_json::to_string(&actor)?)
        .bind(search_vector)
        .bind(input.account_owner_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_company(&row))
    }

    pub async fn get_company(
        &self,
        workspace_id: Uuid,
        company_id: Uuid,
    ) -> anyhow::Result<Option<Company>> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "SELECT {COMPANY_SELECT} FROM {schema}.company
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_company))
    }

    pub async fn update_company(
        &self,
        workspace_id: Uuid,
        company_id: Uuid,
        input: &UpdateCompanyInput,
    ) -> anyhow::Result<Option<Company>> {
        let Some(current) = self.get_company(workspace_id, company_id).await? else {
            return Ok(None);
        };
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let name = match &input.name {
            MaybeUndefined::Value(value) => Some(value.clone()),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.name.clone(),
        };
        let domain = match &input.domain_name {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.domain_name.clone(),
        };
        let address = match &input.address {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.address.clone(),
        };
        let linkedin = match &input.linkedin_link {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.linkedin_link.clone(),
        };
        let revenue = match &input.annual_revenue {
            MaybeUndefined::Value(value) => value.clone(),
            MaybeUndefined::Null => Default::default(),
            MaybeUndefined::Undefined => current.annual_revenue.clone(),
        };
        let position = input.position.value().copied().unwrap_or(current.position);
        let owner = match input.account_owner_id {
            MaybeUndefined::Value(value) => Some(value),
            MaybeUndefined::Null => None,
            MaybeUndefined::Undefined => current.account_owner_id,
        };
        let search_vector = squid_core::company_search_vector(name.as_deref(), &domain);

        let row = sqlx::query(&format!(
            "UPDATE {schema}.company
             SET name = $2, domain_name = $3::jsonb, address = $4::jsonb,
                 linkedin_link = $5::jsonb, annual_revenue = $6::jsonb, position = $7,
                 search_vector = $8, account_owner_id = $9, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL
             RETURNING {COMPANY_SELECT}"
        ))
        .bind(company_id)
        .bind(&name)
        .bind(serde_json::to_string(&domain)?)
        .bind(serde_json::to_string(&address)?)
        .bind(serde_json::to_string(&linkedin)?)
        .bind(serde_json::to_string(&revenue)?)
        .bind(position)
        .bind(search_vector)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_company))
    }

    pub async fn soft_delete_company(
        &self,
        workspace_id: Uuid,
        company_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!(
            "UPDATE {schema}.company SET deleted_at = now(), updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(company_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn destroy_company(
        &self,
        workspace_id: Uuid,
        company_id: Uuid,
    ) -> anyhow::Result<bool> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let result = sqlx::query(&format!("DELETE FROM {schema}.company WHERE id = $1"))
            .bind(company_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn restore_company(
        &self,
        workspace_id: Uuid,
        company_id: Uuid,
    ) -> anyhow::Result<Option<Company>> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let row = sqlx::query(&format!(
            "UPDATE {schema}.company SET deleted_at = NULL, updated_at = now()
             WHERE id = $1 AND deleted_at IS NOT NULL
             RETURNING {COMPANY_SELECT}"
        ))
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_company))
    }

    pub async fn list_companies(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(Vec<Company>, i64)> {
        self.ensure_workspace_company_table(workspace_id).await?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.company WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(&format!(
            "SELECT {COMPANY_SELECT} FROM {schema}.company
             WHERE deleted_at IS NULL
             ORDER BY name NULLS LAST, created_at ASC
             LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows.iter().map(row_to_company).collect(), total))
    }

    pub async fn list_views(
        &self,
        workspace_id: Uuid,
        object_id: Option<Uuid>,
        view_types: Option<&[String]>,
    ) -> anyhow::Result<Vec<View>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_SELECT} FROM core.views
             WHERE workspace_id = $1 AND deleted_at IS NULL
               AND ($2::uuid IS NULL OR object_metadata_id = $2)
             ORDER BY position ASC, created_at ASC"
        ))
        .bind(workspace_id)
        .bind(object_id)
        .fetch_all(&self.pool)
        .await?;

        let mut views = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut view = row_to_view(row)?;
            if let Some(types) = view_types {
                if !types.iter().any(|kind| kind == view.view_type.as_str()) {
                    continue;
                }
            }
            self.load_view_relations(workspace_id, &mut view).await?;
            views.push(view);
        }
        Ok(views)
    }

    pub async fn get_view(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<Option<View>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {VIEW_SELECT} FROM core.views
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut view = row_to_view(&row)?;
        self.load_view_relations(workspace_id, &mut view).await?;
        Ok(Some(view))
    }

    pub async fn list_view_fields_public(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewField>> {
        self.list_view_fields(workspace_id, view_id).await
    }

    async fn list_view_fields(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewField>> {
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_FIELD_SELECT} FROM core.view_fields
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY position ASC, created_at ASC"
        ))
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_view_field).collect()
    }

    pub async fn list_objects(&self, workspace_id: Uuid) -> anyhow::Result<Vec<ObjectMetadata>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {OBJECT_SELECT} FROM core.object_metadata
             WHERE workspace_id = $1 ORDER BY created_at ASC, name_singular ASC"
        ))
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;

        let mut objects = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut object = row_to_object(row)?;
            object.fields = self.list_fields(workspace_id, Some(object.id)).await?;
            object.index_metadata = self.list_indexes(workspace_id, Some(object.id)).await?;
            objects.push(object);
        }
        self.attach_metadata_links(workspace_id, &mut objects)
            .await?;
        Ok(objects)
    }

    pub async fn find_object_by_name(
        &self,
        workspace_id: Uuid,
        name: &str,
    ) -> anyhow::Result<Option<ObjectMetadata>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let object_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM core.object_metadata
             WHERE workspace_id = $1 AND (name_singular = $2 OR name_plural = $2)
             LIMIT 1",
        )
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        let Some(object_id) = object_id else {
            return Ok(None);
        };
        self.get_object(workspace_id, object_id).await
    }

    pub async fn list_custom_records(
        &self,
        workspace_id: Uuid,
        object_name: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<(Vec<serde_json::Value>, i64)> {
        assert_valid_identifier(object_name)?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let table = quote_identifier(object_name);
        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.{table} WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query(&format!(
            "SELECT to_jsonb(t) AS record FROM {schema}.{table} t
             WHERE deleted_at IS NULL ORDER BY created_at ASC, id ASC LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok((rows.iter().map(|row| row.get("record")).collect(), total))
    }

    pub async fn get_custom_record(
        &self,
        workspace_id: Uuid,
        object_name: &str,
        id: Uuid,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        assert_valid_identifier(object_name)?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let table = quote_identifier(object_name);
        let row = sqlx::query(&format!(
            "SELECT to_jsonb(t) AS record FROM {schema}.{table} t
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("record")))
    }

    pub async fn create_custom_record(
        &self,
        workspace_id: Uuid,
        object_name: &str,
        values: &[(String, FieldMetadataType, serde_json::Value)],
    ) -> anyhow::Result<serde_json::Value> {
        assert_valid_identifier(object_name)?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let table = quote_identifier(object_name);
        let mut columns = Vec::new();
        let mut expressions = Vec::new();
        let mut binds = Vec::new();
        for (index, (name, field_type, value)) in values.iter().enumerate() {
            assert_valid_identifier(name)?;
            columns.push(quote_identifier(name));
            let placeholder = format!("${}", index + 1);
            expressions.push(custom_field_expression(&placeholder, *field_type));
            binds.push(value.to_string());
        }
        if columns.is_empty() {
            anyhow::bail!("no fields supplied");
        }
        let sql = format!(
            "INSERT INTO {schema}.{table} ({}) VALUES ({}) RETURNING to_jsonb({table}) AS record",
            columns.join(", "),
            expressions.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for bind in binds {
            query = query.bind(bind);
        }
        let row = query.fetch_one(&self.pool).await?;
        Ok(row.get("record"))
    }

    pub async fn update_custom_record(
        &self,
        workspace_id: Uuid,
        object_name: &str,
        id: Uuid,
        values: &[(String, FieldMetadataType, serde_json::Value)],
    ) -> anyhow::Result<Option<serde_json::Value>> {
        assert_valid_identifier(object_name)?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let table = quote_identifier(object_name);
        if values.is_empty() {
            return self.get_custom_record(workspace_id, object_name, id).await;
        }
        let mut assignments = Vec::new();
        let mut binds = Vec::new();
        for (index, (name, field_type, value)) in values.iter().enumerate() {
            assert_valid_identifier(name)?;
            let placeholder = format!("${}", index + 2);
            assignments.push(format!(
                "{} = {}",
                quote_identifier(name),
                custom_field_expression(&placeholder, *field_type)
            ));
            binds.push(value.to_string());
        }
        let sql = format!(
            "UPDATE {schema}.{table} SET {}, updated_at = now()
             WHERE id = $1 AND deleted_at IS NULL RETURNING to_jsonb({table}) AS record",
            assignments.join(", ")
        );
        let mut query = sqlx::query(&sql).bind(id);
        for bind in binds {
            query = query.bind(bind);
        }
        let row = query.fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("record")))
    }

    pub async fn delete_custom_record(
        &self,
        workspace_id: Uuid,
        object_name: &str,
        id: Uuid,
        soft: bool,
    ) -> anyhow::Result<bool> {
        assert_valid_identifier(object_name)?;
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let table = quote_identifier(object_name);
        let result = if soft {
            sqlx::query(&format!(
                "UPDATE {schema}.{table} SET deleted_at = now(), updated_at = now()
                 WHERE id = $1 AND deleted_at IS NULL"
            ))
            .bind(id)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(&format!("DELETE FROM {schema}.{table} WHERE id = $1"))
                .bind(id)
                .execute(&self.pool)
                .await?
        };
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> anyhow::Result<Option<ObjectMetadata>> {
        Ok(self
            .list_objects(workspace_id)
            .await?
            .into_iter()
            .find(|object| object.id == object_id))
    }

    async fn attach_metadata_links(
        &self,
        workspace_id: Uuid,
        objects: &mut [ObjectMetadata],
    ) -> anyhow::Result<()> {
        let object_rows = sqlx::query(
            "SELECT id, name_singular, name_plural FROM core.object_metadata
             WHERE workspace_id = $1",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        let summaries: std::collections::HashMap<String, ObjectSummary> = object_rows
            .iter()
            .map(|row| {
                let summary = ObjectSummary {
                    id: row.get("id"),
                    name_singular: row.get("name_singular"),
                    name_plural: row.get("name_plural"),
                };
                (summary.name_singular.clone(), summary)
            })
            .collect();

        let field_rows = sqlx::query(
            "SELECT id, object_metadata_id, name FROM core.field_metadata
             WHERE workspace_id = $1",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        let field_ids: std::collections::HashMap<(Uuid, String), Uuid> = field_rows
            .iter()
            .map(|row| {
                (
                    (row.get("object_metadata_id"), row.get("name")),
                    row.get("id"),
                )
            })
            .collect();
        let field_summaries: std::collections::HashMap<(Uuid, Uuid), FieldSummary> = field_rows
            .iter()
            .map(|row| {
                let object_id: Uuid = row.get("object_metadata_id");
                let field = FieldSummary {
                    id: row.get("id"),
                    name: row.get("name"),
                };
                ((object_id, field.id), field)
            })
            .collect();

        let mut reverse_relations: std::collections::HashMap<(Uuid, Uuid), RelationMetadata> =
            std::collections::HashMap::new();
        for object in objects.iter_mut() {
            object.search_field_metadata = build_search_field_metadata(object);
            let source_name = object.name_singular.clone();
            let Some(source_summary) = summaries.get(&source_name).cloned() else {
                continue;
            };
            for field in object.fields.iter_mut() {
                let Some((_, _, target_name, reverse_name)) =
                    METADATA_RELATIONS
                        .iter()
                        .find(|(source, field_name, _, _)| {
                            *source == source_name && *field_name == field.name
                        })
                else {
                    continue;
                };
                let Some(target_summary) = summaries.get(*target_name) else {
                    continue;
                };
                let target_field_id = field_ids
                    .get(&(target_summary.id, (*reverse_name).to_string()))
                    .copied()
                    .unwrap_or_else(|| {
                        Uuid::new_v5(
                            &target_summary.id,
                            format!("relation-reverse:{reverse_name}").as_bytes(),
                        )
                    });
                let source_field = FieldSummary {
                    id: field.id,
                    name: field.name.clone(),
                };
                field.relation = Some(RelationMetadata {
                    relation_type: RelationType::ManyToOne,
                    source_object_metadata: source_summary.clone(),
                    target_object_metadata: target_summary.clone(),
                    source_field_metadata: source_field.clone(),
                    target_field_metadata: FieldSummary {
                        id: target_field_id,
                        name: (*reverse_name).to_string(),
                    },
                });
                reverse_relations.insert(
                    (target_summary.id, target_field_id),
                    RelationMetadata {
                        relation_type: RelationType::OneToMany,
                        source_object_metadata: target_summary.clone(),
                        target_object_metadata: source_summary.clone(),
                        source_field_metadata: FieldSummary {
                            id: target_field_id,
                            name: (*reverse_name).to_string(),
                        },
                        target_field_metadata: source_field,
                    },
                );
            }
        }

        for object in objects.iter_mut() {
            for field in object.fields.iter_mut() {
                if let Some(relation) = reverse_relations.get(&(object.id, field.id)) {
                    field.relation = Some(relation.clone());
                }
            }
        }

        for object in objects.iter_mut() {
            let Some(source_summary) = summaries.get(&object.name_singular).cloned() else {
                continue;
            };
            for field in object.fields.iter_mut() {
                if field.field_type != FieldMetadataType::Relation {
                    continue;
                }
                let Some(settings) = field
                    .settings
                    .as_ref()
                    .and_then(|value| value.0.as_object())
                else {
                    continue;
                };
                let Some(target_object_id) = settings
                    .get("targetObjectMetadataId")
                    .and_then(|value| value.as_str())
                    .and_then(|value| Uuid::parse_str(value).ok())
                else {
                    continue;
                };
                let Some(target_field_id) = settings
                    .get("targetFieldMetadataId")
                    .and_then(|value| value.as_str())
                    .and_then(|value| Uuid::parse_str(value).ok())
                else {
                    continue;
                };
                let Some(target_summary) = summaries
                    .values()
                    .find(|summary| summary.id == target_object_id)
                    .cloned()
                else {
                    continue;
                };
                let Some(target_field) = field_summaries
                    .get(&(target_object_id, target_field_id))
                    .cloned()
                else {
                    continue;
                };
                let relation_type = match settings
                    .get("relationType")
                    .and_then(|value| value.as_str())
                    .unwrap_or("MANY_TO_ONE")
                {
                    "ONE_TO_MANY" => RelationType::OneToMany,
                    _ => RelationType::ManyToOne,
                };
                field.relation = Some(RelationMetadata {
                    relation_type,
                    source_object_metadata: source_summary.clone(),
                    target_object_metadata: target_summary,
                    source_field_metadata: FieldSummary {
                        id: field.id,
                        name: field.name.clone(),
                    },
                    target_field_metadata: target_field,
                });
            }
        }

        for object in objects.iter_mut() {
            let mut by_morph: std::collections::HashMap<Uuid, Vec<RelationMetadata>> =
                std::collections::HashMap::new();
            for field in &object.fields {
                if let (Some(morph_id), Some(relation)) = (field.morph_id, field.relation.clone()) {
                    by_morph.entry(morph_id).or_default().push(relation);
                }
            }
            for field in object.fields.iter_mut() {
                if let Some(morph_id) = field.morph_id {
                    field.morph_relations = by_morph.get(&morph_id).cloned().unwrap_or_default();
                }
            }
        }
        Ok(())
    }

    pub async fn list_fields(
        &self,
        workspace_id: Uuid,
        object_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<FieldMetadata>> {
        let rows = sqlx::query(&format!(
            "SELECT {FIELD_SELECT} FROM core.field_metadata
             WHERE workspace_id = $1 AND ($2::uuid IS NULL OR object_metadata_id = $2)
             ORDER BY created_at ASC, name ASC"
        ))
        .bind(workspace_id)
        .bind(object_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_field).collect()
    }

    pub async fn get_field(
        &self,
        workspace_id: Uuid,
        field_id: Uuid,
    ) -> anyhow::Result<Option<FieldMetadata>> {
        let row = sqlx::query(&format!(
            "SELECT {FIELD_SELECT} FROM core.field_metadata
             WHERE workspace_id = $1 AND id = $2"
        ))
        .bind(workspace_id)
        .bind(field_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_field).transpose()
    }

    // ----- indexes ------------------------------------------------------

    pub async fn list_indexes(
        &self,
        workspace_id: Uuid,
        object_metadata_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<IndexMetadata>> {
        let rows = sqlx::query(
            "SELECT * FROM core.index_metadata
             WHERE workspace_id = $1
               AND ($2::uuid IS NULL OR object_metadata_id = $2)
             ORDER BY created_at ASC, name ASC",
        )
        .bind(workspace_id)
        .bind(object_metadata_id)
        .fetch_all(&self.pool)
        .await?;
        let mut indexes = Vec::with_capacity(rows.len());
        for row in rows {
            let mut index = row_to_index(&row);
            index.index_field_metadata_list = self.index_fields(workspace_id, index.id).await?;
            indexes.push(index);
        }
        Ok(indexes)
    }

    async fn index_fields(
        &self,
        workspace_id: Uuid,
        index_id: Uuid,
    ) -> anyhow::Result<Vec<IndexFieldMetadata>> {
        let rows = sqlx::query(
            "SELECT * FROM core.index_field_metadata
             WHERE workspace_id = $1 AND index_metadata_id = $2
             ORDER BY position ASC",
        )
        .bind(workspace_id)
        .bind(index_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_index_field).collect())
    }

    pub async fn create_index(
        &self,
        workspace_id: Uuid,
        input: &CreateIndexInput,
    ) -> anyhow::Result<IndexMetadata> {
        let object = self
            .get_object(workspace_id, input.object_metadata_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Object not found"))?;
        let suffix = Uuid::new_v4().simple().to_string();
        let name = format!(
            "IDX_{}_{}_{}",
            object.name_singular.to_uppercase(),
            input.fields.len(),
            &suffix[..12]
        );
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO core.index_metadata
                (workspace_id, object_metadata_id, name, is_custom, is_unique, index_type)
             VALUES ($1, $2, $3, TRUE, FALSE, $4)
             RETURNING *",
        )
        .bind(workspace_id)
        .bind(input.object_metadata_id)
        .bind(&name)
        .bind(input.index_type.as_str())
        .fetch_one(&mut *tx)
        .await?;
        let index = row_to_index(&row);
        for (position, field) in input.fields.iter().enumerate() {
            self.get_field(workspace_id, field.field_metadata_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Field not found"))?;
            sqlx::query(
                "INSERT INTO core.index_field_metadata
                    (workspace_id, index_metadata_id, field_metadata_id, position,
                     sub_field_name)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(workspace_id)
            .bind(index.id)
            .bind(field.field_metadata_id)
            .bind(position as f64)
            .bind(&field.sub_field_name)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        let mut index = index;
        index.index_field_metadata_list = self.index_fields(workspace_id, index.id).await?;
        Ok(index)
    }

    pub async fn delete_index(
        &self,
        workspace_id: Uuid,
        index_id: Uuid,
    ) -> anyhow::Result<Option<IndexMetadata>> {
        let mut index = self
            .list_indexes(workspace_id, None)
            .await?
            .into_iter()
            .find(|index| index.id == index_id);
        let Some(current) = index.as_mut() else {
            return Ok(None);
        };
        current.index_field_metadata_list = self.index_fields(workspace_id, index_id).await?;
        let result =
            sqlx::query("DELETE FROM core.index_metadata WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(index_id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        Ok(index)
    }

    // ----- files --------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn create_file(
        &self,
        workspace_id: Uuid,
        filename: &str,
        size: i64,
        content_type: &str,
        file_folder: FileFolder,
        field_metadata_id: Option<Uuid>,
    ) -> anyhow::Result<FileRecord> {
        let id = Uuid::new_v4();
        let folder = file_folder.as_str();
        let path = format!("{folder}/{id}/{filename}");
        let row = sqlx::query(
            "INSERT INTO core.files
                (id, workspace_id, filename, path, size, content_type, file_folder,
                 field_metadata_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING *",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(filename)
        .bind(&path)
        .bind(size)
        .bind(content_type)
        .bind(folder)
        .bind(field_metadata_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_file(&row))
    }

    pub async fn get_file(
        &self,
        workspace_id: Uuid,
        file_id: Uuid,
    ) -> anyhow::Result<Option<FileRecord>> {
        let row = sqlx::query("SELECT * FROM core.files WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(row_to_file))
    }

    pub async fn store_file_content(&self, file_id: Uuid, content: &[u8]) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE core.files
             SET content = $2, size = $3, uploaded = TRUE, updated_at = now()
             WHERE id = $1",
        )
        .bind(file_id)
        .bind(content)
        .bind(content.len() as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_file_uploaded(&self, file_id: Uuid, size: i64) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE core.files SET uploaded = TRUE, size = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(file_id)
        .bind(size)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn complete_file(
        &self,
        workspace_id: Uuid,
        file_id: Uuid,
    ) -> anyhow::Result<Option<FileRecord>> {
        let row = sqlx::query(
            "UPDATE core.files
             SET uploaded = TRUE, updated_at = now()
             WHERE workspace_id = $1 AND id = $2
             RETURNING *",
        )
        .bind(workspace_id)
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_file))
    }

    pub async fn file_content(
        &self,
        file_id: Uuid,
    ) -> anyhow::Result<Option<(FileRecord, Vec<u8>)>> {
        let row = sqlx::query("SELECT * FROM core.files WHERE id = $1")
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| {
            let content: Option<Vec<u8>> = row.get("content");
            (row_to_file(&row), content.unwrap_or_default())
        }))
    }

    pub async fn resolve_file_items(
        &self,
        workspace_id: Uuid,
        items: &[FileItemInput],
    ) -> anyhow::Result<Vec<FileObject>> {
        let mut objects = Vec::with_capacity(items.len());
        for item in items {
            let file = self
                .get_file(workspace_id, item.file_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("File not found: {}", item.file_id))?;
            objects.push(FileObject {
                file_id: file.id,
                label: item.label.clone(),
                extension: file.extension(),
                url: file.url(),
            });
        }
        Ok(objects)
    }

    pub async fn create_metadata_object(
        &self,
        workspace_id: Uuid,
        input: &CreateObjectMetadataInput,
    ) -> anyhow::Result<ObjectMetadata> {
        let row = sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, true, false, true, true,
                     false, 'USER_CHOICE', 'OPEN', 'ROLE_ACCESS', 'OPEN', $11)
             RETURNING id",
        )
        .bind(workspace_id)
        .bind(&input.name_singular)
        .bind(&input.name_plural)
        .bind(&input.label_singular)
        .bind(&input.label_plural)
        .bind(&input.description)
        .bind(&input.icon)
        .bind(&input.color)
        .bind(&input.shortcut)
        .bind(input.is_remote)
        .bind(input.is_label_synced_with_name)
        .fetch_one(&self.pool)
        .await?;
        let object_id: Uuid = row.get("id");
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        assert_valid_identifier(&input.name_singular)?;
        let table = quote_identifier(&input.name_singular);
        let ddl = format!(
            "CREATE TABLE IF NOT EXISTS {schema}.{table} (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                deleted_at TIMESTAMPTZ,
                name TEXT,
                position DOUBLE PRECISION NOT NULL DEFAULT 0,
                created_by JSONB NOT NULL DEFAULT '{{}}'::jsonb,
                updated_by JSONB NOT NULL DEFAULT '{{}}'::jsonb,
                search_vector TEXT
            )"
        );
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;

        if !input.skip_name_field {
            let field = CreateFieldMetadataInput {
                field_type: FieldMetadataType::Text,
                name: "name".to_string(),
                label: "Name".to_string(),
                description: None,
                icon: None,
                is_active: Some(true),
                is_system: Some(false),
                is_ui_editable: Some(true),
                is_ui_read_only: None,
                is_nullable: Some(true),
                is_unique: Some(false),
                is_searchable: Some(false),
                is_audit_logged: Some(true),
                default_value: None,
                options: None,
                settings: None,
                object_metadata_id: object_id,
                is_label_synced_with_name: Some(false),
                is_remote_creation: None,
                relation_creation_payload: None,
                morph_relations_creation_payload: None,
            };
            let name_field = self.create_metadata_field(workspace_id, &field).await?;
            sqlx::query(
                "UPDATE core.object_metadata
                 SET label_identifier_field_metadata_id = $3, updated_at = now()
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(object_id)
            .bind(name_field.id)
            .execute(&self.pool)
            .await?;
        }

        self.get_object(workspace_id, object_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created object not found"))
    }

    pub async fn update_metadata_object(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateObjectMetadataInput,
    ) -> anyhow::Result<Option<ObjectMetadata>> {
        let result = sqlx::query(
            "UPDATE core.object_metadata SET
                label_singular = COALESCE($3, label_singular),
                label_plural = COALESCE($4, label_plural),
                name_singular = COALESCE($5, name_singular),
                name_plural = COALESCE($6, name_plural),
                description = COALESCE($7, description),
                icon = COALESCE($8, icon),
                shortcut = COALESCE($9, shortcut),
                color = COALESCE($10, color),
                is_active = COALESCE($11, is_active),
                label_identifier_field_metadata_id =
                    COALESCE($12, label_identifier_field_metadata_id),
                image_identifier_field_metadata_id =
                    COALESCE($13, image_identifier_field_metadata_id),
                is_label_synced_with_name =
                    COALESCE($14, is_label_synced_with_name),
                is_searchable = COALESCE($15, is_searchable),
                open_record_in = COALESCE($16, open_record_in),
                readability = COALESCE($17, readability),
                sharing_reach = COALESCE($18, sharing_reach),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(&input.label_singular)
        .bind(&input.label_plural)
        .bind(&input.name_singular)
        .bind(&input.name_plural)
        .bind(&input.description)
        .bind(&input.icon)
        .bind(&input.shortcut)
        .bind(&input.color)
        .bind(input.is_active)
        .bind(input.label_identifier_field_metadata_id)
        .bind(input.image_identifier_field_metadata_id)
        .bind(input.is_label_synced_with_name)
        .bind(input.is_searchable)
        .bind(input.open_record_in.map(|value| value.as_str()))
        .bind(input.readability.map(|value| value.as_str()))
        .bind(input.sharing_reach.map(|value| value.as_str()))
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_object(workspace_id, id).await
    }

    pub async fn delete_metadata_object(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<bool> {
        let object_name = object_singular_name(&self.pool, workspace_id, id).await?;
        let result =
            sqlx::query("DELETE FROM core.object_metadata WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() > 0 {
            if let Some(object_name) = object_name {
                assert_valid_identifier(&object_name)?;
                let schema = Self::workspace_schema_name(workspace_id);
                assert_valid_schema(&schema)?;
                let table = quote_identifier(&object_name);
                sqlx::raw_sql(&format!("DROP TABLE IF EXISTS {schema}.{table} CASCADE"))
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(result.rows_affected() > 0)
    }

    pub async fn create_metadata_field(
        &self,
        workspace_id: Uuid,
        input: &CreateFieldMetadataInput,
    ) -> anyhow::Result<FieldMetadata> {
        let default_value = input.default_value.as_ref().map(|value| value.0.clone());
        let options = input.options.as_ref().map(|value| value.0.clone());
        let settings = input.settings.as_ref().map(|value| value.0.clone());
        let row = sqlx::query(
            "INSERT INTO core.field_metadata
                (workspace_id, object_metadata_id, type, name, label, description, icon,
                 is_active, is_system, is_ui_editable, writability, is_nullable, is_unique,
                 is_searchable, is_audit_logged, default_value, options, settings,
                 is_label_synced_with_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'OPEN', $11, $12, $13, $14,
                     $15, $16, $17, $18)
             RETURNING id",
        )
        .bind(workspace_id)
        .bind(input.object_metadata_id)
        .bind(input.field_type.as_str())
        .bind(&input.name)
        .bind(&input.label)
        .bind(&input.description)
        .bind(&input.icon)
        .bind(input.is_active.unwrap_or(true))
        .bind(input.is_system.unwrap_or(false))
        .bind(input.is_ui_editable.unwrap_or(true))
        .bind(input.is_nullable.unwrap_or(true))
        .bind(input.is_unique.unwrap_or(false))
        .bind(input.is_searchable.unwrap_or(false))
        .bind(input.is_audit_logged.unwrap_or(true))
        .bind(default_value)
        .bind(options)
        .bind(settings)
        .bind(input.is_label_synced_with_name.unwrap_or(false))
        .fetch_one(&self.pool)
        .await?;
        let field_id: Uuid = row.get("id");
        if let Some(object_name) =
            object_singular_name(&self.pool, workspace_id, input.object_metadata_id).await?
        {
            assert_valid_identifier(&object_name)?;
            assert_valid_identifier(&input.name)?;
            let schema = Self::workspace_schema_name(workspace_id);
            assert_valid_schema(&schema)?;
            let table = quote_identifier(&object_name);
            let column = quote_identifier(&input.name);
            let ddl = format!(
                "ALTER TABLE {schema}.{table} ADD COLUMN IF NOT EXISTS {column} {}",
                field_column_type(input.field_type)
            );
            sqlx::raw_sql(&ddl).execute(&self.pool).await?;
        }
        self.get_field(workspace_id, field_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created field not found"))
    }

    pub async fn update_metadata_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateFieldMetadataInput,
    ) -> anyhow::Result<Option<FieldMetadata>> {
        let existing = sqlx::query(
            "SELECT f.name AS field_name, o.name_singular AS object_name
             FROM core.field_metadata f
             JOIN core.object_metadata o ON o.id = f.object_metadata_id
             WHERE f.workspace_id = $1 AND f.id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let universal_identifier = input
            .universal_identifier
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let default_value = input.default_value.as_ref().map(|value| value.0.clone());
        let options = input.options.as_ref().map(|value| value.0.clone());
        let settings = input.settings.as_ref().map(|value| value.0.clone());
        let result = sqlx::query(
            "UPDATE core.field_metadata SET
                universal_identifier = COALESCE($3, universal_identifier),
                name = COALESCE($4, name),
                label = COALESCE($5, label),
                description = COALESCE($6, description),
                icon = COALESCE($7, icon),
                is_active = COALESCE($8, is_active),
                is_system = COALESCE($9, is_system),
                is_ui_editable = COALESCE($10, is_ui_editable),
                is_nullable = COALESCE($11, is_nullable),
                is_unique = COALESCE($12, is_unique),
                is_searchable = COALESCE($13, is_searchable),
                is_audit_logged = COALESCE($14, is_audit_logged),
                default_value = COALESCE($15, default_value),
                options = COALESCE($16, options),
                settings = COALESCE($17, settings),
                object_metadata_id = COALESCE($18, object_metadata_id),
                is_label_synced_with_name = COALESCE($19, is_label_synced_with_name),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(universal_identifier)
        .bind(&input.name)
        .bind(&input.label)
        .bind(&input.description)
        .bind(&input.icon)
        .bind(input.is_active)
        .bind(input.is_system)
        .bind(input.is_ui_editable)
        .bind(input.is_nullable)
        .bind(input.is_unique)
        .bind(input.is_searchable)
        .bind(input.is_audit_logged)
        .bind(default_value)
        .bind(options)
        .bind(settings)
        .bind(input.object_metadata_id)
        .bind(input.is_label_synced_with_name)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        if let (Some(existing), Some(new_name)) = (existing, input.name.as_deref()) {
            let old_name: String = existing.get("field_name");
            let object_name: String = existing.get("object_name");
            if old_name != new_name {
                assert_valid_identifier(&old_name)?;
                assert_valid_identifier(new_name)?;
                assert_valid_identifier(&object_name)?;
                let schema = Self::workspace_schema_name(workspace_id);
                assert_valid_schema(&schema)?;
                sqlx::raw_sql(&format!(
                    "ALTER TABLE {schema}.{} RENAME COLUMN {} TO {}",
                    quote_identifier(&object_name),
                    quote_identifier(&old_name),
                    quote_identifier(new_name)
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        self.get_field(workspace_id, id).await
    }

    pub async fn set_field_settings(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        settings: &serde_json::Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE core.field_metadata SET settings = $3::text::jsonb, updated_at = now()
             WHERE workspace_id = $1 AND id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(settings.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_metadata_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<bool> {
        let field = sqlx::query(
            "SELECT f.name AS field_name, o.name_singular AS object_name
             FROM core.field_metadata f
             JOIN core.object_metadata o ON o.id = f.object_metadata_id
             WHERE f.workspace_id = $1 AND f.id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE core.object_metadata
             SET label_identifier_field_metadata_id = NULL
             WHERE workspace_id = $1 AND label_identifier_field_metadata_id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE core.object_metadata
             SET image_identifier_field_metadata_id = NULL
             WHERE workspace_id = $1 AND image_identifier_field_metadata_id = $2",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        let result =
            sqlx::query("DELETE FROM core.field_metadata WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        tx.commit().await?;
        if result.rows_affected() > 0 {
            if let Some(field) = field {
                let field_name: String = field.get("field_name");
                let object_name: String = field.get("object_name");
                assert_valid_identifier(&field_name)?;
                assert_valid_identifier(&object_name)?;
                let schema = Self::workspace_schema_name(workspace_id);
                assert_valid_schema(&schema)?;
                sqlx::raw_sql(&format!(
                    "ALTER TABLE {schema}.{} DROP COLUMN IF EXISTS {}",
                    quote_identifier(&object_name),
                    quote_identifier(&field_name)
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(result.rows_affected() > 0)
    }

    async fn load_view_relations(&self, workspace_id: Uuid, view: &mut View) -> anyhow::Result<()> {
        view.view_fields = self.list_view_fields(workspace_id, view.id).await?;
        view.view_filters = self.list_view_filters(workspace_id, view.id).await?;
        view.view_sorts = self.list_view_sorts(workspace_id, view.id).await?;
        view.view_groups = self.list_view_groups(workspace_id, view.id).await?;
        view.view_filter_groups = self.list_view_filter_groups(workspace_id, view.id).await?;
        view.view_field_groups = self.list_view_field_groups(workspace_id, view.id).await?;
        Ok(())
    }

    pub async fn list_view_filters(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewFilter>> {
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_FILTER_SELECT} FROM core.view_filters
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY created_at ASC"
        ))
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_view_filter).collect()
    }

    pub async fn list_view_sorts(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewSort>> {
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_SORT_SELECT} FROM core.view_sorts
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY created_at ASC"
        ))
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_view_sort).collect()
    }

    pub async fn list_view_groups(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewGroup>> {
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_GROUP_SELECT} FROM core.view_groups
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY position ASC, created_at ASC"
        ))
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_view_group).collect()
    }

    pub async fn list_view_filter_groups(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewFilterGroup>> {
        let rows = sqlx::query(&format!(
            "SELECT {VIEW_FILTER_GROUP_SELECT} FROM core.view_filter_groups
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY created_at ASC"
        ))
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_view_filter_group).collect()
    }

    pub async fn list_view_field_groups(
        &self,
        workspace_id: Uuid,
        view_id: Uuid,
    ) -> anyhow::Result<Vec<ViewFieldGroup>> {
        let rows = sqlx::query(
            "SELECT id, name, position, is_visible, view_id, workspace_id, created_at,
                    updated_at, is_active, deleted_at
             FROM core.view_field_groups
             WHERE workspace_id = $1 AND view_id = $2 AND deleted_at IS NULL
             ORDER BY position ASC, created_at ASC",
        )
        .bind(workspace_id)
        .bind(view_id)
        .fetch_all(&self.pool)
        .await?;
        let fields = self.list_view_fields(workspace_id, view_id).await?;
        let mut groups = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            groups.push(ViewFieldGroup {
                id,
                name: row.get("name"),
                position: row.get("position"),
                is_visible: row.get("is_visible"),
                view_id: row.get("view_id"),
                workspace_id: row.get("workspace_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                is_active: row.get("is_active"),
                deleted_at: row.get("deleted_at"),
                view_fields: fields
                    .iter()
                    .filter(|field| field.view_field_group_id == Some(id))
                    .cloned()
                    .collect(),
                is_overridden: false,
            });
        }
        Ok(groups)
    }

    pub async fn create_view(
        &self,
        workspace_id: Uuid,
        input: &CreateViewInput,
    ) -> anyhow::Result<View> {
        let row = sqlx::query(
            "INSERT INTO core.views
                (id, workspace_id, name, object_metadata_id, type, key, icon, position,
                 is_compact, is_custom, open_record_in, kanban_aggregate_operation,
                 kanban_aggregate_operation_field_metadata_id, main_group_by_field_metadata_id,
                 should_hide_empty_groups, kanban_column_width, group_load_limit,
                 calendar_field_metadata_id, calendar_end_field_metadata_id, calendar_layout,
                 visibility, any_field_filter_value, is_active)
             VALUES
                (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7, $8, $9, true, $10,
                 $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, true)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(&input.name)
        .bind(input.object_metadata_id)
        .bind(input.view_type.unwrap_or(ViewType::Table).as_str())
        .bind(input.key.map(|value| value.as_str()))
        .bind(&input.icon)
        .bind(input.position.unwrap_or(0.0))
        .bind(input.is_compact.unwrap_or(false))
        .bind(
            input
                .open_record_in
                .unwrap_or(ViewOpenRecordIn::SidePanel)
                .as_str(),
        )
        .bind(input.kanban_aggregate_operation.map(|value| value.as_str()))
        .bind(input.kanban_aggregate_operation_field_metadata_id)
        .bind(input.main_group_by_field_metadata_id)
        .bind(input.should_hide_empty_groups.unwrap_or(false))
        .bind(input.kanban_column_width)
        .bind(input.group_load_limit)
        .bind(input.calendar_field_metadata_id)
        .bind(input.calendar_end_field_metadata_id)
        .bind(input.calendar_layout.map(|value| value.as_str()))
        .bind(
            input
                .visibility
                .unwrap_or(ViewVisibility::Workspace)
                .as_str(),
        )
        .bind(&input.any_field_filter_value)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view not found"))
    }

    pub async fn update_view(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewInput,
    ) -> anyhow::Result<Option<View>> {
        fn defined<T>(value: &MaybeUndefined<T>) -> bool {
            !matches!(value, MaybeUndefined::Undefined)
        }
        fn value<T: Clone>(value: &MaybeUndefined<T>) -> Option<T> {
            match value {
                MaybeUndefined::Value(value) => Some(value.clone()),
                MaybeUndefined::Undefined | MaybeUndefined::Null => None,
            }
        }

        let mut query = QueryBuilder::<Postgres>::new("UPDATE core.views SET ");
        let mut first = true;
        macro_rules! set {
            ($column:literal, $defined:expr, $value:expr) => {
                if $defined {
                    if !first {
                        query.push(", ");
                    }
                    query.push(concat!($column, " = ")).push_bind($value);
                    first = false;
                }
            };
        }

        set!("name", defined(&input.name), value(&input.name));
        set!(
            "type",
            defined(&input.view_type),
            value(&input.view_type).map(|value| value.as_str().to_string())
        );
        set!("icon", defined(&input.icon), value(&input.icon));
        set!("position", defined(&input.position), value(&input.position));
        set!(
            "is_compact",
            defined(&input.is_compact),
            value(&input.is_compact)
        );
        set!(
            "open_record_in",
            defined(&input.open_record_in),
            value(&input.open_record_in).map(|value| value.as_str().to_string())
        );
        set!(
            "kanban_aggregate_operation",
            defined(&input.kanban_aggregate_operation),
            value(&input.kanban_aggregate_operation).map(|value| value.as_str().to_string())
        );
        set!(
            "kanban_aggregate_operation_field_metadata_id",
            defined(&input.kanban_aggregate_operation_field_metadata_id),
            value(&input.kanban_aggregate_operation_field_metadata_id)
        );
        set!(
            "any_field_filter_value",
            defined(&input.any_field_filter_value),
            value(&input.any_field_filter_value)
        );
        set!(
            "calendar_layout",
            defined(&input.calendar_layout),
            value(&input.calendar_layout).map(|value| value.as_str().to_string())
        );
        set!(
            "calendar_field_metadata_id",
            defined(&input.calendar_field_metadata_id),
            value(&input.calendar_field_metadata_id)
        );
        set!(
            "calendar_end_field_metadata_id",
            defined(&input.calendar_end_field_metadata_id),
            value(&input.calendar_end_field_metadata_id)
        );
        set!(
            "visibility",
            defined(&input.visibility),
            value(&input.visibility).map(|value| value.as_str().to_string())
        );
        set!(
            "main_group_by_field_metadata_id",
            defined(&input.main_group_by_field_metadata_id),
            value(&input.main_group_by_field_metadata_id)
        );
        set!(
            "should_hide_empty_groups",
            defined(&input.should_hide_empty_groups),
            value(&input.should_hide_empty_groups)
        );
        set!(
            "kanban_column_width",
            defined(&input.kanban_column_width),
            value(&input.kanban_column_width)
        );
        set!(
            "group_load_limit",
            defined(&input.group_load_limit),
            value(&input.group_load_limit)
        );

        if first {
            return self.get_view(workspace_id, id).await;
        }

        query.push(", updated_at = now() WHERE workspace_id = ");
        query.push_bind(workspace_id);
        query.push(" AND id = ");
        query.push_bind(id);
        query.push(" AND deleted_at IS NULL");

        let result = query.build().execute(&self.pool).await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view(workspace_id, id).await
    }

    pub async fn delete_view(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE core.views SET deleted_at = now(), updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn destroy_view(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM core.views WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_view_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewField>> {
        let row = sqlx::query(&format!(
            "SELECT {VIEW_FIELD_SELECT} FROM core.view_fields
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_view_field).transpose()
    }

    pub async fn create_view_field(
        &self,
        workspace_id: Uuid,
        input: &CreateViewFieldInput,
    ) -> anyhow::Result<ViewField> {
        let row = sqlx::query(
            "INSERT INTO core.view_fields
                (id, workspace_id, view_id, field_metadata_id, is_visible, size, position,
                 aggregate_operation, view_field_group_id, is_active)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7, $8, $9, true)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(input.field_metadata_id)
        .bind(input.is_visible.unwrap_or(true))
        .bind(input.size.unwrap_or(0.0))
        .bind(input.position.unwrap_or(0.0))
        .bind(input.aggregate_operation.map(|value| value.as_str()))
        .bind(input.view_field_group_id)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_field(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view field not found"))
    }

    pub async fn update_view_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewFieldInput,
    ) -> anyhow::Result<Option<ViewField>> {
        let result = sqlx::query(
            "UPDATE core.view_fields SET
                is_visible = COALESCE($3, is_visible),
                size = COALESCE($4, size),
                position = COALESCE($5, position),
                aggregate_operation = COALESCE($6, aggregate_operation),
                view_field_group_id = COALESCE($7, view_field_group_id),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(input.is_visible)
        .bind(input.size)
        .bind(input.position)
        .bind(input.aggregate_operation.map(|value| value.as_str()))
        .bind(input.view_field_group_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_field(workspace_id, id).await
    }

    pub async fn delete_view_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewField>> {
        let field = self.get_view_field(workspace_id, id).await?;
        if field.is_some() {
            sqlx::query(
                "UPDATE core.view_fields SET deleted_at = now(), is_active = false,
                    updated_at = now()
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(field)
    }

    pub async fn destroy_view_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewField>> {
        let field = self.get_view_field(workspace_id, id).await?;
        sqlx::query("DELETE FROM core.view_fields WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(field)
    }

    async fn get_view_filter(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFilter>> {
        let row = sqlx::query(&format!(
            "SELECT {VIEW_FILTER_SELECT} FROM core.view_filters
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_view_filter).transpose()
    }

    pub async fn create_view_filter(
        &self,
        workspace_id: Uuid,
        input: &CreateViewFilterInput,
    ) -> anyhow::Result<ViewFilter> {
        let row = sqlx::query(
            "INSERT INTO core.view_filters
                (id, workspace_id, view_id, field_metadata_id, operand, value,
                 view_filter_group_id, position_in_view_filter_group, sub_field_name,
                 relation_target_field_metadata_id)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(input.field_metadata_id)
        .bind(input.operand.unwrap_or(ViewFilterOperand::Is).as_str())
        .bind(input.value.0.clone())
        .bind(input.view_filter_group_id)
        .bind(input.position_in_view_filter_group)
        .bind(&input.sub_field_name)
        .bind(input.relation_target_field_metadata_id)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_filter(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view filter not found"))
    }

    pub async fn update_view_filter(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewFilterInput,
    ) -> anyhow::Result<Option<ViewFilter>> {
        let result = sqlx::query(
            "UPDATE core.view_filters SET
                field_metadata_id = COALESCE($3, field_metadata_id),
                operand = COALESCE($4, operand),
                value = COALESCE($5, value),
                view_filter_group_id = COALESCE($6, view_filter_group_id),
                position_in_view_filter_group =
                    COALESCE($7, position_in_view_filter_group),
                sub_field_name = COALESCE($8, sub_field_name),
                relation_target_field_metadata_id =
                    COALESCE($9, relation_target_field_metadata_id),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(input.field_metadata_id)
        .bind(input.operand.map(|value| value.as_str()))
        .bind(input.value.as_ref().map(|value| value.0.clone()))
        .bind(input.view_filter_group_id)
        .bind(input.position_in_view_filter_group)
        .bind(&input.sub_field_name)
        .bind(input.relation_target_field_metadata_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_filter(workspace_id, id).await
    }

    pub async fn delete_view_filter(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFilter>> {
        let filter = self.get_view_filter(workspace_id, id).await?;
        if filter.is_some() {
            sqlx::query(
                "UPDATE core.view_filters SET deleted_at = now(), updated_at = now()
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(filter)
    }

    pub async fn destroy_view_filter(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFilter>> {
        let filter = self.get_view_filter(workspace_id, id).await?;
        sqlx::query("DELETE FROM core.view_filters WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(filter)
    }

    async fn get_view_sort(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewSort>> {
        let row = sqlx::query(&format!(
            "SELECT {VIEW_SORT_SELECT} FROM core.view_sorts
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_view_sort).transpose()
    }

    pub async fn create_view_sort(
        &self,
        workspace_id: Uuid,
        input: &CreateViewSortInput,
    ) -> anyhow::Result<ViewSort> {
        let row = sqlx::query(
            "INSERT INTO core.view_sorts
                (id, workspace_id, view_id, field_metadata_id, direction, sub_field_name)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(input.field_metadata_id)
        .bind(input.direction.unwrap_or(ViewSortDirection::Asc).as_str())
        .bind(&input.sub_field_name)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_sort(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view sort not found"))
    }

    pub async fn update_view_sort(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewSortInput,
    ) -> anyhow::Result<Option<ViewSort>> {
        let result = sqlx::query(
            "UPDATE core.view_sorts SET
                direction = COALESCE($3, direction),
                sub_field_name = COALESCE($4, sub_field_name),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(input.direction.map(|value| value.as_str()))
        .bind(&input.sub_field_name)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_sort(workspace_id, id).await
    }

    pub async fn delete_view_sort(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE core.view_sorts SET deleted_at = now(), updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn destroy_view_sort(&self, workspace_id: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM core.view_sorts WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_view_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewGroup>> {
        let row = sqlx::query(&format!(
            "SELECT {VIEW_GROUP_SELECT} FROM core.view_groups
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_view_group).transpose()
    }

    pub async fn create_view_group(
        &self,
        workspace_id: Uuid,
        input: &CreateViewGroupInput,
    ) -> anyhow::Result<ViewGroup> {
        let row = sqlx::query(
            "INSERT INTO core.view_groups
                (id, workspace_id, view_id, is_visible, field_value, position)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(input.is_visible.unwrap_or(true))
        .bind(&input.field_value)
        .bind(input.position.unwrap_or(0.0))
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_group(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view group not found"))
    }

    pub async fn update_view_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewGroupInput,
    ) -> anyhow::Result<Option<ViewGroup>> {
        let result = sqlx::query(
            "UPDATE core.view_groups SET
                is_visible = COALESCE($3, is_visible),
                field_value = COALESCE($4, field_value),
                position = COALESCE($5, position),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(input.is_visible)
        .bind(&input.field_value)
        .bind(input.position)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_group(workspace_id, id).await
    }

    pub async fn delete_view_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewGroup>> {
        let group = self.get_view_group(workspace_id, id).await?;
        if group.is_some() {
            sqlx::query(
                "UPDATE core.view_groups SET deleted_at = now(), updated_at = now()
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(group)
    }

    pub async fn destroy_view_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewGroup>> {
        let group = self.get_view_group(workspace_id, id).await?;
        sqlx::query("DELETE FROM core.view_groups WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(group)
    }

    pub async fn create_many_view_groups(
        &self,
        workspace_id: Uuid,
        inputs: &[CreateViewGroupInput],
    ) -> anyhow::Result<Vec<ViewGroup>> {
        let mut groups = Vec::with_capacity(inputs.len());
        for input in inputs {
            groups.push(self.create_view_group(workspace_id, input).await?);
        }
        Ok(groups)
    }

    pub async fn update_many_view_groups(
        &self,
        workspace_id: Uuid,
        inputs: &[(Uuid, UpdateViewGroupInput)],
    ) -> anyhow::Result<Vec<ViewGroup>> {
        let mut groups = Vec::with_capacity(inputs.len());
        for (id, input) in inputs {
            groups.push(
                self.update_view_group(workspace_id, *id, input)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("View group not found"))?,
            );
        }
        Ok(groups)
    }

    pub async fn create_many_view_fields(
        &self,
        workspace_id: Uuid,
        inputs: &[CreateViewFieldInput],
    ) -> anyhow::Result<Vec<ViewField>> {
        let mut fields = Vec::with_capacity(inputs.len());
        for input in inputs {
            fields.push(self.create_view_field(workspace_id, input).await?);
        }
        Ok(fields)
    }

    async fn get_view_field_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFieldGroup>> {
        let row = sqlx::query(
            "SELECT id, name, position, is_visible, view_id, workspace_id, created_at,
                    updated_at, is_active, deleted_at
             FROM core.view_field_groups
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let view_id: Uuid = row.get("view_id");
        let fields = self.list_view_fields(workspace_id, view_id).await?;
        Ok(Some(ViewFieldGroup {
            id: row.get("id"),
            name: row.get("name"),
            position: row.get("position"),
            is_visible: row.get("is_visible"),
            view_id,
            workspace_id: row.get("workspace_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            is_active: row.get("is_active"),
            deleted_at: row.get("deleted_at"),
            view_fields: fields
                .into_iter()
                .filter(|field| field.view_field_group_id == Some(id))
                .collect(),
            is_overridden: false,
        }))
    }

    pub async fn create_view_field_group(
        &self,
        workspace_id: Uuid,
        input: &CreateViewFieldGroupInput,
    ) -> anyhow::Result<ViewFieldGroup> {
        let row = sqlx::query(
            "INSERT INTO core.view_field_groups
                (id, workspace_id, view_id, name, position, is_visible, is_active)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, true)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(&input.name)
        .bind(input.position.unwrap_or(0.0))
        .bind(input.is_visible.unwrap_or(true))
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_field_group(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view field group not found"))
    }

    pub async fn update_view_field_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewFieldGroupInput,
    ) -> anyhow::Result<Option<ViewFieldGroup>> {
        let result = sqlx::query(
            "UPDATE core.view_field_groups SET
                name = COALESCE($3, name),
                position = COALESCE($4, position),
                is_visible = COALESCE($5, is_visible),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(&input.name)
        .bind(input.position)
        .bind(input.is_visible)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_field_group(workspace_id, id).await
    }

    pub async fn delete_view_field_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFieldGroup>> {
        let group = self.get_view_field_group(workspace_id, id).await?;
        if group.is_some() {
            sqlx::query(
                "UPDATE core.view_field_groups SET deleted_at = now(), is_active = false,
                    updated_at = now()
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(group)
    }

    pub async fn destroy_view_field_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFieldGroup>> {
        let group = self.get_view_field_group(workspace_id, id).await?;
        sqlx::query("DELETE FROM core.view_field_groups WHERE workspace_id = $1 AND id = $2")
            .bind(workspace_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(group)
    }

    pub async fn create_many_view_field_groups(
        &self,
        workspace_id: Uuid,
        inputs: &[CreateViewFieldGroupInput],
    ) -> anyhow::Result<Vec<ViewFieldGroup>> {
        let mut groups = Vec::with_capacity(inputs.len());
        for input in inputs {
            groups.push(self.create_view_field_group(workspace_id, input).await?);
        }
        Ok(groups)
    }

    async fn get_view_filter_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ViewFilterGroup>> {
        let row = sqlx::query(&format!(
            "SELECT {VIEW_FILTER_GROUP_SELECT} FROM core.view_filter_groups
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL"
        ))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_view_filter_group).transpose()
    }

    pub async fn create_view_filter_group(
        &self,
        workspace_id: Uuid,
        input: &CreateViewFilterGroupInput,
    ) -> anyhow::Result<ViewFilterGroup> {
        let row = sqlx::query(
            "INSERT INTO core.view_filter_groups
                (id, workspace_id, view_id, parent_view_filter_group_id, logical_operator,
                 position_in_view_filter_group)
             VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(input.id)
        .bind(workspace_id)
        .bind(input.view_id)
        .bind(input.parent_view_filter_group_id)
        .bind(
            input
                .logical_operator
                .unwrap_or(ViewFilterGroupLogicalOperator::And)
                .as_str(),
        )
        .bind(input.position_in_view_filter_group)
        .fetch_one(&self.pool)
        .await?;
        let id: Uuid = row.get("id");
        self.get_view_filter_group(workspace_id, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created view filter group not found"))
    }

    pub async fn update_view_filter_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
        input: &UpdateViewFilterGroupInput,
    ) -> anyhow::Result<Option<ViewFilterGroup>> {
        let result = sqlx::query(
            "UPDATE core.view_filter_groups SET
                parent_view_filter_group_id =
                    COALESCE($3, parent_view_filter_group_id),
                logical_operator = COALESCE($4, logical_operator),
                position_in_view_filter_group =
                    COALESCE($5, position_in_view_filter_group),
                view_id = COALESCE($6, view_id),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(input.parent_view_filter_group_id)
        .bind(input.logical_operator.map(|value| value.as_str()))
        .bind(input.position_in_view_filter_group)
        .bind(input.view_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_view_filter_group(workspace_id, id).await
    }

    pub async fn delete_view_filter_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE core.view_filter_groups SET deleted_at = now(), updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn destroy_view_filter_group(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<bool> {
        let result =
            sqlx::query("DELETE FROM core.view_filter_groups WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}

struct PersonFieldSeed {
    name: &'static str,
    label: &'static str,
    field_type: FieldMetadataType,
    nullable: bool,
    unique: bool,
    searchable: bool,
}

const TASK_TARGET_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "taskId",
        label: "Task",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetCompanyId",
        label: "Company",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetOpportunityId",
        label: "Opportunity",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetPersonId",
        label: "Person",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "task",
        label: "Task",
        field_type: FieldMetadataType::Relation,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "target",
        label: "Related to",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetCompany",
        label: "Company",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetOpportunity",
        label: "Opportunity",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetPerson",
        label: "Person",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const NOTE_TARGET_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "noteId",
        label: "Note",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetCompanyId",
        label: "Company",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetOpportunityId",
        label: "Opportunity",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetPersonId",
        label: "Person",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "note",
        label: "Note",
        field_type: FieldMetadataType::Relation,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "target",
        label: "Related to",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetCompany",
        label: "Company",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetOpportunity",
        label: "Opportunity",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "targetPerson",
        label: "Person",
        field_type: FieldMetadataType::MorphRelation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const TASK_TARGET_SELECT: &str = "id, created_at, updated_at, deleted_at, position, \
     created_by::text AS created_by, updated_by::text AS updated_by, search_vector, \
     task_id, target_company_id, target_opportunity_id, target_person_id";

const NOTE_TARGET_SELECT: &str = "id, created_at, updated_at, deleted_at, position, \
     created_by::text AS created_by, updated_by::text AS updated_by, search_vector, \
     note_id, target_company_id, target_opportunity_id, target_person_id";

fn row_to_task_target(row: &sqlx::postgres::PgRow) -> TaskTarget {
    TaskTarget {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        position: row.get("position"),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        task_id: row.get("task_id"),
        target_company_id: row.get("target_company_id"),
        target_opportunity_id: row.get("target_opportunity_id"),
        target_person_id: row.get("target_person_id"),
    }
}

fn row_to_note_target(row: &sqlx::postgres::PgRow) -> NoteTarget {
    NoteTarget {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        position: row.get("position"),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        note_id: row.get("note_id"),
        target_company_id: row.get("target_company_id"),
        target_opportunity_id: row.get("target_opportunity_id"),
        target_person_id: row.get("target_person_id"),
    }
}

struct StandardObjectSeed<'a> {
    name_singular: &'a str,
    name_plural: &'a str,
    label_singular: &'a str,
    label_plural: &'a str,
    icon: &'a str,
    color: &'a str,
    shortcut: &'a str,
    fields: &'a [PersonFieldSeed],
    view_name: &'a str,
    view_fields: &'a [(&'a str, f64)],
    create_view: bool,
}

const WORKSPACE_MEMBER_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deleted at",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "name",
        label: "Name",
        field_type: FieldMetadataType::FullName,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "colorScheme",
        label: "Color Scheme",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "uiScale",
        label: "Interface Scale",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "locale",
        label: "Locale",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "avatarUrl",
        label: "Avatar",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "userEmail",
        label: "User Email",
        field_type: FieldMetadataType::Emails,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "jobTitle",
        label: "Job Title",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "timeZone",
        label: "Time Zone",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "dateFormat",
        label: "Date Format",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "timeFormat",
        label: "Time Format",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "numberFormat",
        label: "Number Format",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "assignedTasks",
        label: "Assigned tasks",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "accountOwnerForCompanies",
        label: "Account owner for companies",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const TASK_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "title",
        label: "Title",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "bodyV2",
        label: "Body",
        field_type: FieldMetadataType::RichText,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "dueAt",
        label: "Due date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "status",
        label: "Status",
        field_type: FieldMetadataType::Select,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "assigneeId",
        label: "Assignee",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "taskTargets",
        label: "Task targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const DEFAULT_TASK_VIEW_FIELDS: &[(&str, f64)] = &[
    ("title", 220.0),
    ("status", 140.0),
    ("dueAt", 160.0),
    ("assigneeId", 160.0),
];

const NOTE_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "title",
        label: "Title",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "bodyV2",
        label: "Body",
        field_type: FieldMetadataType::RichText,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "noteTargets",
        label: "Note targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const DEFAULT_NOTE_VIEW_FIELDS: &[(&str, f64)] = &[("title", 220.0), ("createdAt", 160.0)];

const TASK_SELECT: &str = "id, created_at, updated_at, deleted_at, position, title, \
     body_v2::text AS body_v2, due_at, status, created_by::text AS created_by, \
     updated_by::text AS updated_by, search_vector, assignee_id";

const NOTE_SELECT: &str = "id, created_at, updated_at, deleted_at, position, title, \
     body_v2::text AS body_v2, created_by::text AS created_by, \
     updated_by::text AS updated_by, search_vector";

fn row_to_task(row: &sqlx::postgres::PgRow) -> Task {
    let status: Option<String> = row.get("status");
    Task {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        position: row.get("position"),
        title: row.get("title"),
        body_v2: parse_json_option(row.get("body_v2")),
        due_at: row.get("due_at"),
        status: status.as_deref().and_then(TaskStatus::parse),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        assignee_id: row.get("assignee_id"),
    }
}

fn row_to_note(row: &sqlx::postgres::PgRow) -> Note {
    Note {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        position: row.get("position"),
        title: row.get("title"),
        body_v2: parse_json_option(row.get("body_v2")),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
    }
}

const OPPORTUNITY_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "name",
        label: "Name",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "amount",
        label: "Amount",
        field_type: FieldMetadataType::Currency,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "closeDate",
        label: "Close date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "stage",
        label: "Stage",
        field_type: FieldMetadataType::Select,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "companyId",
        label: "Company",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "pointOfContactId",
        label: "Point of contact",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "ownerId",
        label: "Owner",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "taskTargets",
        label: "Task targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "noteTargets",
        label: "Note targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const DEFAULT_OPPORTUNITY_VIEW_FIELDS: &[(&str, f64)] = &[
    ("name", 180.0),
    ("amount", 140.0),
    ("closeDate", 160.0),
    ("stage", 120.0),
    ("companyId", 160.0),
];

const OPPORTUNITY_SELECT: &str = "id, created_at, updated_at, deleted_at, name, \
     amount::text AS amount, close_date, stage, position, \
     created_by::text AS created_by, updated_by::text AS updated_by, search_vector, \
     company_id, point_of_contact_id, owner_id";

fn row_to_opportunity(row: &sqlx::postgres::PgRow) -> Opportunity {
    let stage: Option<String> = row.get("stage");
    Opportunity {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        name: row.get("name"),
        amount: parse_json_option(row.get("amount")),
        close_date: row.get("close_date"),
        stage: stage
            .as_deref()
            .and_then(OpportunityStage::parse)
            .unwrap_or_default(),
        position: row.get("position"),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        company_id: row.get("company_id"),
        point_of_contact_id: row.get("point_of_contact_id"),
        owner_id: row.get("owner_id"),
    }
}

const COMPANY_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "name",
        label: "Name",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "domainName",
        label: "Domain name",
        field_type: FieldMetadataType::Links,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "address",
        label: "Address",
        field_type: FieldMetadataType::Address,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "linkedinLink",
        label: "Linkedin",
        field_type: FieldMetadataType::Links,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "annualRevenue",
        label: "Annual revenue",
        field_type: FieldMetadataType::Currency,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "accountOwnerId",
        label: "Account owner",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "people",
        label: "People",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "opportunities",
        label: "Opportunities",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "taskTargets",
        label: "Task targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "noteTargets",
        label: "Note targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const DEFAULT_COMPANY_VIEW_FIELDS: &[(&str, f64)] =
    &[("name", 180.0), ("domainName", 200.0), ("createdAt", 160.0)];

const COMPANY_SELECT: &str = "id, created_at, updated_at, deleted_at, name, \
     domain_name::text AS domain_name, address::text AS address, \
     linkedin_link::text AS linkedin_link, annual_revenue::text AS annual_revenue, \
     position, created_by::text AS created_by, updated_by::text AS updated_by, \
     search_vector, account_owner_id";

fn row_to_company(row: &sqlx::postgres::PgRow) -> Company {
    Company {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        name: row.get("name"),
        domain_name: parse_json_option(row.get("domain_name")),
        address: parse_json_option(row.get("address")),
        linkedin_link: parse_json_option(row.get("linkedin_link")),
        annual_revenue: parse_json_option(row.get("annual_revenue")),
        position: row.get("position"),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        account_owner_id: row.get("account_owner_id"),
    }
}

fn parse_json_option<T: DeserializeOwned + Default>(raw: Option<String>) -> T {
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

const PERSON_METADATA_FIELDS: &[PersonFieldSeed] = &[
    PersonFieldSeed {
        name: "id",
        label: "ID",
        field_type: FieldMetadataType::Uuid,
        nullable: false,
        unique: true,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdAt",
        label: "Creation date",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedAt",
        label: "Last update",
        field_type: FieldMetadataType::DateTime,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "deletedAt",
        label: "Deletion date",
        field_type: FieldMetadataType::DateTime,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "name",
        label: "Name",
        field_type: FieldMetadataType::FullName,
        nullable: false,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "emails",
        label: "Emails",
        field_type: FieldMetadataType::Emails,
        nullable: false,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "linkedinLink",
        label: "Linkedin",
        field_type: FieldMetadataType::Links,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "jobTitle",
        label: "Job title",
        field_type: FieldMetadataType::Text,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "phones",
        label: "Phones",
        field_type: FieldMetadataType::Phones,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "avatarUrl",
        label: "Avatar URL",
        field_type: FieldMetadataType::Text,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "avatarFile",
        label: "Avatar",
        field_type: FieldMetadataType::Files,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "position",
        label: "Position",
        field_type: FieldMetadataType::Position,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "createdBy",
        label: "Created by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "updatedBy",
        label: "Updated by",
        field_type: FieldMetadataType::Actor,
        nullable: false,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "searchVector",
        label: "Search vector",
        field_type: FieldMetadataType::TsVector,
        nullable: true,
        unique: false,
        searchable: true,
    },
    PersonFieldSeed {
        name: "companyId",
        label: "Company",
        field_type: FieldMetadataType::Uuid,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "pointOfContactForOpportunities",
        label: "Point of contact for opportunities",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "taskTargets",
        label: "Task targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
    PersonFieldSeed {
        name: "noteTargets",
        label: "Note targets",
        field_type: FieldMetadataType::Relation,
        nullable: true,
        unique: false,
        searchable: false,
    },
];

const OBJECT_SELECT: &str =
    "id, universal_identifier::text AS universal_identifier, name_singular, \
     name_plural, label_singular, label_plural, description, icon, color, shortcut, is_remote, \
     is_active, is_system, is_ui_editable, is_ui_creatable, is_searchable, open_record_in, \
     readability, sharing_reach, writability, application_id, created_at, updated_at, \
     label_identifier_field_metadata_id, image_identifier_field_metadata_id, \
     is_label_synced_with_name, duplicate_criteria::text AS duplicate_criteria";

const FIELD_SELECT: &str = "id, universal_identifier::text AS universal_identifier, type, name, \
     label, description, icon, is_active, is_system, is_ui_editable, writability, is_nullable, \
     is_unique, is_searchable, is_audit_logged, default_value::text AS default_value, \
     options::text AS options, settings::text AS settings, object_metadata_id, \
     is_label_synced_with_name, morph_id, created_at, updated_at, application_id";

fn row_to_object(row: &sqlx::postgres::PgRow) -> anyhow::Result<ObjectMetadata> {
    Ok(ObjectMetadata {
        id: row.get("id"),
        universal_identifier: row.get("universal_identifier"),
        name_singular: row.get("name_singular"),
        name_plural: row.get("name_plural"),
        label_singular: row.get("label_singular"),
        label_plural: row.get("label_plural"),
        description: row.get("description"),
        icon: row.get("icon"),
        shortcut: row.get("shortcut"),
        color: row.get("color"),
        is_remote: row.get("is_remote"),
        is_active: row.get("is_active"),
        is_system: row.get("is_system"),
        is_ui_editable: row.get("is_ui_editable"),
        is_ui_creatable: row.get("is_ui_creatable"),
        is_searchable: row.get("is_searchable"),
        open_record_in: ObjectOpenRecordIn::parse(row.get("open_record_in"))
            .ok_or_else(|| anyhow::anyhow!("invalid open_record_in metadata value"))?,
        readability: MetadataReadability::parse(row.get("readability"))
            .ok_or_else(|| anyhow::anyhow!("invalid readability metadata value"))?,
        sharing_reach: ObjectSharingReach::parse(row.get("sharing_reach"))
            .ok_or_else(|| anyhow::anyhow!("invalid sharing_reach metadata value"))?,
        writability: MetadataWritability::parse(row.get("writability"))
            .ok_or_else(|| anyhow::anyhow!("invalid writability metadata value"))?,
        application_id: row.get("application_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        label_identifier_field_metadata_id: row.get("label_identifier_field_metadata_id"),
        image_identifier_field_metadata_id: row.get("image_identifier_field_metadata_id"),
        is_label_synced_with_name: row.get("is_label_synced_with_name"),
        duplicate_criteria: row
            .get::<Option<String>, _>("duplicate_criteria")
            .and_then(|raw| serde_json::from_str(&raw).ok()),
        fields: Vec::new(),
        index_metadata: Vec::new(),
        search_field_metadata: Vec::new(),
    })
}

fn row_to_job(row: &sqlx::postgres::PgRow) -> JobRecord {
    JobRecord {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        kind: row.get("kind"),
        payload: row.get("payload"),
        state: row.get("state"),
        attempts: row.get("attempts"),
        max_attempts: row.get("max_attempts"),
        run_at: row.get("run_at"),
        last_error: row.get("last_error"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        completed_at: row.get("completed_at"),
    }
}

fn row_to_webhook_delivery(row: &sqlx::postgres::PgRow) -> WebhookDeliveryRecord {
    WebhookDeliveryRecord {
        id: row.get("id"),
        webhook_id: row.get("webhook_id"),
        event: row.get("event"),
        event_id: row.get("event_id"),
        state: row.get("state"),
        attempts: row.get("attempts"),
        last_status_code: row.get("last_status_code"),
        last_error: row.get("last_error"),
        target_url: row.get("target_url"),
        request_body: row.get("request_body"),
        signature: row.get("signature"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        completed_at: row.get("completed_at"),
    }
}

fn row_to_webhook(row: &sqlx::postgres::PgRow) -> WebhookRecord {
    WebhookRecord {
        id: row.get("id"),
        target_url: row.get("target_url"),
        operations: row.get("operations"),
        description: row.get("description"),
        secret: row.get("secret"),
        application_id: row.get("application_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
    }
}

fn row_to_role(row: &sqlx::postgres::PgRow) -> RoleRecord {
    RoleRecord {
        id: row.get("id"),
        label: row.get("label"),
        description: row.get("description"),
        icon: row.get("icon"),
        is_editable: row.get("is_editable"),
        can_update_all_settings: row.get("can_update_all_settings"),
        can_access_all_tools: row.get("can_access_all_tools"),
        can_read_all_object_records: row.get("can_read_all_object_records"),
        can_update_all_object_records: row.get("can_update_all_object_records"),
        can_soft_delete_all_object_records: row.get("can_soft_delete_all_object_records"),
        can_destroy_all_object_records: row.get("can_destroy_all_object_records"),
        can_be_assigned_to_users: row.get("can_be_assigned_to_users"),
        can_be_assigned_to_agents: row.get("can_be_assigned_to_agents"),
        can_be_assigned_to_api_keys: row.get("can_be_assigned_to_api_keys"),
        application_id: row.get("application_id"),
        universal_identifier: row.get("universal_identifier"),
        permission_flags: Vec::new(),
    }
}

fn row_to_field(row: &sqlx::postgres::PgRow) -> anyhow::Result<FieldMetadata> {
    Ok(FieldMetadata {
        id: row.get("id"),
        universal_identifier: row.get("universal_identifier"),
        field_type: FieldMetadataType::parse(row.get("type"))
            .ok_or_else(|| anyhow::anyhow!("invalid field metadata type"))?,
        name: row.get("name"),
        label: row.get("label"),
        description: row.get("description"),
        icon: row.get("icon"),
        is_active: row.get("is_active"),
        is_system: row.get("is_system"),
        is_ui_editable: row.get("is_ui_editable"),
        writability: MetadataWritability::parse(row.get("writability"))
            .ok_or_else(|| anyhow::anyhow!("invalid writability metadata value"))?,
        is_nullable: row.get("is_nullable"),
        is_unique: row.get("is_unique"),
        is_searchable: row.get("is_searchable"),
        is_audit_logged: row.get("is_audit_logged"),
        default_value: parse_json_opt(row.get("default_value")),
        options: parse_json_opt(row.get("options")),
        settings: parse_json_opt(row.get("settings")),
        object_metadata_id: row.get("object_metadata_id"),
        is_label_synced_with_name: row.get("is_label_synced_with_name"),
        morph_id: row.get("morph_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        application_id: row.get("application_id"),
        relation: None,
        morph_relations: Vec::new(),
    })
}

const METADATA_RELATIONS: &[(&str, &str, &str, &str)] = &[
    ("person", "companyId", "company", "people"),
    ("opportunity", "companyId", "company", "opportunities"),
    (
        "opportunity",
        "pointOfContactId",
        "person",
        "pointOfContactForOpportunities",
    ),
    ("task", "assigneeId", "workspaceMember", "assignedTasks"),
    (
        "company",
        "accountOwnerId",
        "workspaceMember",
        "accountOwnerForCompanies",
    ),
    ("taskTarget", "task", "task", "taskTargets"),
    ("noteTarget", "note", "note", "noteTargets"),
    ("taskTarget", "targetCompany", "company", "taskTargets"),
    (
        "taskTarget",
        "targetOpportunity",
        "opportunity",
        "taskTargets",
    ),
    ("taskTarget", "targetPerson", "person", "taskTargets"),
    ("noteTarget", "targetCompany", "company", "noteTargets"),
    (
        "noteTarget",
        "targetOpportunity",
        "opportunity",
        "noteTargets",
    ),
    ("noteTarget", "targetPerson", "person", "noteTargets"),
];

fn build_search_field_metadata(object: &ObjectMetadata) -> Vec<SearchFieldMetadata> {
    let Some(vector_field) = object
        .fields
        .iter()
        .find(|field| field.field_type == FieldMetadataType::TsVector)
    else {
        return Vec::new();
    };
    object
        .fields
        .iter()
        .filter(|field| field.is_searchable && field.field_type != FieldMetadataType::TsVector)
        .enumerate()
        .map(|(position, field)| SearchFieldMetadata {
            id: Uuid::new_v5(&vector_field.id, field.id.as_bytes()),
            field_metadata_id: field.id,
            ts_vector_field_metadata_id: vector_field.id,
            position: position as f64,
            created_at: field.created_at,
            updated_at: field.updated_at,
        })
        .collect()
}

fn parse_json_opt(raw: Option<String>) -> Option<squid_core::GqlJson<serde_json::Value>> {
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .map(squid_core::GqlJson)
}

const DEFAULT_VIEW_FIELDS: &[(&str, f64)] = &[
    ("name", 180.0),
    ("emails", 200.0),
    ("jobTitle", 160.0),
    ("phones", 160.0),
    ("createdAt", 160.0),
];

const VIEW_SELECT: &str =
    "id, universal_identifier::text AS universal_identifier, application_id, \
     is_system_side_effect, name, object_metadata_id, type, key, icon, position, is_compact, \
     is_custom, open_record_in, kanban_aggregate_operation, \
     kanban_aggregate_operation_field_metadata_id, main_group_by_field_metadata_id, \
     should_hide_empty_groups, kanban_column_width, group_load_limit, calendar_field_metadata_id, \
     calendar_end_field_metadata_id, workspace_id, any_field_filter_value, calendar_layout, \
     created_at, updated_at, deleted_at, visibility, created_by_user_workspace_id, is_active";

const VIEW_FIELD_SELECT: &str = "id, universal_identifier::text AS universal_identifier, \
     application_id, is_system_side_effect, field_metadata_id, is_visible, size, position, \
     aggregate_operation, view_id, view_field_group_id, workspace_id, created_at, updated_at, \
     is_active, deleted_at";

const VIEW_FILTER_SELECT: &str = "id, field_metadata_id, operand, value::text AS value, \
     view_filter_group_id, position_in_view_filter_group, sub_field_name, \
     relation_target_field_metadata_id, view_id, workspace_id, created_at, updated_at, deleted_at";

const VIEW_SORT_SELECT: &str = "id, field_metadata_id, direction, sub_field_name, view_id, \
     workspace_id, created_at, updated_at, deleted_at";

const VIEW_GROUP_SELECT: &str = "id, is_visible, field_value, position, view_id, workspace_id, \
     created_at, updated_at, deleted_at";

const VIEW_FILTER_GROUP_SELECT: &str = "id, parent_view_filter_group_id, logical_operator, \
     position_in_view_filter_group, view_id, workspace_id, created_at, updated_at, deleted_at";

fn row_to_view_filter(row: &sqlx::postgres::PgRow) -> anyhow::Result<ViewFilter> {
    Ok(ViewFilter {
        id: row.get("id"),
        field_metadata_id: row.get("field_metadata_id"),
        operand: ViewFilterOperand::parse(row.get("operand"))
            .ok_or_else(|| anyhow::anyhow!("invalid view filter operand"))?,
        value: parse_json_opt(row.get("value"))
            .unwrap_or(squid_core::GqlJson(serde_json::Value::Null)),
        view_filter_group_id: row.get("view_filter_group_id"),
        position_in_view_filter_group: row.get("position_in_view_filter_group"),
        sub_field_name: row.get("sub_field_name"),
        relation_target_field_metadata_id: row.get("relation_target_field_metadata_id"),
        view_id: row.get("view_id"),
        workspace_id: row.get("workspace_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
    })
}

fn row_to_view_sort(row: &sqlx::postgres::PgRow) -> anyhow::Result<ViewSort> {
    Ok(ViewSort {
        id: row.get("id"),
        field_metadata_id: row.get("field_metadata_id"),
        direction: ViewSortDirection::parse(row.get("direction"))
            .ok_or_else(|| anyhow::anyhow!("invalid view sort direction"))?,
        sub_field_name: row.get("sub_field_name"),
        view_id: row.get("view_id"),
        workspace_id: row.get("workspace_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
    })
}

fn row_to_view_group(row: &sqlx::postgres::PgRow) -> anyhow::Result<ViewGroup> {
    Ok(ViewGroup {
        id: row.get("id"),
        is_visible: row.get("is_visible"),
        field_value: row.get("field_value"),
        position: row.get("position"),
        view_id: row.get("view_id"),
        workspace_id: row.get("workspace_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
    })
}

fn row_to_view_filter_group(row: &sqlx::postgres::PgRow) -> anyhow::Result<ViewFilterGroup> {
    Ok(ViewFilterGroup {
        id: row.get("id"),
        parent_view_filter_group_id: row.get("parent_view_filter_group_id"),
        logical_operator: ViewFilterGroupLogicalOperator::parse(row.get("logical_operator"))
            .ok_or_else(|| anyhow::anyhow!("invalid view filter group operator"))?,
        position_in_view_filter_group: row.get("position_in_view_filter_group"),
        view_id: row.get("view_id"),
        workspace_id: row.get("workspace_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
    })
}

fn row_to_view(row: &sqlx::postgres::PgRow) -> anyhow::Result<View> {
    let key: Option<String> = row.get("key");
    let aggregate: Option<String> = row.get("kanban_aggregate_operation");
    let calendar_layout: Option<String> = row.get("calendar_layout");
    Ok(View {
        id: row.get("id"),
        universal_identifier: row.get("universal_identifier"),
        application_id: row.get("application_id"),
        is_system_side_effect: row.get("is_system_side_effect"),
        name: row.get("name"),
        object_metadata_id: row.get("object_metadata_id"),
        view_type: ViewType::parse(row.get("type"))
            .ok_or_else(|| anyhow::anyhow!("invalid view type"))?,
        key: key.as_deref().and_then(ViewKey::parse),
        icon: row.get("icon"),
        position: row.get("position"),
        is_compact: row.get("is_compact"),
        is_custom: row.get("is_custom"),
        open_record_in: ViewOpenRecordIn::parse(row.get("open_record_in"))
            .ok_or_else(|| anyhow::anyhow!("invalid view open_record_in"))?,
        kanban_aggregate_operation: aggregate.as_deref().and_then(AggregateOperations::parse),
        kanban_aggregate_operation_field_metadata_id: row
            .get("kanban_aggregate_operation_field_metadata_id"),
        main_group_by_field_metadata_id: row.get("main_group_by_field_metadata_id"),
        should_hide_empty_groups: row.get("should_hide_empty_groups"),
        kanban_column_width: row.get("kanban_column_width"),
        group_load_limit: row.get("group_load_limit"),
        calendar_field_metadata_id: row.get("calendar_field_metadata_id"),
        calendar_end_field_metadata_id: row.get("calendar_end_field_metadata_id"),
        workspace_id: row.get("workspace_id"),
        any_field_filter_value: row.get("any_field_filter_value"),
        calendar_layout: calendar_layout
            .as_deref()
            .and_then(ViewCalendarLayout::parse),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        view_fields: Vec::new(),
        view_filters: Vec::new(),
        view_filter_groups: Vec::new(),
        view_sorts: Vec::new(),
        view_groups: Vec::new(),
        view_field_groups: Vec::new(),
        visibility: ViewVisibility::parse(row.get("visibility"))
            .ok_or_else(|| anyhow::anyhow!("invalid view visibility"))?,
        created_by_user_workspace_id: row.get("created_by_user_workspace_id"),
        is_active: row.get("is_active"),
    })
}

fn row_to_view_field(row: &sqlx::postgres::PgRow) -> anyhow::Result<ViewField> {
    let aggregate: Option<String> = row.get("aggregate_operation");
    Ok(ViewField {
        id: row.get("id"),
        universal_identifier: row.get("universal_identifier"),
        application_id: row.get("application_id"),
        is_system_side_effect: row.get("is_system_side_effect"),
        field_metadata_id: row.get("field_metadata_id"),
        is_visible: row.get("is_visible"),
        size: row.get("size"),
        position: row.get("position"),
        aggregate_operation: aggregate.as_deref().and_then(AggregateOperations::parse),
        view_id: row.get("view_id"),
        view_field_group_id: row.get("view_field_group_id"),
        workspace_id: row.get("workspace_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        is_active: row.get("is_active"),
        deleted_at: row.get("deleted_at"),
        is_overridden: None,
    })
}

const PERSON_SELECT: &str = "id, created_at, updated_at, deleted_at, name_first, name_last, \
     primary_email, additional_emails, job_title, linkedin_link::text AS linkedin_link, \
     phones::text AS phones, avatar_url, avatar_file::text AS avatar_file, position, \
     created_by::text AS created_by, updated_by::text AS updated_by, search_vector, company_id";

/// A page of people with the connection flags upstream exposes.
#[derive(Debug, Clone)]
pub struct PersonPage {
    pub persons: Vec<Person>,
    pub total: i64,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

/// Sortable person columns. The REST/GraphQL parsers may only produce these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonColumn {
    Id,
    CreatedAt,
    UpdatedAt,
    NameFirst,
    NameLast,
    PrimaryEmail,
    JobTitle,
    CompanyId,
    CompanyName,
    CompanyCreatedAt,
}

impl PersonColumn {
    fn sql(self) -> &'static str {
        match self {
            PersonColumn::Id => "id",
            PersonColumn::CreatedAt => "created_at",
            PersonColumn::UpdatedAt => "updated_at",
            PersonColumn::NameFirst => "name_first",
            PersonColumn::NameLast => "name_last",
            PersonColumn::PrimaryEmail => "primary_email",
            PersonColumn::JobTitle => "job_title",
            PersonColumn::CompanyId => "company_id",
            PersonColumn::CompanyName => "company_name",
            PersonColumn::CompanyCreatedAt => "company_created_at",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderTerm {
    pub column: PersonColumn,
    pub descending: bool,
    pub nulls_first: bool,
}

impl OrderTerm {
    fn sql(&self, schema: &str) -> String {
        let column = match self.column {
            PersonColumn::CompanyName => {
                format!("(SELECT name FROM {schema}.company WHERE id = company_id)")
            }
            PersonColumn::CompanyCreatedAt => {
                format!("(SELECT created_at FROM {schema}.company WHERE id = company_id)")
            }
            column => column.sql().to_string(),
        };
        format!(
            "{column} {} NULLS {}",
            if self.descending { "DESC" } else { "ASC" },
            if self.nulls_first { "FIRST" } else { "LAST" }
        )
    }
}

fn build_search_vector(
    first_name: &str,
    last_name: &str,
    primary_email: &str,
    job_title: Option<&str>,
) -> Option<String> {
    let mut parts: Vec<(String, usize)> = Vec::new();
    let mut position = 1usize;
    let mut push = |value: &str| {
        for lexeme in value.split_whitespace() {
            if !lexeme.is_empty() {
                parts.push((lexeme.to_lowercase(), position));
                position += 1;
            }
        }
    };
    push(first_name);
    push(last_name);
    push(primary_email);
    if let Some(job_title) = job_title {
        push(job_title);
    }
    if parts.is_empty() {
        None
    } else {
        Some(
            parts
                .into_iter()
                .map(|(lexeme, position)| format!("'{lexeme}':{position}"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

fn row_to_index(row: &sqlx::postgres::PgRow) -> IndexMetadata {
    let index_type: String = row.get("index_type");
    IndexMetadata {
        id: row.get("id"),
        name: row.get("name"),
        is_custom: row.get("is_custom"),
        is_unique: row.get("is_unique"),
        index_where_clause: row.get("index_where_clause"),
        index_type: IndexType::parse(&index_type).unwrap_or(IndexType::Btree),
        index_field_metadata_list: Vec::new(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_index_field(row: &sqlx::postgres::PgRow) -> IndexFieldMetadata {
    IndexFieldMetadata {
        id: row.get("id"),
        field_metadata_id: row.get("field_metadata_id"),
        order: row.get("position"),
        sub_field_name: row.get("sub_field_name"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_file(row: &sqlx::postgres::PgRow) -> FileRecord {
    FileRecord {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        filename: row.get("filename"),
        path: row.get("path"),
        size: row.get("size"),
        content_type: row.get("content_type"),
        file_folder: row.get("file_folder"),
        field_metadata_id: row.get("field_metadata_id"),
        uploaded: row.get("uploaded"),
        created_at: row.get("created_at"),
    }
}

fn row_to_person(row: &sqlx::postgres::PgRow) -> Person {
    let additional: Vec<String> = row.get("additional_emails");
    let avatar_file: Option<String> = row.get("avatar_file");
    Person {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        deleted_at: row.get("deleted_at"),
        name: FullName {
            first_name: row.get("name_first"),
            last_name: row.get("name_last"),
        },
        emails: Emails {
            primary_email: row.get("primary_email"),
            additional_emails: additional,
        },
        linkedin_link: parse_json(row.get("linkedin_link")),
        job_title: row.get("job_title"),
        phones: parse_json(row.get("phones")),
        avatar_url: row.get("avatar_url"),
        avatar_file: avatar_file
            .and_then(|raw| serde_json::from_str::<Vec<squid_core::FileObject>>(&raw).ok()),
        position: row.get("position"),
        created_by: parse_json_option(row.get("created_by")),
        updated_by: parse_json_option(row.get("updated_by")),
        search_vector: row.get("search_vector"),
        company_id: row.get("company_id"),
    }
}

fn parse_json<T: DeserializeOwned + Default>(raw: String) -> T {
    serde_json::from_str(&raw).unwrap_or_default()
}

fn assert_valid_identifier(identifier: &str) -> anyhow::Result<()> {
    let valid = !identifier.is_empty()
        && identifier.len() <= 63
        && identifier
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && identifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        anyhow::bail!("invalid SQL identifier")
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn custom_field_expression(placeholder: &str, field_type: FieldMetadataType) -> String {
    match field_type {
        FieldMetadataType::Uuid
        | FieldMetadataType::Relation
        | FieldMetadataType::MorphRelation => {
            format!("({placeholder}::jsonb #>> '{{}}')::uuid")
        }
        FieldMetadataType::Boolean => format!("({placeholder}::jsonb #>> '{{}}')::boolean"),
        FieldMetadataType::DateTime => {
            format!("({placeholder}::jsonb #>> '{{}}')::timestamptz")
        }
        FieldMetadataType::Date => format!("({placeholder}::jsonb #>> '{{}}')::date"),
        FieldMetadataType::Number | FieldMetadataType::Numeric | FieldMetadataType::Rating => {
            format!("({placeholder}::jsonb #>> '{{}}')::numeric")
        }
        FieldMetadataType::Position => {
            format!("({placeholder}::jsonb #>> '{{}}')::double precision")
        }
        FieldMetadataType::Text | FieldMetadataType::Select | FieldMetadataType::TsVector => {
            format!("({placeholder}::jsonb #>> '{{}}')::text")
        }
        FieldMetadataType::Actor
        | FieldMetadataType::Address
        | FieldMetadataType::Array
        | FieldMetadataType::Currency
        | FieldMetadataType::Emails
        | FieldMetadataType::Files
        | FieldMetadataType::FullName
        | FieldMetadataType::Links
        | FieldMetadataType::MultiSelect
        | FieldMetadataType::Phones
        | FieldMetadataType::RawJson
        | FieldMetadataType::RichText => format!("{placeholder}::jsonb"),
    }
}

fn field_column_type(field_type: FieldMetadataType) -> &'static str {
    match field_type {
        FieldMetadataType::Uuid => "UUID",
        FieldMetadataType::Boolean => "BOOLEAN",
        FieldMetadataType::DateTime => "TIMESTAMPTZ",
        FieldMetadataType::Date => "DATE",
        FieldMetadataType::Number | FieldMetadataType::Numeric | FieldMetadataType::Rating => {
            "NUMERIC"
        }
        FieldMetadataType::Position => "DOUBLE PRECISION",
        FieldMetadataType::Text | FieldMetadataType::Select | FieldMetadataType::TsVector => "TEXT",
        FieldMetadataType::Relation | FieldMetadataType::MorphRelation => "UUID",
        FieldMetadataType::Actor
        | FieldMetadataType::Address
        | FieldMetadataType::Array
        | FieldMetadataType::Currency
        | FieldMetadataType::Emails
        | FieldMetadataType::Files
        | FieldMetadataType::FullName
        | FieldMetadataType::Links
        | FieldMetadataType::MultiSelect
        | FieldMetadataType::Phones
        | FieldMetadataType::RawJson
        | FieldMetadataType::RichText => "JSONB",
    }
}

async fn object_singular_name(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT name_singular FROM core.object_metadata WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(object_id)
    .fetch_optional(pool)
    .await?)
}

fn assert_valid_schema(schema: &str) -> anyhow::Result<()> {
    let re = Regex::new(r"^workspace_[0-9a-f]{32}$").expect("static regex");
    if !re.is_match(schema) {
        anyhow::bail!("invalid workspace schema name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_name_is_deterministic_and_valid() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let name = Db::workspace_schema_name(id);
        assert_eq!(name, "workspace_11111111222233334444555555555555");
        assert!(assert_valid_schema(&name).is_ok());
    }

    #[test]
    fn rejects_injected_schema_names() {
        assert!(assert_valid_schema("workspace_abc; DROP TABLE person").is_err());
        assert!(assert_valid_schema("public").is_err());
        assert!(assert_valid_schema("workspace_XYZ").is_err());
    }
}
