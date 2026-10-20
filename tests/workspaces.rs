use std::{fmt::Debug, time::Duration};

use sebastian_pulse::{
    database::create_pool,
    migrations::{load_migrations, migrate},
    workspaces::{NewGoal, NewProject, NewRecipient, ProjectKind, WorkspaceError, WorkspaceStore},
};
use sqlx::{Connection, PgConnection, Row, postgres::PgPoolOptions};
use uuid::Uuid;

#[allow(dead_code)]
mod support;
use support::*;

#[derive(Clone, Copy)]
struct Actors {
    workspace_a: Uuid,
    workspace_b: Uuid,
    admin_a: Uuid,
    admin_b: Uuid,
    reader_a: Uuid,
    report_only: Uuid,
    dual_admin: Uuid,
}

async fn prepare(db: &mut DbFixture) -> Actors {
    let migrations = load_migrations().expect("load workspace migrations");
    migrate(db.migration.clone(), &migrations)
        .await
        .expect("apply workspace migrations to the disposable database");

    let actors = Actors {
        workspace_a: Uuid::new_v4(),
        workspace_b: Uuid::new_v4(),
        admin_a: Uuid::new_v4(),
        admin_b: Uuid::new_v4(),
        reader_a: Uuid::new_v4(),
        report_only: Uuid::new_v4(),
        dual_admin: Uuid::new_v4(),
    };

    for principal in [
        actors.admin_a,
        actors.admin_b,
        actors.reader_a,
        actors.report_only,
        actors.dual_admin,
    ] {
        sqlx::query("INSERT INTO analytics.principals (id) VALUES ($1)")
            .bind(principal)
            .execute(&mut db.owner)
            .await
            .expect("seed a global principal through the operator connection");
    }

    for (id, name) in [
        (actors.workspace_a, "Workspace A"),
        (actors.workspace_b, "Workspace B"),
    ] {
        sqlx::query("INSERT INTO analytics.workspaces (id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&mut db.owner)
            .await
            .expect("seed a workspace through the operator connection");
    }

    for (workspace, principal, role) in [
        (actors.workspace_a, actors.admin_a, "admin"),
        (actors.workspace_b, actors.admin_b, "admin"),
        (actors.workspace_a, actors.reader_a, "reader"),
        (actors.workspace_a, actors.dual_admin, "admin"),
        (actors.workspace_b, actors.dual_admin, "admin"),
    ] {
        sqlx::query(
            "INSERT INTO analytics.workspace_members (workspace_id, principal_id, role) VALUES ($1, $2, $3)",
        )
        .bind(workspace)
        .bind(principal)
        .bind(role)
        .execute(&mut db.owner)
        .await
        .expect("seed an explicit workspace grant through the operator connection");
    }

    actors
}

fn project(name: &str) -> NewProject {
    NewProject {
        name: name.to_owned(),
        kind: ProjectKind::Website,
    }
}

fn goal(project_id: Uuid, name: &str, event_name: &str) -> NewGoal {
    NewGoal {
        project_id,
        name: name.to_owned(),
        event_name: event_name.to_owned(),
    }
}

fn recipient(email: &str) -> NewRecipient {
    NewRecipient {
        email: email.to_owned(),
    }
}

fn assert_denied<T: Debug>(result: Result<T, WorkspaceError>, operation: &str) {
    match result {
        Err(WorkspaceError::Denied) => {}
        other => panic!("{operation} should be denied, got {other:?}"),
    }
}

fn assert_not_found<T: Debug>(result: Result<T, WorkspaceError>, operation: &str) {
    match result {
        Err(WorkspaceError::NotFound) => {}
        other => panic!("{operation} should be not found, got {other:?}"),
    }
}

fn assert_conflict<T: Debug>(result: Result<T, WorkspaceError>, operation: &str) {
    match result {
        Err(WorkspaceError::Conflict) => {}
        other => panic!("{operation} should conflict, got {other:?}"),
    }
}

fn sqlstate(error: &sqlx::Error) -> Option<String> {
    match error {
        sqlx::Error::Database(database) => database.code().map(|code| code.into_owned()),
        _ => None,
    }
}

fn assert_sqlstate(error: sqlx::Error, expected: &str, operation: &str) {
    assert_eq!(
        sqlstate(&error).as_deref(),
        Some(expected),
        "{operation} should fail with SQLSTATE {expected}: {error}"
    );
}

async fn set_context(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, actor: Uuid, workspace: Uuid) {
    sqlx::query(
        "SELECT set_config('analytics.actor_id', $1, true), set_config('analytics.workspace_id', $2, true)",
    )
    .bind(actor.to_string())
    .bind(workspace.to_string())
    .fetch_one(&mut **tx)
    .await
    .expect("set transaction-local tenant context");
}

#[tokio::test]
async fn workspace_memberships_gate_store_access_and_recipient_records_grant_no_access() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());

        let project_a = store
            .create_project(actors.admin_a, actors.workspace_a, project("A site"))
            .await
            .expect("workspace A admin can create a project");
        let goal_a = store
            .create_goal(
                actors.admin_a,
                actors.workspace_a,
                goal(project_a.id, "Signup", "signup.completed"),
            )
            .await
            .expect("workspace A admin can create a goal");
        let recipient_a = store
            .create_recipient(
                actors.admin_a,
                actors.workspace_a,
                recipient("reports@example.com"),
            )
            .await
            .expect("workspace A admin can configure a report recipient");
        let project_b = store
            .create_project(actors.admin_b, actors.workspace_b, project("B site"))
            .await
            .expect("workspace B admin can create a project");
        sqlx::query(
            "INSERT INTO analytics.recipients (workspace_id, id, email) VALUES ($1, $2, 'report-only@example.com')",
        )
        .bind(actors.workspace_a)
        .bind(Uuid::new_v4())
        .execute(&mut db.owner)
        .await
        .expect("seed a report recipient without granting that recipient membership");

        assert_eq!(
            store
                .workspace(actors.admin_a, actors.workspace_a)
                .await
                .expect("workspace A admin can read its workspace")
                .id,
            actors.workspace_a
        );
        assert_eq!(
            store
                .list_projects(actors.reader_a, actors.workspace_a, None, 10)
                .await
                .expect("reader can read projects")
                .items
                .len(),
            1
        );
        assert_eq!(
            store
                .list_goals(actors.reader_a, actors.workspace_a, None, 10)
                .await
                .expect("reader can read goals")
                .items
                .len(),
            1
        );
        assert_eq!(
            store
                .list_recipients(actors.reader_a, actors.workspace_a, None, 10)
                .await
                .expect("reader can read report recipients")
                .items
                .len(),
            2
        );

        assert_denied(
            store
                .list_projects(actors.report_only, actors.workspace_a, None, 10)
                .await,
            "report-only recipient project read",
        );
        assert_denied(
            store
                .list_goals(actors.report_only, actors.workspace_a, None, 10)
                .await,
            "report-only recipient goal read",
        );
        assert_denied(
            store
                .list_recipients(actors.report_only, actors.workspace_a, None, 10)
                .await,
            "report-only recipient configuration read",
        );
        assert_denied(
            store
                .create_project(actors.report_only, actors.workspace_a, project("Blocked"))
                .await,
            "report-only recipient project write",
        );
        assert_denied(
            store
                .create_goal(
                    actors.report_only,
                    actors.workspace_a,
                    goal(project_a.id, "Blocked", "blocked.event"),
                )
                .await,
            "report-only recipient goal write",
        );
        assert_denied(
            store
                .create_recipient(
                    actors.report_only,
                    actors.workspace_a,
                    recipient("other@example.com"),
                )
                .await,
            "report-only recipient recipient write",
        );

        assert_denied(
            store
                .create_project(actors.reader_a, actors.workspace_a, project("Reader write"))
                .await,
            "reader project create",
        );
        assert_denied(
            store
                .update_project(
                    actors.reader_a,
                    actors.workspace_a,
                    project_a.id,
                    1,
                    project("Reader update"),
                )
                .await,
            "reader project update",
        );
        assert_denied(
            store
                .delete_project(actors.reader_a, actors.workspace_a, project_a.id, 1)
                .await,
            "reader project delete",
        );
        assert_denied(
            store
                .create_goal(
                    actors.reader_a,
                    actors.workspace_a,
                    goal(project_a.id, "Reader goal", "reader.goal"),
                )
                .await,
            "reader goal create",
        );
        assert_denied(
            store
                .update_goal(
                    actors.reader_a,
                    actors.workspace_a,
                    goal_a.id,
                    1,
                    goal(project_a.id, "Reader update", "reader.updated"),
                )
                .await,
            "reader goal update",
        );
        assert_denied(
            store
                .delete_goal(actors.reader_a, actors.workspace_a, goal_a.id, 1)
                .await,
            "reader goal delete",
        );
        assert_denied(
            store
                .create_recipient(
                    actors.reader_a,
                    actors.workspace_a,
                    recipient("reader@example.com"),
                )
                .await,
            "reader recipient create",
        );
        assert_denied(
            store
                .update_recipient(
                    actors.reader_a,
                    actors.workspace_a,
                    recipient_a.id,
                    1,
                    recipient("updated@example.com"),
                )
                .await,
            "reader recipient update",
        );
        assert_denied(
            store
                .delete_recipient(actors.reader_a, actors.workspace_a, recipient_a.id, 1)
                .await,
            "reader recipient delete",
        );

        assert_denied(
            store
                .list_projects(actors.admin_a, actors.workspace_b, None, 10)
                .await,
            "workspace A admin cross-workspace project read",
        );
        assert_denied(
            store
                .create_project(actors.admin_a, actors.workspace_b, project("Cross tenant"))
                .await,
            "workspace A admin cross-workspace project write",
        );
        assert_denied(
            store
                .create_goal(
                    actors.admin_a,
                    actors.workspace_b,
                    goal(project_b.id, "Cross tenant", "cross.tenant"),
                )
                .await,
            "workspace A admin cross-workspace goal write",
        );
        assert_denied(
            store
                .create_recipient(
                    actors.admin_a,
                    actors.workspace_b,
                    recipient("cross@example.com"),
                )
                .await,
            "workspace A admin cross-workspace recipient write",
        );

        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn crud_versions_audit_pagination_and_project_goal_cascade_are_enforced() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());

        let project_a = store
            .create_project(actors.admin_a, actors.workspace_a, project("Initial project"))
            .await
            .expect("create project");
        assert_eq!(project_a.version, 1);
        let project_a = store
            .update_project(
                actors.admin_a,
                actors.workspace_a,
                project_a.id,
                1,
                project("Updated project"),
            )
            .await
            .expect("update project with its current version");
        assert_eq!(project_a.version, 2);
        assert_conflict(
            store
                .update_project(
                    actors.admin_a,
                    actors.workspace_a,
                    project_a.id,
                    1,
                    project("Stale project"),
                )
                .await,
            "stale project update",
        );
        assert_conflict(
            store
                .delete_project(actors.admin_a, actors.workspace_a, project_a.id, 1)
                .await,
            "stale project delete",
        );

        let goal_a = store
            .create_goal(
                actors.admin_a,
                actors.workspace_a,
                goal(project_a.id, "Checkout", "checkout.complete"),
            )
            .await
            .expect("create goal");
        let goal_a = store
            .update_goal(
                actors.admin_a,
                actors.workspace_a,
                goal_a.id,
                1,
                goal(project_a.id, "Checkout complete", "checkout.completed"),
            )
            .await
            .expect("update goal with its current version");
        assert_eq!(goal_a.version, 2);
        assert_conflict(
            store
                .update_goal(
                    actors.admin_a,
                    actors.workspace_a,
                    goal_a.id,
                    1,
                    goal(project_a.id, "Stale goal", "checkout.stale"),
                )
                .await,
            "stale goal update",
        );
        assert_conflict(
            store
                .delete_goal(actors.admin_a, actors.workspace_a, goal_a.id, 1)
                .await,
            "stale goal delete",
        );
        store
            .delete_goal(actors.admin_a, actors.workspace_a, goal_a.id, 2)
            .await
            .expect("delete goal with its current version");

        let cascade_goal = store
            .create_goal(
                actors.admin_a,
                actors.workspace_a,
                goal(project_a.id, "Cascade goal", "cascade.goal"),
            )
            .await
            .expect("create goal that will be removed with its project");
        let recipient_a = store
            .create_recipient(
                actors.admin_a,
                actors.workspace_a,
                recipient("monthly@example.com"),
            )
            .await
            .expect("create recipient");
        let recipient_a = store
            .update_recipient(
                actors.admin_a,
                actors.workspace_a,
                recipient_a.id,
                1,
                recipient("updated.monthly@example.com"),
            )
            .await
            .expect("update recipient with its current version");
        assert_eq!(recipient_a.version, 2);
        assert_conflict(
            store
                .update_recipient(
                    actors.admin_a,
                    actors.workspace_a,
                    recipient_a.id,
                    1,
                    recipient("stale@example.com"),
                )
                .await,
            "stale recipient update",
        );
        assert_conflict(
            store
                .delete_recipient(actors.admin_a, actors.workspace_a, recipient_a.id, 1)
                .await,
            "stale recipient delete",
        );
        store
            .delete_recipient(actors.admin_a, actors.workspace_a, recipient_a.id, 2)
            .await
            .expect("delete recipient with its current version");

        assert!(matches!(
            store
                .create_project(
                    actors.admin_a,
                    actors.workspace_a,
                    NewProject {
                        name: "   ".into(),
                        kind: ProjectKind::Website,
                    },
                )
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .create_goal(
                    actors.admin_a,
                    actors.workspace_a,
                    goal(project_a.id, "Unsafe event", "signup?email=person"),
                )
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .create_recipient(actors.admin_a, actors.workspace_a, recipient("not-an-email"))
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .list_projects(actors.admin_a, actors.workspace_a, None, 0)
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .list_projects(actors.admin_a, actors.workspace_a, None, 101)
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .list_goals(actors.admin_a, actors.workspace_a, None, 0)
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));
        assert!(matches!(
            store
                .list_recipients(actors.admin_a, actors.workspace_a, None, 101)
                .await,
            Err(WorkspaceError::InvalidInput(_))
        ));

        store
            .delete_project(actors.admin_a, actors.workspace_a, project_a.id, 2)
            .await
            .expect("delete project with its current version");
        assert_eq!(
            store
                .list_goals(actors.admin_a, actors.workspace_a, None, 100)
                .await
                .expect("list goals after project deletion")
                .items
                .len(),
            0,
            "deleting a project cascades to its goals"
        );

        let audit_rows = sqlx::query(
            "SELECT action, actor_id, entity_id, entity_version FROM analytics.config_audit WHERE workspace_id = $1 ORDER BY action, entity_id",
        )
        .bind(actors.workspace_a)
        .fetch_all(&mut db.owner)
        .await
        .expect("read the minimal configuration audit through the operator connection");
        let actions = audit_rows
            .iter()
            .map(|row| row.get::<String, _>("action"))
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [
                "create_goal",
                "create_goal",
                "create_project",
                "create_recipient",
                "delete_goal",
                "delete_goal",
                "delete_project",
                "delete_recipient",
                "update_goal",
                "update_project",
                "update_recipient",
            ]
            .map(str::to_owned)
        );
        assert!(audit_rows
            .iter()
            .all(|row| row.get::<Uuid, _>("actor_id") == actors.admin_a));
        assert!(audit_rows
            .iter()
            .all(|row| row.get::<i32, _>("entity_version") > 0));
        assert!(audit_rows
            .iter()
            .any(|row| row.get::<Uuid, _>("entity_id") == cascade_goal.id));
        let cascade_audit = sqlx::query(
            "SELECT action, entity_version FROM analytics.config_audit WHERE workspace_id = $1 AND entity_id = $2 ORDER BY action",
        )
        .bind(actors.workspace_a)
        .bind(cascade_goal.id)
        .fetch_all(&mut db.owner)
        .await
        .expect("read create and cascade-delete audit entries for the goal");
        let cascade_events = cascade_audit
            .iter()
            .map(|row| (row.get::<String, _>("action"), row.get::<i32, _>("entity_version")))
            .collect::<Vec<_>>();
        assert_eq!(
            cascade_events,
            vec![("create_goal".to_owned(), 1), ("delete_goal".to_owned(), 2)]
        );

        let audit_columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns WHERE table_schema = 'analytics' AND table_name = 'config_audit' ORDER BY ordinal_position",
        )
        .fetch_all(&mut db.owner)
        .await
        .expect("inspect the audit schema");
        assert_eq!(
            audit_columns,
            [
                "workspace_id",
                "id",
                "actor_id",
                "action",
                "entity_id",
                "entity_version",
                "occurred_at",
            ]
            .map(str::to_owned)
        );

        for statement in [
            "UPDATE analytics.config_audit SET action = 'update_project'",
            "DELETE FROM analytics.config_audit",
        ] {
            let error = sqlx::query(statement)
                .execute(&pool)
                .await
                .expect_err("runtime audit records are append-only");
            assert_sqlstate(error, "42501", statement);
        }

        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn keyset_pages_are_bounded_and_have_no_duplicates() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());

        for index in 0..103 {
            store
                .create_project(
                    actors.admin_a,
                    actors.workspace_a,
                    project(&format!("Project {index:03}")),
                )
                .await
                .expect("create project for pagination");
        }

        let first = store
            .list_projects(actors.admin_a, actors.workspace_a, None, 100)
            .await
            .expect("read the first bounded page");
        assert_eq!(first.items.len(), 100);
        let cursor = first.next_after.expect("first page has another page");
        assert_eq!(
            first.items.last().expect("first page is nonempty").id,
            cursor
        );

        let second = store
            .list_projects(actors.admin_a, actors.workspace_a, Some(cursor), 100)
            .await
            .expect("read the second page");
        assert_eq!(second.items.len(), 3);
        assert!(second.next_after.is_none());
        let first_ids = first
            .items
            .iter()
            .map(|row| row.id)
            .collect::<std::collections::HashSet<_>>();
        assert!(second.items.iter().all(|row| !first_ids.contains(&row.id)));
        assert!(second.items.first().is_some_and(|row| row.id > cursor));

        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn rls_denies_missing_context_scopes_reads_and_blocks_runtime_grant_changes() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let project_a = Uuid::new_v4();
        let project_b = Uuid::new_v4();
        for (workspace, id, name) in [
            (actors.workspace_a, project_a, "A private project"),
            (actors.workspace_b, project_b, "B private project"),
        ] {
            sqlx::query(
                "INSERT INTO analytics.projects (workspace_id, id, name, kind) VALUES ($1, $2, $3, 'website')",
            )
            .bind(workspace)
            .bind(id)
            .bind(name)
            .execute(&mut db.owner)
            .await
            .expect("seed a tenant project through the operator connection");
        }

        for (workspace, actor, project_id) in [
            (actors.workspace_a, actors.admin_a, project_a),
            (actors.workspace_b, actors.admin_b, project_b),
        ] {
            sqlx::query(
                "INSERT INTO analytics.goals (workspace_id, id, project_id, name, event_name) VALUES ($1, $2, $3, 'Seed goal', 'seed.goal')",
            )
            .bind(workspace)
            .bind(Uuid::new_v4())
            .bind(project_id)
            .execute(&mut db.owner)
            .await
            .expect("seed a tenant goal through the operator connection");
            sqlx::query(
                "INSERT INTO analytics.recipients (workspace_id, id, email) VALUES ($1, $2, 'reports@example.com')",
            )
            .bind(workspace)
            .bind(Uuid::new_v4())
            .execute(&mut db.owner)
            .await
            .expect("seed a tenant recipient through the operator connection");
            sqlx::query(
                "INSERT INTO analytics.config_audit (workspace_id, id, actor_id, action, entity_id, entity_version) VALUES ($1, $2, $3, 'create_project', $4, 1)",
            )
            .bind(workspace)
            .bind(Uuid::new_v4())
            .bind(actor)
            .bind(project_id)
            .execute(&mut db.owner)
            .await
            .expect("seed a tenant audit row through the operator connection");
        }

        let pool = create_pool(db.runtime.clone());
        let mut runtime = PgConnection::connect_with(&db.runtime)
            .await
            .expect("connect as the restricted application role");
        let workspace_count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.workspaces")
            .fetch_one(&mut runtime)
            .await
            .expect("missing tenant context denies workspace reads without an error");
        let project_count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.projects")
            .fetch_one(&mut runtime)
            .await
            .expect("missing tenant context denies project reads without an error");
        let membership_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.workspace_members")
                .fetch_one(&mut runtime)
                .await
                .expect("missing tenant context denies membership reads without an error");
        let goal_count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.goals")
            .fetch_one(&mut runtime)
            .await
            .expect("missing tenant context denies goal reads without an error");
        let recipient_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.recipients")
                .fetch_one(&mut runtime)
                .await
                .expect("missing tenant context denies recipient reads without an error");
        let audit_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.config_audit")
                .fetch_one(&mut runtime)
                .await
                .expect("missing tenant context denies audit reads without an error");
        assert_eq!(
            (workspace_count, project_count, membership_count, goal_count, recipient_count, audit_count),
            (0, 0, 0, 0, 0, 0)
        );

        let no_context_write = sqlx::query(
            "INSERT INTO analytics.projects (workspace_id, id, name, kind) VALUES ($1, $2, 'No context', 'website')",
        )
        .bind(actors.workspace_a)
        .bind(Uuid::new_v4())
        .execute(&mut runtime)
        .await
        .expect_err("missing tenant context must block project writes");
        assert_sqlstate(no_context_write, "42501", "project insert without context");
        let no_context_audit = sqlx::query(
            "INSERT INTO analytics.config_audit (workspace_id, id, actor_id, action, entity_id, entity_version) VALUES ($1, $2, $3, 'create_project', $4, 1)",
        )
        .bind(actors.workspace_a)
        .bind(Uuid::new_v4())
        .bind(actors.admin_a)
        .bind(project_a)
        .execute(&mut runtime)
        .await
        .expect_err("missing tenant context must block audit writes");
        assert_sqlstate(no_context_audit, "42501", "audit insert without context");
        runtime.close().await.expect("close runtime connection");

        let mut scoped = PgConnection::connect_with(&db.runtime)
            .await
            .expect("connect for scoped row-level security checks");
        let mut tx = scoped.begin().await.expect("begin scoped read transaction");
        set_context(&mut tx, actors.admin_a, actors.workspace_a).await;
        let workspaces = sqlx::query("SELECT id FROM analytics.workspaces ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .expect("workspace context returns only the matching granted workspace");
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].get::<Uuid, _>("id"), actors.workspace_a);
        let projects = sqlx::query("SELECT id FROM analytics.projects ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .expect("workspace context returns only the matching tenant project");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].get::<Uuid, _>("id"), project_a);
        let goals = sqlx::query("SELECT workspace_id FROM analytics.goals")
            .fetch_all(&mut *tx)
            .await
            .expect("workspace context returns only the matching tenant goal");
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].get::<Uuid, _>("workspace_id"), actors.workspace_a);
        let recipients = sqlx::query("SELECT workspace_id FROM analytics.recipients")
            .fetch_all(&mut *tx)
            .await
            .expect("workspace context returns only the matching tenant recipient");
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].get::<Uuid, _>("workspace_id"), actors.workspace_a);
        let audit = sqlx::query("SELECT workspace_id FROM analytics.config_audit")
            .fetch_all(&mut *tx)
            .await
            .expect("workspace context returns only the matching tenant audit row");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].get::<Uuid, _>("workspace_id"), actors.workspace_a);
        let memberships = sqlx::query(
            "SELECT principal_id FROM analytics.workspace_members ORDER BY principal_id",
        )
        .fetch_all(&mut *tx)
        .await
        .expect("a member sees only its own membership row");
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].get::<Uuid, _>("principal_id"), actors.admin_a);
        tx.commit().await.expect("commit scoped read transaction");

        let mut cross_context_tx = scoped
            .begin()
            .await
            .expect("begin mismatched actor/workspace read transaction");
        set_context(&mut cross_context_tx, actors.admin_a, actors.workspace_b).await;
        let mismatched_workspace_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.workspaces")
                .fetch_one(&mut *cross_context_tx)
                .await
                .expect("a mismatched actor/workspace pair returns no tenant rows");
        let mismatched_project_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.projects")
                .fetch_one(&mut *cross_context_tx)
                .await
                .expect("a mismatched actor/workspace pair returns no projects");
        assert_eq!((mismatched_workspace_count, mismatched_project_count), (0, 0));
        cross_context_tx
            .commit()
            .await
            .expect("commit mismatched actor/workspace read transaction");

        let mut reader_tx = scoped.begin().await.expect("begin reader write check");
        set_context(&mut reader_tx, actors.reader_a, actors.workspace_a).await;
        let reader_write = sqlx::query(
            "INSERT INTO analytics.projects (workspace_id, id, name, kind) VALUES ($1, $2, 'Reader write', 'website')",
        )
        .bind(actors.workspace_a)
        .bind(Uuid::new_v4())
        .execute(&mut *reader_tx)
        .await
        .expect_err("reader membership must not satisfy admin write policy");
        assert_sqlstate(reader_write, "42501", "reader project insert");
        reader_tx.rollback().await.expect("roll back denied reader write");

        for statement in [
            "CREATE TABLE analytics.runtime_ddl_probe (id integer)",
            "INSERT INTO analytics.principals (id) VALUES ('00000000-0000-0000-0000-000000000001')",
            "INSERT INTO analytics.workspace_members (workspace_id, principal_id, role) VALUES ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000002', 'admin')",
            "UPDATE analytics.workspace_members SET role = 'admin'",
            "SET ROLE analytics_migrator",
        ] {
            let error = sqlx::query(statement)
                .execute(&mut scoped)
                .await
                .expect_err("runtime cannot create global identities, grants, or schema objects");
            assert_sqlstate(error, "42501", statement);
        }
        scoped.close().await.expect("close scoped connection");
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn composite_project_foreign_key_rejects_cross_workspace_goal_links() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());
        let project_b = store
            .create_project(actors.admin_b, actors.workspace_b, project("B project"))
            .await
            .expect("create project in workspace B");

        assert_not_found(
            store
                .create_goal(
                    actors.admin_a,
                    actors.workspace_a,
                    goal(project_b.id, "Cross link", "cross.link"),
                )
                .await,
            "store goal linked to another workspace's project",
        );

        let mut runtime = PgConnection::connect_with(&db.runtime)
            .await
            .expect("connect as the restricted application role");
        let mut tx = runtime.begin().await.expect("begin raw cross-link check");
        set_context(&mut tx, actors.admin_a, actors.workspace_a).await;
        let error = sqlx::query(
            "INSERT INTO analytics.goals (workspace_id, id, project_id, name, event_name) VALUES ($1, $2, $3, 'Cross link', 'cross.link')",
        )
        .bind(actors.workspace_a)
        .bind(Uuid::new_v4())
        .bind(project_b.id)
        .execute(&mut *tx)
        .await
        .expect_err("composite workspace/project foreign key must reject the cross-link");
        assert_sqlstate(error, "23503", "cross-workspace goal insert");
        tx.rollback().await.expect("roll back the invalid cross-link");
        runtime.close().await.expect("close runtime connection");
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn transaction_local_context_does_not_leak_through_a_single_connection_pool() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(db.runtime.clone())
            .await
            .expect("connect one restricted pooled connection");
        let store = WorkspaceStore::new(pool.clone());

        assert_eq!(
            store
                .workspace(actors.dual_admin, actors.workspace_a)
                .await
                .expect("dual admin can explicitly access workspace A")
                .id,
            actors.workspace_a
        );
        assert_eq!(
            store
                .workspace(actors.dual_admin, actors.workspace_b)
                .await
                .expect("dual admin can explicitly access workspace B")
                .id,
            actors.workspace_b
        );
        assert_denied(
            store.workspace(actors.admin_a, actors.workspace_b).await,
            "admin A access to workspace B after dual-admin access",
        );
        assert_eq!(
            store
                .workspace(actors.admin_a, actors.workspace_a)
                .await
                .expect("admin A can still access workspace A")
                .id,
            actors.workspace_a
        );

        let leaked_context = async {
            let (actor, workspace): (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT NULLIF(current_setting('analytics.actor_id', true), ''), NULLIF(current_setting('analytics.workspace_id', true), '')",
            )
            .fetch_one(&pool)
            .await
            .expect("read current tenant settings on the reused pooled connection");
            assert!(actor.is_none(), "actor setting leaked between transactions");
            assert!(workspace.is_none(), "workspace setting leaked between transactions");
        };
        leaked_context.await;

        // A denied operation exits after setting local context. Reusing the one
        // pooled connection must not retain the failed operation's actor/workspace.
        assert_denied(
            store.workspace(actors.admin_a, actors.workspace_b).await,
            "failed store operation",
        );
        let (actor, workspace): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT NULLIF(current_setting('analytics.actor_id', true), ''), NULLIF(current_setting('analytics.workspace_id', true), '')",
        )
        .fetch_one(&pool)
        .await
        .expect("read settings after a failed store transaction");
        assert!(actor.is_none() && workspace.is_none());

        let mut dropped = pool.begin().await.expect("begin a transaction to drop");
        set_context(&mut dropped, actors.admin_a, actors.workspace_a).await;
        drop(dropped);
        let (actor, workspace): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT NULLIF(current_setting('analytics.actor_id', true), ''), NULLIF(current_setting('analytics.workspace_id', true), '')",
        )
        .fetch_one(&pool)
        .await
        .expect("pool rolls back a dropped transaction before reuse");
        assert!(actor.is_none() && workspace.is_none());

        let cancellation_pool = pool.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let canceled = tokio::spawn(async move {
            let mut tx = cancellation_pool
                .begin()
                .await
                .expect("begin a transaction that will be canceled");
            set_context(&mut tx, actors.admin_a, actors.workspace_a).await;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
            tx.commit().await.expect("unreachable canceled transaction commit");
        });
        started_rx
            .await
            .expect("canceled task set its local tenant context");
        canceled.abort();
        let _ = canceled.await;
        tokio::time::timeout(Duration::from_secs(3), async {
            let (actor, workspace): (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT NULLIF(current_setting('analytics.actor_id', true), ''), NULLIF(current_setting('analytics.workspace_id', true), '')",
            )
            .fetch_one(&pool)
            .await
            .expect("read tenant settings after task cancellation");
            assert!(actor.is_none() && workspace.is_none());
        })
        .await
        .expect("canceled transaction returns the sole connection to the pool");

        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn revoking_a_membership_blocks_the_next_store_operation() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());
        store
            .create_project(actors.admin_a, actors.workspace_a, project("Before revoke"))
            .await
            .expect("admin can write before membership revocation");

        sqlx::query(
            "DELETE FROM analytics.workspace_members WHERE workspace_id = $1 AND principal_id = $2",
        )
        .bind(actors.workspace_a)
        .bind(actors.admin_a)
        .execute(&mut db.owner)
        .await
        .expect("operator revokes the workspace membership");

        assert_denied(
            store
                .list_projects(actors.admin_a, actors.workspace_a, None, 10)
                .await,
            "next store read after membership revocation",
        );
        assert_denied(
            store
                .create_project(actors.admin_a, actors.workspace_a, project("After revoke"))
                .await,
            "next store write after membership revocation",
        );
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn concurrent_compare_and_swap_updates_create_one_winner_and_one_audit_row() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());
        let initial = store
            .create_project(actors.admin_a, actors.workspace_a, project("Original"))
            .await
            .expect("create project for concurrent updates");

        let first_store = store.clone();
        let second_store = store.clone();
        let first = first_store.update_project(
            actors.admin_a,
            actors.workspace_a,
            initial.id,
            1,
            project("First writer"),
        );
        let second = second_store.update_project(
            actors.admin_a,
            actors.workspace_a,
            initial.id,
            1,
            project("Second writer"),
        );
        let (first, second) = tokio::join!(first, second);
        let outcomes = [first, second];
        let winners = outcomes
            .iter()
            .filter(|result| matches!(result, Ok(row) if row.version == 2))
            .count();
        let conflicts = outcomes
            .iter()
            .filter(|result| matches!(result, Err(WorkspaceError::Conflict)))
            .count();
        assert_eq!(winners, 1);
        assert_eq!(conflicts, 1);

        let audit_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM analytics.config_audit WHERE workspace_id = $1 AND entity_id = $2 AND action = 'update_project'",
        )
        .bind(actors.workspace_a)
        .bind(initial.id)
        .fetch_one(&mut db.owner)
        .await
        .expect("count successful update audit records");
        assert_eq!(audit_count, 1, "a losing CAS writer must not add audit");
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn audit_insert_failure_rolls_back_the_configuration_mutation() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        sqlx::query("REVOKE INSERT ON analytics.config_audit FROM analytics_runtime")
            .execute(&mut db.owner)
            .await
            .expect("temporarily remove audit insert permission from the runtime group");

        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());
        let result = store
            .create_project(
                actors.admin_a,
                actors.workspace_a,
                project("Must roll back"),
            )
            .await;
        let error = match result {
            Err(WorkspaceError::Database(error)) => error,
            other => panic!("failed audit insert should return a database error, got {other:?}"),
        };
        assert_eq!(
            sqlstate(&error).as_deref(),
            Some("42501"),
            "audit insert must fail because its runtime grant was removed"
        );
        pool.close().await;

        let project_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM analytics.projects WHERE workspace_id = $1")
                .bind(actors.workspace_a)
                .fetch_one(&mut db.owner)
                .await
                .expect("check whether the failed operation left a project");
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM analytics.config_audit WHERE workspace_id = $1",
        )
        .bind(actors.workspace_a)
        .fetch_one(&mut db.owner)
        .await
        .expect("check whether the failed operation left an audit row");
        assert_eq!(
            project_count, 0,
            "project insert must roll back with its audit"
        );
        assert_eq!(audit_count, 0);

        sqlx::query("GRANT INSERT ON analytics.config_audit TO analytics_runtime")
            .execute(&mut db.owner)
            .await
            .expect("restore the audit insert permission before fixture cleanup");
    })
    .await;
}

#[tokio::test]
async fn final_live_revision_still_allows_audited_deletion_including_goals() {
    with_fixture(|mut db| async move {
        let actors = prepare(&mut db).await;
        let pool = create_pool(db.runtime.clone());
        let store = WorkspaceStore::new(pool.clone());
        let item = store.create_project(actors.admin_a, actors.workspace_a, project("Boundary")).await.unwrap();
        let child = store.create_goal(actors.admin_a, actors.workspace_a, goal(item.id, "Boundary goal", "boundary.goal")).await.unwrap();
        let viewer = store.create_recipient(actors.admin_a, actors.workspace_a, recipient("boundary@example.com")).await.unwrap();
        let penultimate = i32::MAX - 2;
        // A trusted fixture jumps close to the boundary without billions of edits.
        sqlx::query("UPDATE analytics.projects SET version = $1 WHERE workspace_id = $2 AND id = $3").bind(penultimate).bind(actors.workspace_a).bind(item.id).execute(&mut db.owner).await.unwrap();
        sqlx::query("UPDATE analytics.goals SET version = $1 WHERE workspace_id = $2 AND id = $3").bind(penultimate).bind(actors.workspace_a).bind(child.id).execute(&mut db.owner).await.unwrap();
        sqlx::query("UPDATE analytics.recipients SET version = $1 WHERE workspace_id = $2 AND id = $3").bind(penultimate).bind(actors.workspace_a).bind(viewer.id).execute(&mut db.owner).await.unwrap();
        let item = store.update_project(actors.admin_a, actors.workspace_a, item.id, penultimate, project("Final project revision")).await.unwrap();
        let child = store.update_goal(actors.admin_a, actors.workspace_a, child.id, penultimate, goal(item.id, "Final goal revision", "boundary.goal")).await.unwrap();
        let viewer = store.update_recipient(actors.admin_a, actors.workspace_a, viewer.id, penultimate, recipient("final@example.com")).await.unwrap();
        assert_eq!(item.version, i32::MAX - 1);
        assert_eq!(child.version, i32::MAX - 1);
        assert_eq!(viewer.version, i32::MAX - 1);
        assert!(matches!(store.update_project(actors.admin_a, actors.workspace_a, item.id, item.version, project("Overflow")).await, Err(WorkspaceError::InvalidInput(_))));
        assert!(matches!(store.update_goal(actors.admin_a, actors.workspace_a, child.id, child.version, goal(item.id, "Overflow", "overflow.goal")).await, Err(WorkspaceError::InvalidInput(_))));
        assert!(matches!(store.update_recipient(actors.admin_a, actors.workspace_a, viewer.id, viewer.version, recipient("overflow@example.com")).await, Err(WorkspaceError::InvalidInput(_))));
        store.delete_project(actors.admin_a, actors.workspace_a, item.id, item.version).await.unwrap();
        store.delete_recipient(actors.admin_a, actors.workspace_a, viewer.id, viewer.version).await.unwrap();
        let deleted: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.config_audit WHERE workspace_id = $1 AND action LIKE 'delete_%' AND entity_version = $2").bind(actors.workspace_a).bind(i32::MAX).fetch_one(&mut db.owner).await.unwrap();
        assert_eq!(deleted, 3, "project, goal, and recipient deletions retain their final audit revision");
        assert!(store.list_projects(actors.admin_a, actors.workspace_a, None, 100).await.unwrap().items.is_empty());
        pool.close().await;
    }).await;
}
