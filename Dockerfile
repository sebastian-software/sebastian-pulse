# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
FROM rust:1.97.1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97 AS builder
WORKDIR /app
ENV SQLX_OFFLINE=true
COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY migrations ./migrations
COPY src ./src
# Cargo validates declared test target paths even for a binary-only build.
COPY tests ./tests
RUN cargo build --locked --release --bin sebastian-analytics

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
ARG RELEASE_ID=local
ENV PORT=3000 RELEASE_ID=${RELEASE_ID}
LABEL org.opencontainers.image.source="https://github.com/sebastian-software/sebastian-analytics"
LABEL org.opencontainers.image.revision="${RELEASE_ID}"
RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/sebastian-analytics /usr/local/bin/sebastian-analytics
COPY --chown=1000:1000 migrations ./migrations
USER 1000:1000
EXPOSE 3000
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=3 \
  CMD ["sebastian-analytics", "healthcheck"]
ENTRYPOINT ["sebastian-analytics"]
CMD ["serve"]
