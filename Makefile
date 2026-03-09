HOST = root@familiar.fhmmt.games
BIN  = target/x86_64-unknown-linux-musl/release/familiar
CLIENT_DIR = familiar/client

.PHONY: build build-client dev deploy clean

# ── Rust backend ──────────────────────────────────────────────────────────────
build:
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
		cargo build --release -p familiar --target x86_64-unknown-linux-musl

# ── Frontend ──────────────────────────────────────────────────────────────────
build-client:
	cd $(CLIENT_DIR) && bun install --frozen-lockfile && bun run build

# Start frontend dev server (proxies /api and /ws to localhost:3000)
dev-client:
	cd $(CLIENT_DIR) && bun run dev

# Start backend in dev mode (reads .env automatically via dotenvy)
dev-server:
	cargo run -p familiar

# ── Full build (backend + frontend) ───────────────────────────────────────────
all: build-client build

# ── Deploy ────────────────────────────────────────────────────────────────────
deploy: all
	ssh $(HOST) "systemctl stop familiar"
	scp $(BIN) $(HOST):/usr/local/bin/familiar
	ssh $(HOST) "mkdir -p /srv/familiar/client/dist"
	rsync -av --delete $(CLIENT_DIR)/dist/ $(HOST):/srv/familiar/client/dist/
	ssh $(HOST) "systemctl start familiar"
	@echo "✓ deployed"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf $(CLIENT_DIR)/dist $(CLIENT_DIR)/node_modules
