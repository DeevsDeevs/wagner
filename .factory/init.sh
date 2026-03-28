#!/bin/bash
set -e

# Ensure devbox is available and dependencies are fetched
if command -v devbox &> /dev/null; then
    echo "devbox found, ensuring environment..."
    devbox run cargo check --all-features 2>/dev/null || true
else
    echo "WARNING: devbox not found. Ensure Rust toolchain is available."
fi
