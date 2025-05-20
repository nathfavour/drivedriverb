#!/bin/bash

set -e

# Default build type
BUILD_TYPE="release"

# Parse CLI flags
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --debug) BUILD_TYPE="debug"; shift ;;
        --release) BUILD_TYPE="release"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
done

# Build the Rust package
if [ "$BUILD_TYPE" == "release" ]; then
    cargo build --release
    TARGET_DIR="release"
else
    cargo build
    TARGET_DIR="debug"
fi

# Find the executable name from Cargo.toml
if [ -f "Cargo.toml" ]; then
    EXEC_NAME=$(grep '^name\s*=' Cargo.toml | head -n1 | cut -d '"' -f2)
else
    echo "Cargo.toml not found!"
    exit 1
fi

# Determine system home directory
SYSTEM_HOME=$(getent passwd "$USER" | cut -d: -f6)

# Create bin directory if it doesn't exist
mkdir -p "$SYSTEM_HOME/bin"

# Copy the executable
cp "target/$TARGET_DIR/$EXEC_NAME" "$SYSTEM_HOME/bin/"

echo "Installed $EXEC_NAME to $SYSTEM_HOME/bin/"