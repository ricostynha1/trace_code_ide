.PHONY: check build test shell cargo-check clean

# Build dev container and run cargo check (workspace)
check:
	docker compose run --rm dev bash -c "npm install && cargo check --workspace"

# Build the full release image
build:
	docker compose build build

# Interactive shell in dev container
shell:
	docker compose run --rm dev bash

# Run cargo check only (workspace)
cargo-check:
	docker compose run --rm dev bash -c "cargo check --workspace"

# Run all tests (workspace)
test:
	docker compose run --rm dev bash -c "cargo test --workspace"

# Run unit tests only
test-unit:
	docker compose run --rm dev bash -c "cargo test -p tracelean-tests"

# Clean build artifacts
clean:
	docker compose down -v
	rm -rf src-tauri/target node_modules dist
