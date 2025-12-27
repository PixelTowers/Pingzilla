# ABOUTME: Makefile for PingZilla - simplifies build, release, and App Store tasks
# ABOUTME: Run `make help` to see all available targets

# Configuration
APP_NAME := PingZilla
BUNDLE_ID := pingzilla.pixeltowers.io
TEAM_ID := Y5223T2D8X
VERSION := $(shell grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/.*: "\(.*\)".*/\1/')

# Signing Identities
DIST_CERT := Apple Distribution: PixelTowers OU ($(TEAM_ID))
INSTALLER_CERT := 3rd Party Mac Developer Installer: PixelTowers OU ($(TEAM_ID))

# Paths
BUILD_DIR := src-tauri/target
RELEASE_DIR := $(BUILD_DIR)/release
UNIVERSAL_DIR := $(BUILD_DIR)/universal-apple-darwin/release
APP_BUNDLE := $(RELEASE_DIR)/bundle/macos/$(APP_NAME).app
UNIVERSAL_APP := $(UNIVERSAL_DIR)/bundle/macos/$(APP_NAME).app
PKG_FILE := $(UNIVERSAL_DIR)/$(APP_NAME)-$(VERSION).pkg
ENTITLEMENTS := src-tauri/Entitlements.plist
PROVISION_PROFILE := src-tauri/embedded.provisionprofile

# Colors for output
GREEN := \033[0;32m
YELLOW := \033[0;33m
NC := \033[0m # No Color

.PHONY: help dev build release universal clean run kill pkg sign upload lint check appstore clean-profile icons

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
icons: ## Regenerate icons with rounded corners and required sizes
	@echo "$(GREEN)Generating App Store icons...$(NC)"
	@cd src-tauri/icons && \
	if [ -f icon_square_backup.png ]; then \
		SIZE=$$(magick identify -format "%w" icon_square_backup.png) && \
		RADIUS=$$((SIZE * 18 / 100)) && \
		magick icon_square_backup.png \
			\( -size $${SIZE}x$${SIZE} xc:black -fill white -draw "roundrectangle 0,0 $$((SIZE-1)),$$((SIZE-1)) $$RADIUS,$$RADIUS" \) \
			-alpha set -compose DstIn -composite \
			-define png:color-type=6 icon.png && \
		magick icon.png -resize 1024x1024 -define png:color-type=6 icon_1024.png && \
		magick icon_1024.png -resize 32x32 -define png:color-type=6 32x32.png && \
		magick icon_1024.png -resize 128x128 -define png:color-type=6 128x128.png && \
		magick icon_1024.png -resize 256x256 -define png:color-type=6 128x128@2x.png && \
		mkdir -p icon.iconset && \
		magick icon_1024.png -resize 16x16 -define png:color-type=6 icon.iconset/icon_16x16.png && \
		magick icon_1024.png -resize 32x32 -define png:color-type=6 icon.iconset/icon_16x16@2x.png && \
		magick icon_1024.png -resize 32x32 -define png:color-type=6 icon.iconset/icon_32x32.png && \
		magick icon_1024.png -resize 64x64 -define png:color-type=6 icon.iconset/icon_32x32@2x.png && \
		magick icon_1024.png -resize 128x128 -define png:color-type=6 icon.iconset/icon_128x128.png && \
		magick icon_1024.png -resize 256x256 -define png:color-type=6 icon.iconset/icon_128x128@2x.png && \
		magick icon_1024.png -resize 256x256 -define png:color-type=6 icon.iconset/icon_256x256.png && \
		magick icon_1024.png -resize 512x512 -define png:color-type=6 icon.iconset/icon_256x256@2x.png && \
		magick icon_1024.png -resize 512x512 -define png:color-type=6 icon.iconset/icon_512x512.png && \
		cp icon_1024.png icon.iconset/icon_512x512@2x.png && \
		iconutil -c icns icon.iconset -o icon.icns && \
		rm -rf icon.iconset && \
		echo "$(GREEN)Icons generated with rounded corners and 1024x1024 size$(NC)"; \
	else \
		echo "$(YELLOW)ERROR: icon_square_backup.png not found!$(NC)"; \
		echo "Save your original square icon as src-tauri/icons/icon_square_backup.png"; \
		exit 1; \
	fi

clean-profile: ## Remove quarantine from provisioning profile
	@echo "$(GREEN)Cleaning provisioning profile...$(NC)"
	@if [ -f "$(PROVISION_PROFILE)" ]; then \
		xxd "$(PROVISION_PROFILE)" > /tmp/profile.hex && \
		xxd -r /tmp/profile.hex > /tmp/clean_profile.provisionprofile && \
		cp /tmp/clean_profile.provisionprofile "$(PROVISION_PROFILE)" && \
		echo "$(GREEN)Profile cleaned!$(NC)"; \
	else \
		echo "$(YELLOW)No provisioning profile found at $(PROVISION_PROFILE)$(NC)"; \
		echo "Download from Apple Developer Portal and place in src-tauri/embedded.provisionprofile"; \
	fi

sign: universal ## Sign the app for App Store distribution
	@echo "$(GREEN)Embedding provisioning profile...$(NC)"
	@if [ ! -f "$(PROVISION_PROFILE)" ]; then \
		echo "$(YELLOW)ERROR: Provisioning profile not found!$(NC)"; \
		echo "Download from Apple Developer Portal and save as:"; \
		echo "  $(PROVISION_PROFILE)"; \
		exit 1; \
	fi
	@xxd "$(PROVISION_PROFILE)" > /tmp/profile.hex
	@xxd -r /tmp/profile.hex > "$(UNIVERSAL_APP)/Contents/embedded.provisionprofile"
	@echo "$(GREEN)Signing app with: $(DIST_CERT)$(NC)"
	codesign --deep --force --verify --verbose \
		--sign "$(DIST_CERT)" \
		--entitlements "$(ENTITLEMENTS)" \
		--options runtime \
		"$(UNIVERSAL_APP)"
	@echo "$(GREEN)App signed successfully!$(NC)"

pkg: sign ## Create signed installer package for App Store
	@echo "$(GREEN)Creating installer package...$(NC)"
	productbuild --component "$(UNIVERSAL_APP)" /Applications \
		--sign "$(INSTALLER_CERT)" \
		"$(PKG_FILE)"
	@echo "$(GREEN)Package created: $(PKG_FILE)$(NC)"

appstore: pkg ## Full App Store build pipeline (build, sign, package)
	@echo ""
	@echo "$(GREEN)========================================$(NC)"
	@echo "$(GREEN)App Store package ready!$(NC)"
	@echo "$(GREEN)========================================$(NC)"
	@echo ""
	@echo "Package: $(PKG_FILE)"
	@echo ""
	@echo "To upload, run: make upload"

upload: ## Upload to App Store Connect
	@echo "$(YELLOW)Upload to App Store Connect:$(NC)"
	@echo ""
	@echo "Option 1 - Using Transporter (recommended):"
	@echo "  1. Open Transporter app"
	@echo "  2. Drag $(PKG_FILE)"
	@echo "  3. Click Deliver"
	@echo ""
	@echo "Option 2 - Using altool with App-Specific Password:"
	@echo "  xcrun altool --upload-app \\"
	@echo "    -f \"$(PKG_FILE)\" \\"
	@echo "    -t macos \\"
	@echo "    -u \"YOUR_APPLE_ID\" \\"
	@echo "    -p \"YOUR_APP_SPECIFIC_PASSWORD\""
	@echo ""
	@echo "Generate App-Specific Password at: https://appleid.apple.com"

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
