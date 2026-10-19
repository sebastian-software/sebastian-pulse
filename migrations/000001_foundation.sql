REVOKE ALL ON SCHEMA analytics FROM PUBLIC;
GRANT USAGE ON SCHEMA analytics TO analytics_runtime;

-- The runtime can inspect migration history, but cannot change it or own schema objects.
REVOKE ALL ON analytics.schema_migrations FROM analytics_runtime;
GRANT SELECT ON analytics.schema_migrations TO analytics_runtime;

-- Future application tables belong to the migrator and grant only data operations.
ALTER DEFAULT PRIVILEGES IN SCHEMA analytics
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO analytics_runtime;
ALTER DEFAULT PRIVILEGES IN SCHEMA analytics
  GRANT USAGE, SELECT ON SEQUENCES TO analytics_runtime;
