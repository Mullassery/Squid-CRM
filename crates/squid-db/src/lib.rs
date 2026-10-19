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
use squid_core::{Actor, CreatePersonInput, Emails, FullName, Person, Role, User, Workspace};
use uuid::Uuid;

const CORE_SCHEMA_SQL: &str = include_str!("../migrations/core/0001_core.sql");
const WORKSPACE_PERSON_SQL: &str = include_str!("../migrations/workspace/0001_person.sql");
const WORKSPACE_PERSON_CONTRACT_SQL: &str =
    include_str!("../migrations/workspace/0002_person_contract.sql");

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub expires_at: DateTime<Utc>,
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

        let result = sqlx::raw_sql(CORE_SCHEMA_SQL).execute(&mut *conn).await;

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

        for ddl in [WORKSPACE_PERSON_SQL, WORKSPACE_PERSON_CONTRACT_SQL] {
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

    // ----- person --------------------------------------------------------

    /// One page of people plus connection metadata.
    pub async fn list_persons_page(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
        starting_after: Option<Uuid>,
        ending_before: Option<Uuid>,
        order: &[OrderTerm],
    ) -> anyhow::Result<PersonPage> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let total = self.count_persons(workspace_id).await?;
        let limit = limit.clamp(1, 1000);
        let fetch = limit + 1;

        let has_next_page;
        let has_previous_page;
        let mut rows = if let Some(cursor_id) = starting_after {
            let cursor_created_at = self.person_created_at(&schema, cursor_id).await?;
            has_previous_page = true;
            let rows = sqlx::query(&format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL AND (created_at, id) > ($1, $2)
                 ORDER BY created_at ASC, id ASC
                 LIMIT $3"
            ))
            .bind(cursor_created_at)
            .bind(cursor_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await?;
            has_next_page = rows.len() as i64 > limit;
            rows
        } else if let Some(cursor_id) = ending_before {
            let cursor_created_at = self.person_created_at(&schema, cursor_id).await?;
            let mut rows = sqlx::query(&format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL AND (created_at, id) < ($1, $2)
                 ORDER BY created_at DESC, id DESC
                 LIMIT $3"
            ))
            .bind(cursor_created_at)
            .bind(cursor_id)
            .bind(fetch)
            .fetch_all(&self.pool)
            .await?;
            has_previous_page = rows.len() as i64 > limit;
            has_next_page = true;
            rows.truncate(limit as usize);
            rows.reverse();
            rows
        } else {
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
            let rows = sqlx::query(&format!(
                "SELECT {PERSON_SELECT} FROM {schema}.person
                 WHERE deleted_at IS NULL
                 ORDER BY {order_sql}
                 LIMIT $1 OFFSET $2"
            ))
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
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
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {schema}.person WHERE deleted_at IS NULL"
        ))
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
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
        let search_vector = build_search_vector(input);

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

fn build_search_vector(input: &CreatePersonInput) -> Option<String> {
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
    push(&input.name.first_name);
    push(&input.name.last_name);
    push(&input.emails.primary_email);
    if let Some(job_title) = input.job_title.as_deref() {
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
        created_by: parse_json(row.get("created_by")),
        updated_by: parse_json(row.get("updated_by")),
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
