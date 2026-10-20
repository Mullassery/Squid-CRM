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
use sqlx::{PgPool, Row};
use squid_core::{
    Actor, AggregateOperations, ApiKey, Company, CreateCompanyInput, CreateFieldMetadataInput,
    CreateNoteInput, CreateNoteTargetInput, CreateObjectMetadataInput, CreateOpportunityInput,
    CreatePersonInput, CreateTaskInput, CreateTaskTargetInput, CreateViewFieldGroupInput,
    CreateViewFieldInput, CreateViewFilterGroupInput, CreateViewFilterInput, CreateViewGroupInput,
    CreateViewInput, CreateViewSortInput, DateOp, Emails, FieldMetadata, FieldMetadataType,
    FilterExpr, FullName, MaybeUndefined, MetadataReadability, MetadataWritability, Note,
    NoteTarget, ObjectMetadata, ObjectOpenRecordIn, ObjectSharingReach, Opportunity,
    OpportunityStage, Person, Role, StringOp, Task, TaskStatus, TaskTarget, UpdateCompanyInput,
    UpdateFieldMetadataInput, UpdateNoteInput, UpdateNoteTargetInput, UpdateObjectMetadataInput,
    UpdateOpportunityInput, UpdatePersonInput, UpdateTaskInput, UpdateTaskTargetInput,
    UpdateViewFieldGroupInput, UpdateViewFieldInput, UpdateViewFilterGroupInput,
    UpdateViewFilterInput, UpdateViewGroupInput, UpdateViewInput, UpdateViewSortInput, User,
    UuidOp, View, ViewCalendarLayout, ViewField, ViewFieldGroup, ViewFilter, ViewFilterGroup,
    ViewFilterGroupLogicalOperator, ViewFilterOperand, ViewGroup, ViewKey, ViewOpenRecordIn,
    ViewSort, ViewSortDirection, ViewType, ViewVisibility, Workspace,
};
use uuid::Uuid;

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

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    pub api_key_id: Uuid,
    pub workspace_id: Uuid,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
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

fn compile_filter(expr: &FilterExpr, start: usize) -> FilterSql {
    let mut binds = Vec::new();
    let sql = compile_expr(expr, &mut binds, start);
    FilterSql { sql, binds }
}

fn placeholder(binds: &mut Vec<Bind>, value: Bind, start: usize) -> String {
    binds.push(value);
    format!("${}", start + binds.len() - 1)
}

fn compile_expr(expr: &FilterExpr, binds: &mut Vec<Bind>, start: usize) -> String {
    match expr {
        FilterExpr::Id(op) => compile_uuid("id", op, binds, start),
        FilterExpr::CreatedAt(op) => compile_date("created_at", op, binds, start),
        FilterExpr::UpdatedAt(op) => compile_date("updated_at", op, binds, start),
        FilterExpr::FirstName(op) => compile_string("name_first", op, binds, start),
        FilterExpr::LastName(op) => compile_string("name_last", op, binds, start),
        FilterExpr::PrimaryEmail(op) => compile_string("primary_email", op, binds, start),
        FilterExpr::JobTitle(op) => compile_string("job_title", op, binds, start),
        FilterExpr::And(exprs) => compile_join("AND", exprs, "TRUE", binds, start),
        FilterExpr::Or(exprs) => compile_join("OR", exprs, "FALSE", binds, start),
        FilterExpr::Not(expr) => format!("NOT ({})", compile_expr(expr, binds, start)),
    }
}

fn compile_join(
    operator: &str,
    exprs: &[FilterExpr],
    empty: &str,
    binds: &mut Vec<Bind>,
    start: usize,
) -> String {
    if exprs.is_empty() {
        return empty.to_string();
    }
    let parts: Vec<String> = exprs
        .iter()
        .map(|expr| compile_expr(expr, binds, start))
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

const API_KEY_SELECT: &str = "id, name, role, expires_at, revoked_at, created_at, updated_at";

fn row_to_api_key(row: &sqlx::postgres::PgRow) -> ApiKey {
    ApiKey {
        id: row.get("id"),
        name: row.get("name"),
        role: Role::parse(row.get("role")).unwrap_or(Role::Member),
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

    pub async fn find_workspace_member(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Option<squid_core::WorkspaceMember>> {
        let row = sqlx::query(
            "SELECT id, workspace_id, user_id, role, created_at
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
                created_at: row.get("created_at"),
            })
        }))
    }

    pub async fn list_workspace_members_with_users(
        &self,
        workspace_id: Uuid,
    ) -> sqlx::Result<Vec<(squid_core::WorkspaceMember, squid_core::User)>> {
        let rows = sqlx::query(
            "SELECT m.id, m.workspace_id, m.user_id, m.role, m.created_at,
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
            "SELECT id, workspace_id, role, expires_at FROM core.api_keys
             WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| ApiKeyAuth {
            api_key_id: row.get("id"),
            workspace_id: row.get("workspace_id"),
            role: Role::parse(row.get("role")).unwrap_or(Role::Member),
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
            let filter = filter.map(|filter| compile_filter(filter, 3));
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
            let filter = filter.map(|filter| compile_filter(filter, 3));
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
            let filter = filter.map(|filter| compile_filter(filter, 1));
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
                        .map(OrderTerm::sql)
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
        let filter = filter.map(|filter| compile_filter(filter, 1));
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
        let filter = filter.map(|filter| compile_filter(filter, 1));
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

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.person
                (name_first, name_last, primary_email, additional_emails, job_title,
                 linkedin_link, phones, avatar_url, position, created_by, updated_by,
                 search_vector, company_id)
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, $8, $9,
                     $10::jsonb, $11::jsonb, $12, $13)
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
                 phones = $8::jsonb, avatar_url = $9, position = $10,
                 search_vector = $11, company_id = $12, updated_at = now()
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

    pub async fn ensure_standard_metadata(&self, workspace_id: Uuid) -> anyhow::Result<()> {
        self.ensure_person_metadata(workspace_id).await?;
        self.ensure_company_metadata(workspace_id).await?;
        self.ensure_opportunity_metadata(workspace_id).await?;
        self.ensure_task_metadata(workspace_id).await?;
        self.ensure_note_metadata(workspace_id).await?;
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

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO core.object_metadata
                (workspace_id, name_singular, name_plural, label_singular, label_plural,
                 description, icon, color, shortcut, is_remote, is_active, is_system,
                 is_ui_editable, is_ui_creatable, is_searchable, open_record_in,
                 readability, sharing_reach, writability, is_label_synced_with_name)
             VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, false, true, true, true,
                     true, true, 'SIDE_PANEL', 'OPEN', 'ROLE_ACCESS', 'OPEN', false)
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

        for field in definition.fields {
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
            },
            WORKSPACE_NOTE_SQL,
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
            objects.push(object);
        }
        Ok(objects)
    }

    pub async fn get_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> anyhow::Result<Option<ObjectMetadata>> {
        self.ensure_standard_metadata(workspace_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {OBJECT_SELECT} FROM core.object_metadata
             WHERE workspace_id = $1 AND id = $2"
        ))
        .bind(workspace_id)
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut object = row_to_object(&row)?;
        object.fields = self.list_fields(workspace_id, Some(object_id)).await?;
        Ok(Some(object))
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
        let result =
            sqlx::query("DELETE FROM core.object_metadata WHERE workspace_id = $1 AND id = $2")
                .bind(workspace_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
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
        self.get_field(workspace_id, id).await
    }

    pub async fn delete_metadata_field(
        &self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> anyhow::Result<bool> {
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
        let result = sqlx::query(
            "UPDATE core.views SET
                name = COALESCE($3, name),
                type = COALESCE($4, type),
                icon = COALESCE($5, icon),
                position = COALESCE($6, position),
                is_compact = COALESCE($7, is_compact),
                open_record_in = COALESCE($8, open_record_in),
                kanban_aggregate_operation = COALESCE($9, kanban_aggregate_operation),
                kanban_aggregate_operation_field_metadata_id =
                    COALESCE($10, kanban_aggregate_operation_field_metadata_id),
                any_field_filter_value = COALESCE($11, any_field_filter_value),
                calendar_layout = COALESCE($12, calendar_layout),
                calendar_field_metadata_id = COALESCE($13, calendar_field_metadata_id),
                calendar_end_field_metadata_id = COALESCE($14, calendar_end_field_metadata_id),
                visibility = COALESCE($15, visibility),
                main_group_by_field_metadata_id =
                    COALESCE($16, main_group_by_field_metadata_id),
                should_hide_empty_groups = COALESCE($17, should_hide_empty_groups),
                kanban_column_width = COALESCE($18, kanban_column_width),
                group_load_limit = COALESCE($19, group_load_limit),
                updated_at = now()
             WHERE workspace_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(workspace_id)
        .bind(id)
        .bind(&input.name)
        .bind(input.view_type.map(|value| value.as_str()))
        .bind(&input.icon)
        .bind(input.position)
        .bind(input.is_compact)
        .bind(input.open_record_in.map(|value| value.as_str()))
        .bind(input.kanban_aggregate_operation.map(|value| value.as_str()))
        .bind(input.kanban_aggregate_operation_field_metadata_id)
        .bind(&input.any_field_filter_value)
        .bind(input.calendar_layout.map(|value| value.as_str()))
        .bind(input.calendar_field_metadata_id)
        .bind(input.calendar_end_field_metadata_id)
        .bind(input.visibility.map(|value| value.as_str()))
        .bind(input.main_group_by_field_metadata_id)
        .bind(input.should_hide_empty_groups)
        .bind(input.kanban_column_width)
        .bind(input.group_load_limit)
        .execute(&self.pool)
        .await?;
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
}

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
    })
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
    })
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
    fn sql(&self) -> String {
        format!(
            "{} {} NULLS {}",
            self.column.sql(),
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
        avatar_file: avatar_file.map(|raw| squid_core::GqlJson(parse_value(&raw))),
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

fn parse_value(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
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
