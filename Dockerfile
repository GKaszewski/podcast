FROM rust:1.79-slim-bookworm AS builder
WORKDIR /app
COPY . .
COPY ./.sqlx ./sqlx
ENV SQLX_OFFLINE=true
RUN \
 --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    cargo build --release && \
    cp ./target/release/podcast /

FROM alpine:3.20.3 AS final
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home /nonexistent \
    --shell /sbin/nologin \
    --no-create-home \
    --uid "10001" \
    appuser

COPY --from=builder /podcast /usr/local/bin/
RUN chown appuser /usr/local/bin/podcast
COPY --from=builder /app/static /static
USER appuser
ENV RUST_LOG="podcast=info"
WORKDIR /opt/podcast
# ENTRYPOINT [ "podcast" ]
CMD [ "ls -la" ]
EXPOSE 3000/tcp

