# ============================================================================
# Detect binary name from Cargo.toml
# ============================================================================
name := `grep -m1 '^name = ' Cargo.toml | cut -d'"' -f2`

# Show this help message
_default:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @just --list

# -----------------------------------------------------------------------------
# Build Commands (Release is default)
# -----------------------------------------------------------------------------

# Build optimized release binary (default)
build:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo build --release

# Build debug binary (optional, for development)
build-debug:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo build

# -----------------------------------------------------------------------------
# Run the program (Release is default)
# -----------------------------------------------------------------------------

# Run release build (default)
run *args:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo run --release -- {{ args }}

# Run debug build (optional, for development)
run-debug *args:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo run -- {{ args }}

# -----------------------------------------------------------------------------
# Testing (Release is default)
# -----------------------------------------------------------------------------

# Run tests in release mode (default)
tests:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo test --release

# Run tests in debug mode (optional)
tests-debug:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo test

# -----------------------------------------------------------------------------
# Code Quality
# -----------------------------------------------------------------------------

# Auto-format all source files
fmt:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo fmt

# Check if code is formatted (CI-friendly)
fmt-check:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo fmt --check

# Run clippy linter on release build
lint:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo clippy --release -- -D warnings

# Fast compile check without codegen
check:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo check

# -----------------------------------------------------------------------------
# Installation (Release is default)
# -----------------------------------------------------------------------------

# Install release binary to ~/.cargo/bin (default)
install:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo install --path . --force

# Install debug binary to ~/.cargo/bin (optional)
install-debug:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo install --path . --force --debug

# -----------------------------------------------------------------------------
# Cleanup
# -----------------------------------------------------------------------------

# Remove all build artifacts
clean:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo clean

# Remove everything including target directory
clean-all:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cargo clean
    @rm -rf target

# Wipe config and database files
wipe:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @rm -f ~/.config/{{ name }}/config.ron
    @rm -f ~/.config/{{ name }}/recipes.ron
    @echo "Config and recipes wiped."

# -----------------------------------------------------------------------------
# Configuration
# -----------------------------------------------------------------------------

# Open config file in $EDITOR
config-edit:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @${EDITOR:-nvim} ~/.config/{{ name }}/config.ron

# Display current config file contents
config-show:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @cat ~/.config/{{ name }}/config.ron 2>/dev/null || echo "No config found."

# -----------------------------------------------------------------------------
# Utility Commands
# -----------------------------------------------------------------------------

# Show current package version
version:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2

# Show binary info (size, location)
info:
    @if [ -z "{{ name }}" ]; then echo "Error: Could not extract 'name' from Cargo.toml" && exit 1; fi
    @echo "Release binary:"
    @ls -lh "$PWD/target/release/{{ name }}" 2>/dev/null || echo "  Release version not built (run 'just build')"
    @echo ""
    @echo "Debug binary:"
    @ls -lh "$PWD/target/debug/{{ name }}" 2>/dev/null || echo "  Debug version not built (run 'just build-debug')"
    @echo ""
    @echo "Installed executable:"
    @which {{ name }} 2>/dev/null | xargs -I {} sh -c 'if [ -L "{}" ]; then echo "  Symlink (in PATH): {}"; echo "  Physical binary: $(readlink {})"; else echo "  File: {}"; fi' || echo "  Not installed (run 'just install')"
    @echo ""
    @echo "Project root: $PWD"

# -----------------------------------------------------------------------------
# Git commands
# -----------------------------------------------------------------------------

set shell := ["bash", "-c"]

# Internal helper to ensure cargo-bump is installed
_ensure-cargo-bump:
    @if ! command -v cargo-bump &> /dev/null; then \
        echo "📥 cargo-bump not found. Installing it now..."; \
        cargo install cargo-bump; \
    fi

# Bump patch version (1.0.0 -> 1.0.1)
bump-patch: _ensure-cargo-bump
    @cargo bump patch
    @echo "✅ Bumped patch version"

# Bump minor version (1.0.0 -> 1.1.0)
bump-minor: _ensure-cargo-bump
    @cargo bump minor
    @echo "✅ Bumped minor version"

# Bump major version (1.0.0 -> 2.0.0)
bump-major: _ensure-cargo-bump
    @cargo bump major
    @echo "✅ Bumped major version"

# Add all changes and open editor for commit message
git-commit:
    @git add .
    @echo "Opening editor for commit message..."
    @git commit && echo "✅ Commit successful" || (echo "❌ Commit aborted (no message or user cancelled)"; exit 1)

# Create and push a release tag
push-release-tag:
    @echo "Existing tags:"
    @git tag --sort=-v:refname | head -5 || echo "  (none)"
    @echo ""
    @echo "Current version in Cargo.toml: v\$(grep -m1 '^version = ' Cargo.toml | cut -d'\"' -f2)"
    @echo ""
    @read -p "Tag name (e.g., v1.0.0): " tag; \
    if [ -z "$tag" ]; then echo "Cancelled."; exit 0; fi
    @echo ""
    @read -p "Create and push tag $tag? (y/N): " confirm; \
    if [ "$confirm" = "y" ] || [ "$confirm" = "Y" ]; then \
        git tag "$tag" && git push origin "$tag" && echo "✅ Tag $tag pushed!"; \
    else \
        echo "Cancelled."; \
    fi

# Master release recipe: bump version, commit, tag, and push
release: _ensure-cargo-bump
    @echo "🚀 Starting release process..."; \
    echo ""; \
    CURRENT_VER=$(grep -m1 "^version = " Cargo.toml | cut -d"\"" -f2); \
    echo "Current version: $CURRENT_VER"; \
    echo ""; \
    echo "Select bump type:"; \
    echo "  1) Patch (1.0.0 -> 1.0.1)"; \
    echo "  2) Minor (1.0.0 -> 1.1.0)"; \
    echo "  3) Major (1.0.0 -> 2.0.0)"; \
    echo "  4) Custom (enter manually)"; \
    echo "  5) No bump (use current version)"; \
    echo "  q) Cancel"; \
    echo ""; \
    read -p "Choice: " choice; \
    case $choice in \
        1) cargo bump patch ;; \
        2) cargo bump minor ;; \
        3) cargo bump major ;; \
        4) read -p "Enter new version: " version; \
           sed -i "s/^version = \".*\"/version = \"$version\"/" Cargo.toml; \
           echo "✅ Version updated to $version" ;; \
        5) echo "✅ Keeping current version ($CURRENT_VER)" ;; \
        q) echo "Cancelled."; exit 0 ;; \
        *) echo "Invalid choice. Cancelled."; exit 1 ;; \
    esac; \
    NEW_VERSION=$(grep -m1 "^version = " Cargo.toml | cut -d"\"" -f2); \
    DEFAULT_TAG="v$NEW_VERSION"; \
    echo ""; \
    echo "Recent tags:"; \
    git tag --sort=-v:refname | head -5 || echo "  (none)"; \
    echo ""; \
    read -p "Use default tag name '$DEFAULT_TAG'? (y/N): " tag_choice; \
    if [ "$tag_choice" = "y" ] || [ "$tag_choice" = "Y" ] || [ -z "$tag_choice" ]; then \
        TAG="$DEFAULT_TAG"; \
    else \
        read -p "Enter custom tag name: " custom_tag; \
        if [ -z "$custom_tag" ]; then echo "Cancelled."; exit 0; fi; \
        TAG="$custom_tag"; \
    fi; \
    echo ""; \
    echo "✅ Target release configuration set (tag: $TAG)"; \
    echo ""; \
    read -p "Add all changes and commit? (y/N): " commit_confirm; \
    if [ "$commit_confirm" != "y" ] && [ "$commit_confirm" != "Y" ]; then \
        echo "Cancelled."; \
        exit 0; \
    fi; \
    git add .; \
    echo "Opening editor for commit message..."; \
    if ! git commit --allow-empty; then \
        echo "❌ Commit cancelled. Release aborted."; \
        exit 1; \
    fi; \
    echo ""; \
    read -p "Push commits and create tag $TAG? (y/N): " tag_confirm; \
    if [ "$tag_confirm" = "y" ] || [ "$tag_confirm" = "Y" ]; then \
        git push origin main && git tag "$TAG" && git push origin "$TAG"; \
        echo ""; \
        echo "✅ Commits pushed and tag $TAG created!"; \
        echo "🚀 Release complete!"; \
    else \
        echo "Commit made locally, but not pushed."; \
    fi
