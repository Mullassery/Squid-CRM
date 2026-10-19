SHELL := /bin/bash
TEST_DATABASE_URL ?= postgres://postgres:postgres@127.0.0.1:5432/squid_test

.PHONY: help build check fmt clippy test test-integration db-up db-down run smoke

help:
	@echo "build            cargo build --workspace"
	@echo "check            cargo check --workspace"
	@echo "fmt              cargo fmt --all"
	@echo "clippy           cargo clippy --workspace --all-targets -- -D warnings"
	@echo "test             unit tests"
	@echo "test-integration tests against real PostgreSQL (TEST_DATABASE_URL)"
	@echo "db-up / db-down  docker compose up/down postgres + redis"
	@echo "run              run the server locally (needs .env)"
	@echo "smoke            boot server and exercise auth + people via curl"

build:
	cargo build --workspace

check:
	cargo check --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

test-integration:
	TEST_DATABASE_URL="$(TEST_DATABASE_URL)" cargo test --workspace

db-up:
	docker compose up -d db redis

db-down:
	docker compose down

run:
	cargo run -p squid-server

smoke:
	./scripts/smoke.sh
