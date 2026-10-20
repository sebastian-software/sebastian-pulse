-- Provider identities remain operator-managed. The runtime resolves an exact
-- issuer/subject pair through a narrow definer function; it cannot inspect or
-- change identity links or workspace grants directly. Email claims never link
-- identities automatically.
CREATE TABLE analytics.identity_links (
  issuer text NOT NULL,
  subject text NOT NULL,
  principal_id uuid NOT NULL REFERENCES analytics.principals(id),
  enabled boolean NOT NULL DEFAULT true,
  CONSTRAINT identity_links_issuer_length CHECK (char_length(issuer) BETWEEN 1 AND 2048),
  CONSTRAINT identity_links_subject_length CHECK (char_length(subject) BETWEEN 1 AND 255),
  PRIMARY KEY (issuer, subject),
  UNIQUE (issuer, subject, principal_id)
);

CREATE FUNCTION analytics.resolve_principal(issuer text, subject text)
RETURNS uuid
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, analytics
AS $$
  SELECT link.principal_id
  FROM analytics.identity_links AS link
  WHERE link.issuer = $1
    AND link.subject = $2
    AND link.enabled
$$;

REVOKE ALL ON FUNCTION analytics.resolve_principal(text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION analytics.resolve_principal(text, text) TO analytics_runtime;

-- Browser login state exists before a principal is known, so these opaque
-- pre-authentication rows are global and intentionally have no tenant RLS.
CREATE TABLE analytics.browser_login_attempts (
  state_hash bytea PRIMARY KEY,
  binding_hash bytea NOT NULL,
  pkce_verifier text NOT NULL,
  nonce text NOT NULL,
  expires_at timestamptz NOT NULL,
  CONSTRAINT browser_login_attempts_state_hash_length
    CHECK (octet_length(state_hash) = 32),
  CONSTRAINT browser_login_attempts_binding_hash_length
    CHECK (octet_length(binding_hash) = 32),
  CONSTRAINT browser_login_attempts_pkce_verifier_length
    CHECK (char_length(pkce_verifier) BETWEEN 43 AND 128),
  CONSTRAINT browser_login_attempts_nonce_length
    CHECK (char_length(nonce) BETWEEN 1 AND 256)
);
CREATE INDEX browser_login_attempts_expires_at
  ON analytics.browser_login_attempts (expires_at);

-- Browser session rows are global bearer-token lookups keyed only by a digest.
-- Deleting an identity link cascades to its sessions, so unlink/relink cannot
-- revive a token issued for the previous link.
CREATE TABLE analytics.browser_sessions (
  token_hash bytea PRIMARY KEY,
  principal_id uuid NOT NULL REFERENCES analytics.principals(id) ON DELETE CASCADE,
  issuer text NOT NULL,
  subject text NOT NULL,
  csrf_token text NOT NULL,
  expires_at timestamptz NOT NULL,
  CONSTRAINT browser_sessions_token_hash_length
    CHECK (octet_length(token_hash) = 32),
  CONSTRAINT browser_sessions_csrf_token_length
    CHECK (char_length(csrf_token) BETWEEN 43 AND 128),
  CONSTRAINT browser_sessions_identity_link_fk
    FOREIGN KEY (issuer, subject, principal_id)
    REFERENCES analytics.identity_links (issuer, subject, principal_id)
    ON DELETE CASCADE
);
CREATE INDEX browser_sessions_expires_at
  ON analytics.browser_sessions (expires_at);

-- Session lookup rechecks resolve_principal on every request. Runtime has no
-- direct SELECT privilege on identity_links; the exact mapped principal must
-- still be enabled for a stored session to resolve.
REVOKE ALL ON TABLE analytics.identity_links FROM PUBLIC, analytics_runtime;
REVOKE ALL ON TABLE analytics.browser_login_attempts FROM PUBLIC, analytics_runtime;
REVOKE ALL ON TABLE analytics.browser_sessions FROM PUBLIC, analytics_runtime;

-- Opaque pre-auth state is not tenant data, so runtime access is deliberately
-- limited to the operations needed to begin and consume a browser login.
GRANT SELECT, INSERT, DELETE ON analytics.browser_login_attempts TO analytics_runtime;
GRANT SELECT, INSERT, DELETE ON analytics.browser_sessions TO analytics_runtime;
