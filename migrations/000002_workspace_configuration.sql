-- Bootstrap principals, workspaces, and initial memberships through a trusted
-- operator/migrator connection. The application runtime cannot create or alter
-- these global identity and workspace grants.
--
-- Trust boundary: row-level security trusts the backend to assert the actor and
-- workspace in transaction-local settings. It does not protect against leaked
-- runtime database credentials, whose holder can set those values directly.

CREATE TABLE analytics.principals (
  id uuid PRIMARY KEY
);

CREATE TABLE analytics.workspaces (
  id uuid PRIMARY KEY,
  name text NOT NULL,
  version integer NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT workspaces_name_length CHECK (char_length(name) BETWEEN 1 AND 120),
  CONSTRAINT workspaces_version_positive CHECK (version > 0)
);

CREATE TABLE analytics.workspace_members (
  workspace_id uuid NOT NULL REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  principal_id uuid NOT NULL REFERENCES analytics.principals(id),
  role text NOT NULL,
  version integer NOT NULL DEFAULT 1,
  CONSTRAINT workspace_members_role CHECK (role IN ('admin', 'reader')),
  CONSTRAINT workspace_members_version_positive CHECK (version > 0),
  PRIMARY KEY (workspace_id, principal_id)
);

CREATE TABLE analytics.projects (
  workspace_id uuid NOT NULL REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  id uuid NOT NULL,
  name text NOT NULL,
  kind text NOT NULL,
  version integer NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT projects_name_length CHECK (char_length(name) BETWEEN 1 AND 120),
  CONSTRAINT projects_kind CHECK (kind IN ('website', 'product', 'cli')),
  CONSTRAINT projects_version_positive CHECK (version > 0),
  PRIMARY KEY (workspace_id, id)
);

CREATE TABLE analytics.goals (
  workspace_id uuid NOT NULL REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  id uuid NOT NULL,
  project_id uuid NOT NULL,
  name text NOT NULL,
  event_name text NOT NULL,
  version integer NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT goals_name_length CHECK (char_length(name) BETWEEN 1 AND 120),
  CONSTRAINT goals_event_name_length CHECK (char_length(event_name) BETWEEN 1 AND 128),
  CONSTRAINT goals_event_name_safe CHECK ((event_name COLLATE "C") ~ '^[A-Za-z0-9_.:-]+$'),
  CONSTRAINT goals_version_positive CHECK (version > 0),
  PRIMARY KEY (workspace_id, id),
  CONSTRAINT goals_project_workspace_fk
    FOREIGN KEY (workspace_id, project_id)
    REFERENCES analytics.projects(workspace_id, id) ON DELETE CASCADE
);

CREATE TABLE analytics.recipients (
  workspace_id uuid NOT NULL REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  id uuid NOT NULL,
  email text NOT NULL,
  version integer NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT recipients_email_length CHECK (char_length(email) BETWEEN 1 AND 254),
  CONSTRAINT recipients_version_positive CHECK (version > 0),
  PRIMARY KEY (workspace_id, id)
);

CREATE TABLE analytics.config_audit (
  workspace_id uuid NOT NULL REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  id uuid NOT NULL,
  actor_id uuid NOT NULL REFERENCES analytics.principals(id),
  action text NOT NULL,
  entity_id uuid NOT NULL,
  entity_version integer NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT config_audit_action CHECK (
    action IN (
      'create_project', 'update_project', 'delete_project',
      'create_goal', 'update_goal', 'delete_goal',
      'create_recipient', 'update_recipient', 'delete_recipient'
    )
  ),
  CONSTRAINT config_audit_entity_version_positive CHECK (entity_version > 0),
  PRIMARY KEY (workspace_id, id)
);

-- Membership is read by the definer-owned helper below. Keep owner bypass
-- available on this table; do not FORCE row-level security here.
ALTER TABLE analytics.principals ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.workspace_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.goals ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.recipients ENABLE ROW LEVEL SECURITY;
ALTER TABLE analytics.config_audit ENABLE ROW LEVEL SECURITY;

CREATE FUNCTION analytics.can_access_workspace(write_required boolean)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, analytics
AS $$
  SELECT CASE
    WHEN write_required IS TRUE THEN EXISTS (
      SELECT 1
      FROM analytics.workspace_members AS wm
      WHERE wm.workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
        AND wm.principal_id = NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '')::uuid
        AND wm.role = 'admin'
    )
    WHEN write_required IS FALSE THEN EXISTS (
      SELECT 1
      FROM analytics.workspace_members AS wm
      WHERE wm.workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
        AND wm.principal_id = NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '')::uuid
        AND wm.role IN ('admin', 'reader')
    )
    ELSE false
  END
$$;

REVOKE ALL ON FUNCTION analytics.can_access_workspace(boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION analytics.can_access_workspace(boolean) TO analytics_runtime;

CREATE POLICY workspaces_select ON analytics.workspaces
  FOR SELECT TO analytics_runtime
  USING (
    id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(false)
  );

CREATE POLICY workspace_members_select_self ON analytics.workspace_members
  FOR SELECT TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND principal_id = NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '')::uuid
  );

CREATE POLICY projects_select ON analytics.projects
  FOR SELECT TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(false)
  );
CREATE POLICY projects_insert ON analytics.projects
  FOR INSERT TO analytics_runtime
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY projects_update ON analytics.projects
  FOR UPDATE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  )
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY projects_delete ON analytics.projects
  FOR DELETE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );

CREATE POLICY goals_select ON analytics.goals
  FOR SELECT TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(false)
  );
CREATE POLICY goals_insert ON analytics.goals
  FOR INSERT TO analytics_runtime
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY goals_update ON analytics.goals
  FOR UPDATE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  )
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY goals_delete ON analytics.goals
  FOR DELETE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );

CREATE POLICY recipients_select ON analytics.recipients
  FOR SELECT TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(false)
  );
CREATE POLICY recipients_insert ON analytics.recipients
  FOR INSERT TO analytics_runtime
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY recipients_update ON analytics.recipients
  FOR UPDATE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  )
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );
CREATE POLICY recipients_delete ON analytics.recipients
  FOR DELETE TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );

CREATE POLICY config_audit_select ON analytics.config_audit
  FOR SELECT TO analytics_runtime
  USING (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND analytics.can_access_workspace(false)
  );
CREATE POLICY config_audit_insert ON analytics.config_audit
  FOR INSERT TO analytics_runtime
  WITH CHECK (
    workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
    AND actor_id = NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '')::uuid
    AND analytics.can_access_workspace(true)
  );

-- Replace the foundation's broad future-table grants with explicit runtime
-- access. The schema_migrations grant is retained for readiness/history reads.
REVOKE ALL ON SCHEMA analytics FROM analytics_runtime;
GRANT USAGE ON SCHEMA analytics TO analytics_runtime;
REVOKE ALL ON ALL TABLES IN SCHEMA analytics FROM analytics_runtime;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA analytics FROM analytics_runtime;
ALTER DEFAULT PRIVILEGES IN SCHEMA analytics
  REVOKE ALL ON TABLES FROM analytics_runtime;
ALTER DEFAULT PRIVILEGES IN SCHEMA analytics
  REVOKE ALL ON SEQUENCES FROM analytics_runtime;

GRANT SELECT ON analytics.schema_migrations TO analytics_runtime;
GRANT SELECT ON analytics.workspaces, analytics.workspace_members TO analytics_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE
  ON analytics.projects, analytics.goals, analytics.recipients TO analytics_runtime;
GRANT SELECT, INSERT ON analytics.config_audit TO analytics_runtime;
