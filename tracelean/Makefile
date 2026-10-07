.PHONY: shell check test e2e vitest cover ci sandbox-test build clean

# A shell with every toolchain the suite needs: Rust (pinned by
# rust-toolchain.toml), Node 22, Lean 4.12.0 via elan, pyright, python3.
shell:
	docker compose run --rm dev bash

# Compile every crate and every target, without running anything.
check:
	docker compose run --rm check

# The whole suite: Rust tests, TypeScript type-check, vitest.
test:
	docker compose run --rm test

# The differential path end to end. Needs Lean, which the dev image has:
# builds the example's model runner and runs all six bindings against it.
e2e:
	docker compose run --rm e2e

# Frontend tests only.
vitest:
	docker compose run --rm dev bash -c "npm install && npm test"

# Line coverage, Rust and Python. Not the same question as the coverage the
# traceability panel reports: that one asks whether a requirement has evidence,
# this one asks whether the code ran.
cover:
	docker compose run --rm cover

# What CI runs, as one command. The image build succeeding IS the green light.
# Context is the repository root: the keymap test regenerates ../README.md and
# the end-to-end run works on ../example.
ci:
	docker build -f tracelean/Dockerfile.ci -t tracelean-ci ..

# The sandbox tests need an unprivileged user namespace, which `docker build`
# forbids and which therefore cannot be checked by `make ci`. This runs them
# with the container privileges bubblewrap actually needs.
sandbox-test:
	docker compose run --rm sandbox

# The release binary.
build:
	docker compose build app

clean:
	docker compose down -v
	rm -rf target node_modules dist
