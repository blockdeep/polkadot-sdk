#!/bin/bash

export DYLD_LIBRARY_PATH=/opt/homebrew/opt/llvm/lib

echo "🔨 Building test-parachain binary..."
echo

cargo build --release -p cumulus-test-service --bin test-parachain
if [ $? -ne 0 ]; then
    echo "❌ Failed to build test-parachain binary"
    exit 1
fi

echo "✅ test-parachain binary built successfully"
echo
echo "📍 Binary location: target/release/test-parachain"
echo
echo "🚀 Ready to run zombienet!"
echo "   zombienet spawn pubsub-dev/zombienet-test-runtime.toml"
