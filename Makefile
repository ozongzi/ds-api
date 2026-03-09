HOST = root@familiar.fhmmt.games
BIN  = target/x86_64-unknown-linux-musl/release/familiar

.PHONY: build deploy

build:
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
		cargo build --release -p familiar --target x86_64-unknown-linux-musl

deploy: build
	ssh $(HOST) "systemctl stop familiar"
	scp $(BIN) $(HOST):/usr/local/bin/familiar
	ssh $(HOST) "systemctl start familiar"
	@echo "✓ deployed"
