# ABOUTME: Makefile for PingZilla - simplifies build, release, and App Store tasks
# ABOUTME: Run `make help` to see all available targets

# Configuration
APP_NAME := PingZilla
BUNDLE_ID := com.pingzilla.monitor
VERSION := $(shell grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/.*: "\(.*\)".*/\1/')

# Paths
BUILD_DIR := src-tauri/target
RELEASE_DIR := $(BUILD_DIR)/release
UNIVERSAL_DIR := $(BUILD_DIR)/universal-apple-darwin/release
APP_BUNDLE := $(RELEASE_DIR)/bundle/macos/$(APP_NAME).app
UNIVERSAL_APP := $(UNIVERSAL_DIR)/bundle/macos/$(APP_NAME).app

# Colors for output
GREEN := \033[0;32m
YELLOW := \033[0;33m
NC := \033[0m # No Color

.PHONY: help dev build release universal clean run kill pkg sign upload lint check

help: ## Show this help message
	@echo "$(GREEN)PingZilla Build System$(NC)"
	@echo "Version: $(VERSION)"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(YELLOW)%-15s$(NC) %s\n", $$1, $$2}'

# Development
dev: ## Start development server with hot reload
	pnpm tauri dev

run: build ## Build and run the app
	@pkill -f $(APP_NAME) 2>/dev/null || true
	@sleep 1
	$(APP_BUNDLE)/Contents/MacOS/$(APP_NAME) &
	@echo "$(GREEN)$(APP_NAME) is running!$(NC)"

kill: ## Kill any running PingZilla processes
	@pkill -f $(APP_NAME) 2>/dev/null || true
	@echo "$(GREEN)Killed $(APP_NAME) processes$(NC)"

# Building
build: ## Build for current architecture (fast)
	pnpm tauri build

release: ## Build optimized release for current architecture
	pnpm tauri build --bundles app,dmg

universal: ## Build universal binary (Intel + Apple Silicon)
	@echo "$(GREEN)Building universal binary...$(NC)"
	pnpm tauri build --bundles app --target universal-apple-darwin
	@echo "$(GREEN)Universal build complete!$(NC)"
	@echo "App: $(UNIVERSAL_APP)"

# App Store
pkg: universal ## Create installer package for App Store
	@echo "$(GREEN)Creating installer package...$(NC)"
	@echo "$(YELLOW)Note: You need to sign with your certificate$(NC)"
	@echo ""
	@echo "Run manually:"
	@echo "  xcrun productbuild --sign \"3rd Party Mac Developer Installer: YOUR NAME (TEAM_ID)\" \\"
	@echo "    --component \"$(UNIVERSAL_APP)\" /Applications \"$(APP_NAME).pkg\""

sign: ## Sign the app for distribution (requires certificate)
	@echo "$(YELLOW)Manual signing required:$(NC)"
	@echo ""
	@echo "1. Sign the app:"
	@echo "   codesign --deep --force --verify --verbose \\"
	@echo "     --sign \"Apple Distribution: YOUR NAME (TEAM_ID)\" \\"
	@echo "     --options runtime \\"
	@echo "     --entitlements src-tauri/Entitlements.plist \\"
	@echo "     \"$(UNIVERSAL_APP)\""
	@echo ""
	@echo "2. Create installer package:"
	@echo "   xcrun productbuild --sign \"3rd Party Mac Developer Installer: YOUR NAME (TEAM_ID)\" \\"
	@echo "     --component \"$(UNIVERSAL_APP)\" /Applications \"$(APP_NAME).pkg\""

upload: ## Upload to App Store Connect (requires API key)
	@echo "$(YELLOW)Upload to App Store Connect:$(NC)"
	@echo ""
	@echo "Option 1 - Using altool:"
	@echo "  xcrun altool --upload-app --type macos --file \"$(APP_NAME).pkg\" \\"
	@echo "    --apiKey YOUR_API_KEY_ID --apiIssuer YOUR_API_ISSUER"
	@echo ""
	@echo "Option 2 - Using Transporter app (GUI):"
	@echo "  1. Download Transporter from App Store"
	@echo "  2. Open and sign in with Apple ID"
	@echo "  3. Drag $(APP_NAME).pkg to upload"

# Maintenance
clean: ## Clean build artifacts
	@echo "$(GREEN)Cleaning build artifacts...$(NC)"
	rm -rf $(BUILD_DIR)/release
	rm -rf $(BUILD_DIR)/debug
	rm -rf $(BUILD_DIR)/universal-apple-darwin
	rm -rf dist
	@echo "$(GREEN)Clean complete!$(NC)"

clean-all: clean ## Clean everything including cargo cache
	cd src-tauri && cargo clean
	rm -rf node_modules

# Code quality
lint: ## Run linters
	pnpm lint
	cd src-tauri && cargo clippy

check: ## Type check and validate
	pnpm tsc --noEmit
	cd src-tauri && cargo check

# Info
info: ## Show build info
	@echo "$(GREEN)PingZilla Build Info$(NC)"
	@echo "  Version:    $(VERSION)"
	@echo "  Bundle ID:  $(BUNDLE_ID)"
	@echo "  App:        $(APP_BUNDLE)"
	@echo ""
	@echo "$(GREEN)Rust:$(NC)"
	@rustc --version
	@cargo --version
	@echo ""
	@echo "$(GREEN)Node:$(NC)"
	@node --version
	@pnpm --version
