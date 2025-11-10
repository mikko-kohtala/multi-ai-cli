.PHONY: build install clean test check fmt clippy help

help:
	@echo "Available targets:"
	@echo "  make build   - Build the project in release mode"
	@echo "  make install - Install the binary using cargo install --path ."
	@echo "  make clean   - Clean build artifacts"
	@echo "  make test    - Run tests"
	@echo "  make check   - Run cargo check"
	@echo "  make fmt     - Format code"
	@echo "  make clippy  - Run clippy linter"

build:
	cargo build --release

install: build
	cargo install --path .

clean:
	cargo clean

test:
	cargo test

check:
	cargo check

fmt:
	cargo fmt

clippy:
	cargo clippy
