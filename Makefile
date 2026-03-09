HOST = root@familiar.fhmmt.games
BIN  = target/x86_64-unknown-linux-musl/release/familiar
CLIENT_DIR = familiar/client
REMOTE_SRC = /root

.PHONY: build build-client dev deploy clean

# ── Rust backend (local, cross-compiled musl) ─────────────────────────────────
build:
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
		cargo build --release -p familiar --target x86_64-unknown-linux-musl

# ── Rust backend (remote, native linux/gnu) ───────────────────────────────────
build-remote:
	ssh $(HOST) "cd $(REMOTE_SRC) && ~/.cargo/bin/cargo build -p familiar --release"

# ── Frontend ──────────────────────────────────────────────────────────────────
build-client:
	cd $(CLIENT_DIR) && bun install --frozen-lockfile && bun run build

# Start frontend dev server (proxies /api and /ws to localhost:3000)
dev-client:
	cd $(CLIENT_DIR) && bun run dev

# Start backend in dev mode (reads .env automatically via dotenvy)
dev-server:
	cargo run -p familiar

# ── Sync source to server (no git required on server) ────────────────────────
sync:
	rsync -av --delete \
		--exclude 'target/' \
		--exclude 'client/node_modules/' \
		--exclude 'client/dist/' \
		--exclude '.env' \
		familiar/ $(HOST):$(REMOTE_SRC)/familiar/
	rsync -av --exclude 'target/' ds-api/ $(HOST):$(REMOTE_SRC)/ds-api/
	rsync -av --exclude 'target/' ds-api-macros/ $(HOST):$(REMOTE_SRC)/ds-api-macros/
	rsync -av Cargo.toml Cargo.lock $(HOST):$(REMOTE_SRC)/

# ── Full build (backend + frontend) ───────────────────────────────────────────
all: build-client build

# ── Deploy (local cross-compile → scp binary) ────────────────────────────────
deploy: all
	ssh $(HOST) "systemctl stop familiar"
	scp $(BIN) $(HOST):/usr/local/bin/familiar
	ssh $(HOST) "mkdir -p /srv/familiar/client/dist"
	rsync -av --delete $(CLIENT_DIR)/dist/ $(HOST):/srv/familiar/client/dist/
	ssh $(HOST) "systemctl start familiar"
	@echo "✓ deployed"

# ── Deploy remote (sync source → build on server → restart) ──────────────────
deploy-remote: build-client sync build-remote
	ssh $(HOST) "systemctl stop familiar"
	ssh $(HOST) "cp $(REMOTE_SRC)/target/release/familiar /usr/local/bin/familiar"
	ssh $(HOST) "mkdir -p /srv/familiar/client/dist"
	rsync -av --delete $(CLIENT_DIR)/dist/ $(HOST):/srv/familiar/client/dist/
	ssh $(HOST) "systemctl start familiar"
	@echo "✓ deployed (remote build)"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf $(CLIENT_DIR)/dist $(CLIENT_DIR)/node_modules
