use super::{
    Goal, NewGoal, NewProject, NewRecipient, Page, Project, Recipient, Result, Workspace,
    WorkspaceError,
};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkspaceStore {
    pool: PgPool,
}

impl WorkspaceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn workspace(&self, actor: Uuid, workspace: Uuid) -> Result<Workspace> {
        let mut tx = self.begin_scope(actor, workspace, false).await?;
        let workspace_row = sqlx::query_as!(
            Workspace,
            r#"
            SELECT id, name, version, created_at, updated_at
            FROM analytics.workspaces
            WHERE id = $1
            "#,
            workspace
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(WorkspaceError::NotFound)?;

        tx.commit().await?;
        Ok(workspace_row)
    }

    pub async fn list_projects(
        &self,
        actor: Uuid,
        workspace: Uuid,
        after: Option<Uuid>,
        limit: u16,
    ) -> Result<Page<Project>> {
        let fetch_limit = pagination_limit(limit)?;
        let mut tx = self.begin_scope(actor, workspace, false).await?;
        let mut rows = sqlx::query_as!(
            Project,
            r#"
            SELECT workspace_id, id, name, kind, version, created_at, updated_at
            FROM analytics.projects
            WHERE workspace_id = $1
              AND ($2::uuid IS NULL OR id > $2)
            ORDER BY id
            LIMIT $3
            "#,
            workspace,
            after,
            fetch_limit
        )
        .fetch_all(&mut *tx)
        .await?;

        let page = finish_page(&mut rows, limit);
        tx.commit().await?;
        Ok(page)
    }

    pub async fn list_goals(
        &self,
        actor: Uuid,
        workspace: Uuid,
        after: Option<Uuid>,
        limit: u16,
    ) -> Result<Page<Goal>> {
        let fetch_limit = pagination_limit(limit)?;
        let mut tx = self.begin_scope(actor, workspace, false).await?;
        let mut rows = sqlx::query_as!(
            Goal,
            r#"
            SELECT workspace_id, id, project_id, name, event_name, version, created_at, updated_at
            FROM analytics.goals
            WHERE workspace_id = $1
              AND ($2::uuid IS NULL OR id > $2)
            ORDER BY id
            LIMIT $3
            "#,
            workspace,
            after,
            fetch_limit
        )
        .fetch_all(&mut *tx)
        .await?;

        let page = finish_page(&mut rows, limit);
        tx.commit().await?;
        Ok(page)
    }

    pub async fn list_recipients(
        &self,
        actor: Uuid,
        workspace: Uuid,
        after: Option<Uuid>,
        limit: u16,
    ) -> Result<Page<Recipient>> {
        let fetch_limit = pagination_limit(limit)?;
        let mut tx = self.begin_scope(actor, workspace, false).await?;
        let mut rows = sqlx::query_as!(
            Recipient,
            r#"
            SELECT workspace_id, id, email, version, created_at, updated_at
            FROM analytics.recipients
            WHERE workspace_id = $1
              AND ($2::uuid IS NULL OR id > $2)
            ORDER BY id
            LIMIT $3
            "#,
            workspace,
            after,
            fetch_limit
        )
        .fetch_all(&mut *tx)
        .await?;

        let page = finish_page(&mut rows, limit);
        tx.commit().await?;
        Ok(page)
    }

    pub async fn create_project(
        &self,
        actor: Uuid,
        workspace: Uuid,
        input: NewProject,
    ) -> Result<Project> {
        input.validate()?;
        let NewProject { name, kind } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let id = Uuid::new_v4();
        let row = sqlx::query_as!(
            Project,
            r#"
            INSERT INTO analytics.projects (workspace_id, id, name, kind)
            VALUES ($1, $2, $3, $4)
            RETURNING workspace_id, id, name, kind, version, created_at, updated_at
            "#,
            workspace,
            id,
            name,
            kind.as_str()
        )
        .fetch_one(&mut *tx)
        .await?;

        insert_audit(
            &mut tx,
            workspace,
            actor,
            "create_project",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn update_project(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
        input: NewProject,
    ) -> Result<Project> {
        input.validate()?;
        check_expected_version(expected_version)?;
        let NewProject { name, kind } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let row = sqlx::query_as!(
            Project,
            r#"
            UPDATE analytics.projects
            SET name = $1, kind = $2, version = version + 1, updated_at = now()
            WHERE workspace_id = $3 AND id = $4 AND version = $5
            RETURNING workspace_id, id, name, kind, version, created_at, updated_at
            "#,
            name,
            kind.as_str(),
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            return Err(cas_miss_project(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "update_project",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn delete_project(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
    ) -> Result<()> {
        check_expected_version(expected_version)?;
        let next_version = expected_version
            .checked_add(1)
            .ok_or(WorkspaceError::InvalidInput(
                "version must be less than i32::MAX",
            ))?;
        let mut tx = self.begin_scope(actor, workspace, true).await?;

        let current_version = sqlx::query_scalar!(
            r#"
            SELECT version
            FROM analytics.projects
            WHERE workspace_id = $1 AND id = $2
            FOR UPDATE
            "#,
            workspace,
            id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(current_version) = current_version else {
            return Err(WorkspaceError::NotFound);
        };
        if current_version != expected_version {
            return Err(WorkspaceError::Conflict);
        }

        sqlx::query!(
            r#"
            WITH deleted_goals AS (
                DELETE FROM analytics.goals
                WHERE workspace_id = $1 AND project_id = $2
                RETURNING workspace_id, id, version
            )
            INSERT INTO analytics.config_audit
                (workspace_id, id, actor_id, action, entity_id, entity_version)
            SELECT
                workspace_id,
                gen_random_uuid(),
                $3,
                'delete_goal',
                id,
                version + 1
            FROM deleted_goals
            "#,
            workspace,
            id,
            actor
        )
        .execute(&mut *tx)
        .await?;

        let deleted_id = sqlx::query_scalar!(
            r#"
            DELETE FROM analytics.projects
            WHERE workspace_id = $1 AND id = $2 AND version = $3
            RETURNING id
            "#,
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(deleted_id) = deleted_id else {
            return Err(cas_miss_project(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "delete_project",
            deleted_id,
            next_version,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_goal(&self, actor: Uuid, workspace: Uuid, input: NewGoal) -> Result<Goal> {
        input.validate()?;
        let NewGoal {
            project_id,
            name,
            event_name,
        } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        require_project(&mut tx, workspace, project_id).await?;
        let id = Uuid::new_v4();
        let row = sqlx::query_as!(
            Goal,
            r#"
            INSERT INTO analytics.goals (workspace_id, id, project_id, name, event_name)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING workspace_id, id, project_id, name, event_name, version, created_at, updated_at
            "#,
            workspace,
            id,
            project_id,
            name,
            event_name
        )
        .fetch_one(&mut *tx)
        .await?;

        insert_audit(
            &mut tx,
            workspace,
            actor,
            "create_goal",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn update_goal(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
        input: NewGoal,
    ) -> Result<Goal> {
        input.validate()?;
        check_expected_version(expected_version)?;
        let NewGoal {
            project_id,
            name,
            event_name,
        } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        require_project(&mut tx, workspace, project_id).await?;
        let row = sqlx::query_as!(
            Goal,
            r#"
            UPDATE analytics.goals
            SET project_id = $1, name = $2, event_name = $3,
                version = version + 1, updated_at = now()
            WHERE workspace_id = $4 AND id = $5 AND version = $6
            RETURNING workspace_id, id, project_id, name, event_name, version, created_at, updated_at
            "#,
            project_id,
            name,
            event_name,
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            return Err(cas_miss_goal(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "update_goal",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn delete_goal(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
    ) -> Result<()> {
        check_expected_version(expected_version)?;
        let next_version = expected_version
            .checked_add(1)
            .ok_or(WorkspaceError::InvalidInput(
                "version must be less than i32::MAX",
            ))?;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let deleted_id = sqlx::query_scalar!(
            r#"
            DELETE FROM analytics.goals
            WHERE workspace_id = $1 AND id = $2 AND version = $3
            RETURNING id
            "#,
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(deleted_id) = deleted_id else {
            return Err(cas_miss_goal(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "delete_goal",
            deleted_id,
            next_version,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_recipient(
        &self,
        actor: Uuid,
        workspace: Uuid,
        input: NewRecipient,
    ) -> Result<Recipient> {
        input.validate()?;
        let NewRecipient { email } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let id = Uuid::new_v4();
        let row = sqlx::query_as!(
            Recipient,
            r#"
            INSERT INTO analytics.recipients (workspace_id, id, email)
            VALUES ($1, $2, $3)
            RETURNING workspace_id, id, email, version, created_at, updated_at
            "#,
            workspace,
            id,
            email
        )
        .fetch_one(&mut *tx)
        .await?;

        insert_audit(
            &mut tx,
            workspace,
            actor,
            "create_recipient",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn update_recipient(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
        input: NewRecipient,
    ) -> Result<Recipient> {
        input.validate()?;
        check_expected_version(expected_version)?;
        let NewRecipient { email } = input;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let row = sqlx::query_as!(
            Recipient,
            r#"
            UPDATE analytics.recipients
            SET email = $1, version = version + 1, updated_at = now()
            WHERE workspace_id = $2 AND id = $3 AND version = $4
            RETURNING workspace_id, id, email, version, created_at, updated_at
            "#,
            email,
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            return Err(cas_miss_recipient(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "update_recipient",
            row.id,
            row.version,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn delete_recipient(
        &self,
        actor: Uuid,
        workspace: Uuid,
        id: Uuid,
        expected_version: i32,
    ) -> Result<()> {
        check_expected_version(expected_version)?;
        let next_version = expected_version
            .checked_add(1)
            .ok_or(WorkspaceError::InvalidInput(
                "version must be less than i32::MAX",
            ))?;
        let mut tx = self.begin_scope(actor, workspace, true).await?;
        let deleted_id = sqlx::query_scalar!(
            r#"
            DELETE FROM analytics.recipients
            WHERE workspace_id = $1 AND id = $2 AND version = $3
            RETURNING id
            "#,
            workspace,
            id,
            expected_version
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(deleted_id) = deleted_id else {
            return Err(cas_miss_recipient(&mut tx, workspace, id).await?);
        };
        insert_audit(
            &mut tx,
            workspace,
            actor,
            "delete_recipient",
            deleted_id,
            next_version,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn begin_scope(
        &self,
        actor: Uuid,
        workspace: Uuid,
        write: bool,
    ) -> Result<Transaction<'_, Postgres>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            r#"
            SELECT
                set_config('analytics.actor_id', $1, true) AS actor_id,
                set_config('analytics.workspace_id', $2, true) AS workspace_id
            "#,
            actor.to_string(),
            workspace.to_string()
        )
        .fetch_one(&mut *tx)
        .await?;

        let role = sqlx::query_scalar!(
            r#"
            SELECT role
            FROM analytics.workspace_members
            WHERE workspace_id = $1 AND principal_id = $2
            "#,
            workspace,
            actor
        )
        .fetch_optional(&mut *tx)
        .await?;

        match role.as_deref() {
            Some("admin") => Ok(tx),
            Some("reader") if !write => Ok(tx),
            _ => Err(WorkspaceError::Denied),
        }
    }
}

fn pagination_limit(limit: u16) -> Result<i64> {
    if !(1..=100).contains(&limit) {
        return Err(WorkspaceError::InvalidInput(
            "limit must be between 1 and 100",
        ));
    }
    Ok(i64::from(limit) + 1)
}

fn finish_page<T>(rows: &mut Vec<T>, limit: u16) -> Page<T>
where
    T: HasId,
{
    let has_more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let next_after = if has_more {
        rows.last().map(|item| item.id())
    } else {
        None
    };
    Page {
        items: std::mem::take(rows),
        next_after,
    }
}

trait HasId {
    fn id(&self) -> Uuid;
}

impl HasId for Project {
    fn id(&self) -> Uuid {
        self.id
    }
}

impl HasId for Goal {
    fn id(&self) -> Uuid {
        self.id
    }
}

impl HasId for Recipient {
    fn id(&self) -> Uuid {
        self.id
    }
}

fn check_expected_version(version: i32) -> Result<()> {
    if version <= 0 {
        return Err(WorkspaceError::InvalidInput("version must be positive"));
    }
    if version == i32::MAX {
        return Err(WorkspaceError::InvalidInput(
            "version must be less than i32::MAX",
        ));
    }
    Ok(())
}

async fn require_project(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    project: Uuid,
) -> Result<()> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT id
        FROM analytics.projects
        WHERE workspace_id = $1 AND id = $2
        "#,
        workspace,
        project
    )
    .fetch_optional(&mut **tx)
    .await?;

    if found.is_some() {
        Ok(())
    } else {
        Err(WorkspaceError::NotFound)
    }
}

async fn cas_miss_project(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    id: Uuid,
) -> Result<WorkspaceError> {
    let found = sqlx::query_scalar!(
        "SELECT id FROM analytics.projects WHERE workspace_id = $1 AND id = $2",
        workspace,
        id
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(if found.is_some() {
        WorkspaceError::Conflict
    } else {
        WorkspaceError::NotFound
    })
}

async fn cas_miss_goal(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    id: Uuid,
) -> Result<WorkspaceError> {
    let found = sqlx::query_scalar!(
        "SELECT id FROM analytics.goals WHERE workspace_id = $1 AND id = $2",
        workspace,
        id
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(if found.is_some() {
        WorkspaceError::Conflict
    } else {
        WorkspaceError::NotFound
    })
}

async fn cas_miss_recipient(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    id: Uuid,
) -> Result<WorkspaceError> {
    let found = sqlx::query_scalar!(
        "SELECT id FROM analytics.recipients WHERE workspace_id = $1 AND id = $2",
        workspace,
        id
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(if found.is_some() {
        WorkspaceError::Conflict
    } else {
        WorkspaceError::NotFound
    })
}

async fn insert_audit(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    actor: Uuid,
    action: &'static str,
    entity: Uuid,
    entity_version: i32,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO analytics.config_audit
            (workspace_id, id, actor_id, action, entity_id, entity_version)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        workspace,
        Uuid::new_v4(),
        actor,
        action,
        entity,
        entity_version
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}
