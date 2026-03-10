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



# ── Full build (backend + frontend) ───────────────────────────────────────────
all: build-client build

# ── Deploy (local cross-compile → scp binary + client, then restart) ─────────
# scp/rsync first, restart last — never stop before copying so the running
# process is never killed mid-tool-call by its own deploy.
deploy: all
	scp $(BIN) $(HOST):/usr/local/bin/familiar.new
	ssh $(HOST) "mv /usr/local/bin/familiar.new /usr/local/bin/familiar"
	ssh $(HOST) "mkdir -p /srv/familiar/client/dist"
	rsync -av --delete $(CLIENT_DIR)/dist/ $(HOST):/srv/familiar/client/dist/
	scp familiar/config.toml $(HOST):/srv/familiar/config.toml
	ssh $(HOST) "systemctl restart familiar"
	@echo "✓ deployed"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf $(CLIENT_DIR)/dist $(CLIENT_DIR)/node_modules
