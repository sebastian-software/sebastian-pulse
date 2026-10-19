-- Disposable local bootstrap, executed once by the PostgreSQL image as its administrator.
CREATE ROLE analytics_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
CREATE ROLE analytics_app LOGIN INHERIT PASSWORD 'local-runtime-only'
  NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS IN ROLE analytics_runtime;
CREATE ROLE analytics_migrator LOGIN PASSWORD 'local-migration-only'
  NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
REVOKE ALL ON DATABASE analytics FROM PUBLIC;
GRANT CONNECT ON DATABASE analytics TO analytics_app, analytics_migrator;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA analytics AUTHORIZATION analytics_migrator;
