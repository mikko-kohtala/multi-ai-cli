.PHONY: build install clean test check fmt clippy help

help:
	@echo "Available targets:"
	@echo "  make build   - Build the project in release mode"
	@echo "  make install - Install the binary using cargo install --path ."
	@echo "  make clean   - Clean build artifacts"
	@echo "  make test    - Run tests"
	@echo "  make check   - Run all checks (fmt, clippy, test)"
	@echo "  make fmt     - Format code"
	@echo "  make clippy  - Run clippy linter (strict mode)"

build:
	cargo build --release

install: build
	cargo install --path .
	@mkdir -p ~/.config/multi-ai-cli
	@if [ -L ~/.config/multi-ai-cli/apps.jsonc ] || [ ! -e ~/.config/multi-ai-cli/apps.jsonc ]; then \
		ln -sf $(CURDIR)/apps.jsonc ~/.config/multi-ai-cli/apps.jsonc; \
		echo "→ Symlinked apps.jsonc to ~/.config/multi-ai-cli/apps.jsonc"; \
	else \
		echo "→ ~/.config/multi-ai-cli/apps.jsonc exists (not a symlink), skipping"; \
	fi

clean:
	cargo clean

test:
	cargo test

check:
	@echo "→ Checking formatting..."
	@cargo fmt -- --check
	@echo "→ Running clippy..."
	@cargo clippy --all-targets -- -D warnings
	@echo "→ Running tests..."
	@cargo test
	@echo "✓ All checks passed"

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets -- -D warnings
