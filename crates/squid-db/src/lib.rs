//! PostgreSQL access layer: connection pool, core migrations, per-workspace
//! schema management, and the Phase 4 repositories.
//!
//! Tenant isolation invariant: every workspace-scoped query interpolates a
//! *validated* schema name derived from a UUID. No caller-supplied SQL fragment
//! ever reaches the database.

use chrono::{DateTime, Utc};
use regex::Regex;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use squid_core::{CreatePersonInput, Emails, FullName, Person, Role, User, Workspace};
use uuid::Uuid;

const CORE_SCHEMA_SQL: &str = include_str!("../migrations/core/0001_core.sql");
const WORKSPACE_PERSON_SQL: &str = include_str!("../migrations/workspace/0001_person.sql");

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

        let ddl = WORKSPACE_PERSON_SQL.replace("{{schema}}", &schema);
        sqlx::raw_sql(&ddl).execute(&self.pool).await?;

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

    pub async fn list_persons(
        &self,
        workspace_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Person>> {
        let schema = Self::workspace_schema_name(workspace_id);
        assert_valid_schema(&schema)?;

        let rows = sqlx::query(&format!(
            "SELECT id, name_first, name_last, primary_email, additional_emails,
                    job_title, created_at, updated_at
             FROM {schema}.person
             WHERE deleted_at IS NULL
             ORDER BY created_at ASC, id ASC
             LIMIT $1 OFFSET $2"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(row_to_person).collect())
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
            "SELECT id, name_first, name_last, primary_email, additional_emails,
                    job_title, created_at, updated_at
             FROM {schema}.person
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

        let additional = input.emails.additional_emails.clone().unwrap_or_default();

        let row = sqlx::query(&format!(
            "INSERT INTO {schema}.person
                (name_first, name_last, primary_email, additional_emails, job_title)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, name_first, name_last, primary_email, additional_emails,
                       job_title, created_at, updated_at"
        ))
        .bind(&input.name.first_name)
        .bind(&input.name.last_name)
        .bind(&input.emails.primary_email)
        .bind(&additional)
        .bind(&input.job_title)
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
            "UPDATE {schema}.person SET deleted_at = now()
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(person_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn row_to_person(row: &sqlx::postgres::PgRow) -> Person {
    let additional: Vec<String> = row.get("additional_emails");
    Person {
        id: row.get("id"),
        name: FullName {
            first_name: row.get("name_first"),
            last_name: row.get("name_last"),
        },
        emails: Emails {
            primary_email: row.get("primary_email"),
            additional_emails: if additional.is_empty() {
                None
            } else {
                Some(additional)
            },
        },
        job_title: row.get("job_title"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
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
