-- WorkOS owns membership and permissions. These tables only bind verified
-- provider identifiers to stable product IDs; they contain no access grants.
CREATE TABLE analytics.workos_principals (
  client_id text NOT NULL CHECK (char_length(client_id) BETWEEN 1 AND 128),
  user_id text NOT NULL CHECK (char_length(user_id) BETWEEN 1 AND 128),
  principal_id uuid NOT NULL UNIQUE REFERENCES analytics.principals(id),
  PRIMARY KEY (client_id, user_id)
);

CREATE TABLE analytics.workos_workspaces (
  client_id text NOT NULL CHECK (char_length(client_id) BETWEEN 1 AND 128),
  organization_id text NOT NULL CHECK (char_length(organization_id) BETWEEN 1 AND 128),
  workspace_id uuid NOT NULL UNIQUE REFERENCES analytics.workspaces(id) ON DELETE CASCADE,
  PRIMARY KEY (client_id, organization_id)
);

REVOKE ALL ON analytics.workos_principals, analytics.workos_workspaces
  FROM PUBLIC, analytics_runtime;

-- Called only after WorkOS token and current membership/permission verification.
-- Creating an ID binding grants no membership or access by itself. Serialize
-- concurrent first requests so one provider user always receives the same UUID.
CREATE FUNCTION analytics.resolve_workos_principal(client text, subject text, proposed_id uuid)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, analytics
AS $$
DECLARE result uuid;
BEGIN
  IF char_length(client) NOT BETWEEN 1 AND 128 OR
     char_length(subject) NOT BETWEEN 1 AND 128 OR proposed_id IS NULL THEN
    RETURN NULL;
  END IF;
  PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(client || ':' || subject, 0));
  SELECT principal_id INTO result FROM analytics.workos_principals
    WHERE client_id = client AND user_id = subject;
  IF result IS NULL THEN
    INSERT INTO analytics.principals(id) VALUES(proposed_id);
    INSERT INTO analytics.workos_principals(client_id, user_id, principal_id)
      VALUES(client, subject, proposed_id);
    result := proposed_id;
  END IF;
  RETURN result;
END;
$$;

CREATE FUNCTION analytics.resolve_workos_workspace(client text, organization text)
RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, analytics
AS $$
  SELECT workspace_id FROM analytics.workos_workspaces
    WHERE client_id = $1 AND organization_id = $2
$$;

REVOKE ALL ON FUNCTION analytics.resolve_workos_principal(text, text, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION analytics.resolve_workos_workspace(text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION analytics.resolve_workos_principal(text, text, uuid) TO analytics_runtime;
GRANT EXECUTE ON FUNCTION analytics.resolve_workos_workspace(text, text) TO analytics_runtime;

-- RLS still trusts the authenticated backend's transaction-local assertion.
-- Managed requests never fall back to stale local membership grants. The store
-- rejects a different actor/workspace before installing managed authority.
CREATE OR REPLACE FUNCTION analytics.can_access_workspace(write_required boolean)
RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, analytics
AS $$
  SELECT CASE
    WHEN write_required IS NULL THEN false
    WHEN pg_catalog.current_setting('analytics.managed_authority', true) = 'workos' THEN
      NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '') IS NOT NULL
      AND NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '') IS NOT NULL
      AND CASE WHEN write_required THEN
        pg_catalog.current_setting('analytics.managed_permission', true) = 'write'
      ELSE
        pg_catalog.current_setting('analytics.managed_permission', true) IN ('read', 'write')
      END
    ELSE EXISTS (
      SELECT 1 FROM analytics.workspace_members AS wm
      WHERE wm.workspace_id = NULLIF(pg_catalog.current_setting('analytics.workspace_id', true), '')::uuid
        AND wm.principal_id = NULLIF(pg_catalog.current_setting('analytics.actor_id', true), '')::uuid
        AND (wm.role = 'admin' OR (NOT write_required AND wm.role = 'reader'))
    )
  END
$$;
