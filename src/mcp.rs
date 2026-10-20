//! Stateless Streamable HTTP MCP adapter for workspace configuration.
//!
//! Authentication is provided by the outer Axum router. Each tool reads the
//! verified caller from the HTTP request parts carried in RMCP's request
//! context; caller identity is never accepted as a tool argument or retained
//! in an MCP session.

use crate::workspaces::{
    Goal, NewGoal, NewProject, NewRecipient, Page, Project, ProjectKind, Recipient, Workspace,
    WorkspaceError, WorkspaceStore,
};
use axum::Router;
use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Build the MCP service router with an explicit `/mcp` route so the parent
/// router's route layer can protect every request to the MCP transport.
pub fn router(store: WorkspaceStore, host: String, origin: String) -> Router {
    let service = StreamableHttpService::new(
        move || Ok(McpHandler::new(store.clone())),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_allowed_hosts([host])
            .with_allowed_origins([origin])
            .enforce_origin_validation(),
    );

    Router::new().route_service("/mcp", service)
}

#[derive(Clone)]
struct McpHandler {
    store: WorkspaceStore,
}

impl McpHandler {
    fn new(store: WorkspaceStore) -> Self {
        Self { store }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkspaceGetArgs {
    /// Workspace UUID granted to the authenticated principal.
    workspace_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    /// Workspace UUID granted to the authenticated principal.
    workspace_id: String,
    /// Return rows after this UUID cursor.
    after: Option<String>,
    /// Page size, from 1 to 100. Defaults to 50.
    limit: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ProjectKindArg {
    Website,
    Product,
    Cli,
}

impl From<ProjectKindArg> for ProjectKind {
    fn from(kind: ProjectKindArg) -> Self {
        match kind {
            ProjectKindArg::Website => Self::Website,
            ProjectKindArg::Product => Self::Product,
            ProjectKindArg::Cli => Self::Cli,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectCreateArgs {
    workspace_id: String,
    name: String,
    kind: ProjectKindArg,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectUpdateArgs {
    workspace_id: String,
    id: String,
    expected_version: i32,
    name: String,
    kind: ProjectKindArg,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteArgs {
    workspace_id: String,
    id: String,
    expected_version: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GoalCreateArgs {
    workspace_id: String,
    project_id: String,
    name: String,
    event_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GoalUpdateArgs {
    workspace_id: String,
    id: String,
    expected_version: i32,
    project_id: String,
    name: String,
    event_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecipientCreateArgs {
    workspace_id: String,
    email: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecipientUpdateArgs {
    workspace_id: String,
    id: String,
    expected_version: i32,
    email: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WorkspaceOutput {
    id: String,
    name: String,
    version: i32,
    created_at: String,
    updated_at: String,
}

impl From<Workspace> for WorkspaceOutput {
    fn from(row: Workspace) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            version: row.version,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ProjectOutput {
    workspace_id: String,
    id: String,
    name: String,
    kind: String,
    version: i32,
    created_at: String,
    updated_at: String,
}

impl From<Project> for ProjectOutput {
    fn from(row: Project) -> Self {
        Self {
            workspace_id: row.workspace_id.to_string(),
            id: row.id.to_string(),
            name: row.name,
            kind: row.kind,
            version: row.version,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct GoalOutput {
    workspace_id: String,
    id: String,
    project_id: String,
    name: String,
    event_name: String,
    version: i32,
    created_at: String,
    updated_at: String,
}

impl From<Goal> for GoalOutput {
    fn from(row: Goal) -> Self {
        Self {
            workspace_id: row.workspace_id.to_string(),
            id: row.id.to_string(),
            project_id: row.project_id.to_string(),
            name: row.name,
            event_name: row.event_name,
            version: row.version,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct RecipientOutput {
    workspace_id: String,
    id: String,
    email: String,
    version: i32,
    created_at: String,
    updated_at: String,
}

impl From<Recipient> for RecipientOutput {
    fn from(row: Recipient) -> Self {
        Self {
            workspace_id: row.workspace_id.to_string(),
            id: row.id.to_string(),
            email: row.email,
            version: row.version,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ProjectPageOutput {
    items: Vec<ProjectOutput>,
    next_after: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GoalPageOutput {
    items: Vec<GoalOutput>,
    next_after: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct RecipientPageOutput {
    items: Vec<RecipientOutput>,
    next_after: Option<String>,
}

impl From<Page<Project>> for ProjectPageOutput {
    fn from(page: Page<Project>) -> Self {
        Self {
            items: page.items.into_iter().map(ProjectOutput::from).collect(),
            next_after: page.next_after.map(|id| id.to_string()),
        }
    }
}

impl From<Page<Goal>> for GoalPageOutput {
    fn from(page: Page<Goal>) -> Self {
        Self {
            items: page.items.into_iter().map(GoalOutput::from).collect(),
            next_after: page.next_after.map(|id| id.to_string()),
        }
    }
}

impl From<Page<Recipient>> for RecipientPageOutput {
    fn from(page: Page<Recipient>) -> Self {
        Self {
            items: page.items.into_iter().map(RecipientOutput::from).collect(),
            next_after: page.next_after.map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct DeleteOutput {
    deleted: bool,
}

#[tool_router(server_handler)]
impl McpHandler {
    #[tool(
        name = "workspace_get",
        description = "Get workspace details. Requires membership in the workspace."
    )]
    async fn workspace_get(
        &self,
        Parameters(args): Parameters<WorkspaceGetArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<WorkspaceOutput>, McpError> {
        let (actor, _) = caller(&context)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .workspace(actor, workspace_id)
            .await
            .map(WorkspaceOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "projects_list",
        description = "List projects in a workspace using a bounded cursor page. Requires workspace membership."
    )]
    async fn projects_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ProjectPageOutput>, McpError> {
        let (actor, _) = caller(&context)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let after = args
            .after
            .as_deref()
            .map(|value| parse_uuid(value, "after"))
            .transpose()?;
        let limit = page_limit(args.limit)?;
        self.store
            .for_caller(caller_context(&context)?)
            .list_projects(actor, workspace_id, after, limit)
            .await
            .map(ProjectPageOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "goals_list",
        description = "List goals in a workspace using a bounded cursor page. Requires workspace membership."
    )]
    async fn goals_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<GoalPageOutput>, McpError> {
        let (actor, _) = caller(&context)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let after = args
            .after
            .as_deref()
            .map(|value| parse_uuid(value, "after"))
            .transpose()?;
        let limit = page_limit(args.limit)?;
        self.store
            .for_caller(caller_context(&context)?)
            .list_goals(actor, workspace_id, after, limit)
            .await
            .map(GoalPageOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "recipients_list",
        description = "List report recipients in a workspace using a bounded cursor page. Requires workspace membership."
    )]
    async fn recipients_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<RecipientPageOutput>, McpError> {
        let (actor, _) = caller(&context)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let after = args
            .after
            .as_deref()
            .map(|value| parse_uuid(value, "after"))
            .transpose()?;
        let limit = page_limit(args.limit)?;
        self.store
            .for_caller(caller_context(&context)?)
            .list_recipients(actor, workspace_id, after, limit)
            .await
            .map(RecipientPageOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "projects_create",
        description = "Create a project. Requires write delegation and workspace write permission."
    )]
    async fn projects_create(
        &self,
        Parameters(args): Parameters<ProjectCreateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ProjectOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let input = NewProject {
            name: args.name,
            kind: args.kind.into(),
        };
        self.store
            .for_caller(caller_context(&context)?)
            .create_project(actor, workspace_id, input)
            .await
            .map(ProjectOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "projects_update",
        description = "Update a project with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn projects_update(
        &self,
        Parameters(args): Parameters<ProjectUpdateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ProjectOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        let input = NewProject {
            name: args.name,
            kind: args.kind.into(),
        };
        self.store
            .for_caller(caller_context(&context)?)
            .update_project(actor, workspace_id, id, args.expected_version, input)
            .await
            .map(ProjectOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "projects_delete",
        description = "Delete a project and its goals with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn projects_delete(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<DeleteOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .delete_project(actor, workspace_id, id, args.expected_version)
            .await
            .map(|()| Json(DeleteOutput { deleted: true }))
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "goals_create",
        description = "Create a goal. Requires write delegation and workspace write permission."
    )]
    async fn goals_create(
        &self,
        Parameters(args): Parameters<GoalCreateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<GoalOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let input = NewGoal {
            project_id: parse_uuid(&args.project_id, "project_id")?,
            name: args.name,
            event_name: args.event_name,
        };
        self.store
            .for_caller(caller_context(&context)?)
            .create_goal(actor, workspace_id, input)
            .await
            .map(GoalOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "goals_update",
        description = "Update a goal with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn goals_update(
        &self,
        Parameters(args): Parameters<GoalUpdateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<GoalOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        let input = NewGoal {
            project_id: parse_uuid(&args.project_id, "project_id")?,
            name: args.name,
            event_name: args.event_name,
        };
        self.store
            .for_caller(caller_context(&context)?)
            .update_goal(actor, workspace_id, id, args.expected_version, input)
            .await
            .map(GoalOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "goals_delete",
        description = "Delete a goal with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn goals_delete(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<DeleteOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .delete_goal(actor, workspace_id, id, args.expected_version)
            .await
            .map(|()| Json(DeleteOutput { deleted: true }))
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "recipients_create",
        description = "Add a report recipient. Requires write delegation and workspace write permission."
    )]
    async fn recipients_create(
        &self,
        Parameters(args): Parameters<RecipientCreateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<RecipientOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .create_recipient(actor, workspace_id, NewRecipient { email: args.email })
            .await
            .map(RecipientOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "recipients_update",
        description = "Update a report recipient with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn recipients_update(
        &self,
        Parameters(args): Parameters<RecipientUpdateArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<RecipientOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .update_recipient(
                actor,
                workspace_id,
                id,
                args.expected_version,
                NewRecipient { email: args.email },
            )
            .await
            .map(RecipientOutput::from)
            .map(Json)
            .map_err(map_workspace_error)
    }

    #[tool(
        name = "recipients_delete",
        description = "Remove a report recipient with its expected version. Requires write delegation and workspace write permission."
    )]
    async fn recipients_delete(
        &self,
        Parameters(args): Parameters<DeleteArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<DeleteOutput>, McpError> {
        let (actor, can_write) = caller(&context)?;
        require_write(can_write)?;
        let workspace_id = parse_uuid(&args.workspace_id, "workspace_id")?;
        let id = parse_uuid(&args.id, "id")?;
        self.store
            .for_caller(caller_context(&context)?)
            .delete_recipient(actor, workspace_id, id, args.expected_version)
            .await
            .map(|()| Json(DeleteOutput { deleted: true }))
            .map_err(map_workspace_error)
    }
}

fn caller(context: &RequestContext<RoleServer>) -> Result<(Uuid, bool), McpError> {
    let caller = caller_context(context)?;
    Ok((caller.principal_id, caller.can_write))
}

fn caller_context(context: &RequestContext<RoleServer>) -> Result<crate::auth::Caller, McpError> {
    let parts = context
        .extensions
        .get::<axum::http::request::Parts>()
        .ok_or_else(|| {
            McpError::internal_error("authenticated HTTP request context is missing", None)
        })?;
    let caller = parts
        .extensions
        .get::<crate::auth::Caller>()
        .ok_or_else(|| McpError::internal_error("authenticated caller context is missing", None))?;
    Ok(*caller)
}

fn require_write(can_write: bool) -> Result<(), McpError> {
    if can_write {
        Ok(())
    } else {
        Err(McpError::invalid_params(
            "write delegation is required",
            None,
        ))
    }
}

fn parse_uuid(value: &str, parameter: &str) -> Result<Uuid, McpError> {
    Uuid::parse_str(value)
        .map_err(|_| McpError::invalid_params(format!("{parameter} must be a valid UUID"), None))
}

fn page_limit(limit: Option<u16>) -> Result<u16, McpError> {
    let limit = limit.unwrap_or(50);
    if (1..=100).contains(&limit) {
        Ok(limit)
    } else {
        Err(McpError::invalid_params(
            "limit must be between 1 and 100",
            None,
        ))
    }
}

fn map_workspace_error(error: WorkspaceError) -> McpError {
    match error {
        WorkspaceError::Denied => McpError::invalid_params("workspace access denied", None),
        WorkspaceError::NotFound => {
            McpError::invalid_params("workspace configuration item not found", None)
        }
        WorkspaceError::Conflict => {
            McpError::invalid_params("configuration changed; reload before retrying", None)
        }
        WorkspaceError::InvalidInput(message) => McpError::invalid_params(message, None),
        WorkspaceError::Database(_) => McpError::internal_error("workspace operation failed", None),
    }
}
