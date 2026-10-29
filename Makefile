# mcpd
#
# `make check` is what CI runs. If it passes locally it passes there.

SHELL := /bin/bash
.DEFAULT_GOAL := help
CARGO ?= cargo
NODE ?= node
NPM ?= npm

.PHONY: help
help: ## Show this help
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
	  | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[1m%-16s\033[0m %s\n", $$1, $$2}'

# ---------------------------------------------------------------- generate ---

.PHONY: gen
gen: ## Regenerate every derived artifact from spec/mcpd.spec.json
	$(NODE) tools/codegen/index.mjs

.PHONY: gen-check
gen-check: ## Fail if any generated file is out of date with the spec
	$(NODE) tools/codegen/index.mjs --check

# THIRD_PARTY.md is generated too, from the lock files rather than the spec.
# It lists every crate and npm package a release is built from, by licence,
# so a dependency bump that changes a licence changes a committed file in the
# same pull request.
.PHONY: prices
prices: ## Refresh the built-in price book from the public model catalogue
	$(CARGO) run -q -p mcpd -- prices refresh --out crates/mcpd-core/prices/builtin.json

.PHONY: third-party
third-party: ## Regenerate THIRD_PARTY.md from Cargo.lock and web/package-lock.json
	$(NODE) tools/third-party.mjs

.PHONY: third-party-check
third-party-check: ## Fail if THIRD_PARTY.md is out of date with the lock files
	$(NODE) tools/third-party.mjs --check

# ------------------------------------------------------------------- build ---

.PHONY: build
build: ui ## Build the release binary with the interface compiled in
	$(CARGO) build --release

# The packages are installed when they are missing, so `make build` and
# `make dist` work on a fresh clone, which is what the install page tells you
# to run. An existing install is left alone rather than reinstalled each time.
#
# `.gitkeep` is put back after the copy. The directory is gitignored apart from
# that file, and `include_dir!` needs the directory to exist, so a checkout
# without it does not compile; replacing the directory wholesale deleted it,
# and a commit made after a build then removed it from the tree.
.PHONY: ui
ui: ## Build the web application into the server crate
	[ -d web/node_modules ] || (cd web && $(NPM) ci --no-audit --no-fund)
	cd web && $(NPM) run build
	rm -rf crates/mcpd-server/ui
	mkdir -p crates/mcpd-server/ui
	cp -r web/dist/. crates/mcpd-server/ui/
	touch crates/mcpd-server/ui/.gitkeep

.PHONY: dist
dist: ui ## Build the fully optimised single binary
	$(CARGO) build --profile dist

# -------------------------------------------------------------------- test ---

.PHONY: test
test: test-rust test-sdks ## Run every test suite

.PHONY: test-rust
test-rust: ## Rust tests across the workspace
	$(CARGO) test --workspace

.PHONY: test-sdks
test-sdks: ## Every SDK's own tests, skipping toolchains that are absent
	@set -e; \
	if command -v node >/dev/null; then \
	  echo "--- typescript"; (cd sdks/typescript && $(NPM) test --silent); \
	else echo "--- typescript: node not installed, skipped"; fi; \
	if command -v python3 >/dev/null; then \
	  echo "--- python"; (cd sdks/python && python3 -m pytest -q 2>/dev/null \
	    || python3 -m unittest discover -q); \
	else echo "--- python: not installed, skipped"; fi; \
	echo "--- rust"; $(CARGO) test -p mcpd-sdk; \
	if command -v go >/dev/null; then \
	  echo "--- go"; (cd sdks/go && go test ./...); \
	else echo "--- go: toolchain not installed, skipped"; fi; \
	if command -v gradle >/dev/null; then \
	  echo "--- kotlin"; (cd sdks/kotlin && gradle test -q); \
	else echo "--- kotlin: toolchain not installed, skipped"; fi; \
	if command -v mix >/dev/null; then \
	  echo "--- elixir"; (cd sdks/elixir && mix test); \
	else echo "--- elixir: toolchain not installed, skipped"; fi

# ------------------------------------------------------------------- check ---

.PHONY: check
check: gen-check third-party-check lint-rust test-rust consistency site ## Everything CI runs

.PHONY: lint-rust
lint-rust: ## Clippy with warnings denied, and a format check
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: consistency
consistency: ## Cross-cutting checks the compiler cannot make
	$(NODE) tools/check-consistency.mjs

.PHONY: conformance
conformance: ## Every SDK, against one live daemon, compared to each other
	$(CARGO) build -p mcpd
	$(NODE) tests/e2e/run.mjs
	$(NODE) tests/e2e/runs.mjs

# The other three end-to-end suites. Each starts its own daemon on a port the
# operating system assigns. `errors.mjs` runs every SDK against a daemon that
# is broken in eight ways, `mcp.mjs` drives `mcpd mcp` over stdio, and
# `mutations.mjs` corrupts batches on the wire to prove the conformance checks
# can go red. They are not in `make check` because the first needs every SDK
# toolchain to say anything, and a skipped language is reported, not passed.
.PHONY: e2e
e2e: ## The error paths, the MCP server and the mutation proof, against live daemons
	$(CARGO) build -p mcpd
	$(NODE) tests/e2e/errors.mjs
	$(NODE) tests/e2e/mcp.mjs
	$(NODE) tests/e2e/mutations.mjs

# The judge, against a real model. This is NOT part of `make check`, and the
# reason is money: every run it grades is a paid request to whichever model you
# point it at, so a suite that ran on every commit would bill you for every
# commit. `make check` has to be free to run and repeatable, and a real judge is
# neither. Run this when you change `judge.rs`, the prompt in `judge_request`,
# or the criteria contract.
#
# Credentials come from the environment or from `.env`, which is gitignored.
# With none set it reports a skip and exits zero, so it is safe to run
# anywhere; set MCPD_REQUIRE_LLM=1 to turn that skip into a failure, which is
# what a pipeline meant to run it should do so it cannot pass by doing nothing.
.PHONY: judge-e2e
judge-e2e: ## LLM-as-judge against a real model (costs money; not in `make check`)
	$(CARGO) build -p mcpd
	$(NODE) tests/e2e/judge.mjs

.PHONY: site
site: ## Typecheck and build the documentation site
	# `tsc` first. `LanguageId` already requires every language to appear in
	# both snippet maps, but nothing ran the typechecker, so adding a sixth SDK
	# surfaced as a crash halfway through the build instead of a type error.
	cd web && npx tsc --noEmit -p tsconfig.json
	$(MAKE) blog-check
	cd web && $(NPM) run build

# Builds with a fixture post that sets every front matter key, reads each key
# back out of the pages, the feed and the sitemap, and removes the fixture
# again, whether the build passed or not. `site` builds again after it, so
# web/dist never holds the fixture once `make check` is done.
.PHONY: blog-check
blog-check: ## Prove a blog post with every front matter key renders
	cd web && cp tests/fixtures/blog/every-key.md src/content/blog/ && \
	  { $(NPM) run build && node tests/blog.mjs; s=$$?; rm -f src/content/blog/every-key.md; exit $$s; }

.PHONY: fmt
fmt: ## Format everything
	$(CARGO) fmt --all
	cd web && $(NPM) run --silent format 2>/dev/null || true

# --------------------------------------------------------------------- dev ---

.PHONY: dev
dev: ## Daemon with demo data, plus the site, both watching
	@echo "daemon  http://localhost:4317"
	@echo "site    http://localhost:4321"
	@trap 'kill 0' EXIT; \
	$(CARGO) run -p mcpd -- demo --data ./mcpd-dev.db & \
	(cd web && $(NPM) run dev) & \
	wait

.PHONY: clean
clean: ## Remove build output
	$(CARGO) clean
	rm -rf web/dist web/.astro crates/mcpd-server/ui/*
