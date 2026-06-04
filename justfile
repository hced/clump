# ============================================================================
# Detect binary name from Cargo.toml
# ============================================================================
name := `grep -m1 '^name = ' Cargo.toml | cut -d'"' -f2`

# Show this help message
default:
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
    @cargo pkgid | cut -d'@' -f2

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
    @which {{ name }} 2>/dev/null | xargs -I {} sh -c 'if [ -L "{}" ]; then echo "  Symlink: {}"; echo "  Physical binary: $(readlink {})"; else echo "  File: {}"; fi' || echo "  Not installed (run 'just install')"
    @echo ""
    @echo "Project root: $PWD"
