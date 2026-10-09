SHELL := /bin/bash
DESKTOP := desktop

.PHONY: help dev build test format check package

help:
	@printf '%s\n' 'ClashBar · Rust + Tauri' 'make dev      启动桌面开发环境' 'make build    构建前端与 Rust 客户端' 'make test     单元与浏览器测试' 'make check    类型、格式和 Clippy 检查' 'make package  构建 Windows NSIS 安装包'

dev:
	cd $(DESKTOP) && npm run tauri dev

build:
	cd $(DESKTOP) && npm run build
	cargo build --manifest-path $(DESKTOP)/src-tauri/Cargo.toml

test:
	cd $(DESKTOP) && npm test && npm run test:e2e
	cargo test --manifest-path $(DESKTOP)/src-tauri/Cargo.toml --no-default-features

format:
	cargo fmt --manifest-path $(DESKTOP)/src-tauri/Cargo.toml --all

check:
	cd $(DESKTOP) && npm run typecheck
	cargo fmt --manifest-path $(DESKTOP)/src-tauri/Cargo.toml --all -- --check
	cargo clippy --manifest-path $(DESKTOP)/src-tauri/Cargo.toml --all-targets -- -D warnings

package:
	cd $(DESKTOP) && npm run tauri build -- --bundles nsis -- --locked
