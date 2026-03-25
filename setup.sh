#!/bin/bash
# Downloads Phi-3-mini-4bit GGUF to models/ for LLM mode.
# Usage: ./setup.sh
# No-download alternative: cargo run -- --mock

set -e
mkdir -p models

# FILL IN after your LLM backend spike (see PROJECT_PLAN.md LLM Backend Decision Protocol):
MODEL_URL="https://huggingface.co/YOUR_REPO/resolve/main/YOUR_MODEL.gguf"
MODEL_FILE="models/YOUR_MODEL.gguf"

# Fallback for low-memory machines (< 8 GB RAM):
# TinyLlama-1.1B-Q4 (~700 MB) — lower accuracy, same safety demonstration
# TINY_URL="https://huggingface.co/..."

if [ -f "$MODEL_FILE" ]; then
    echo "✅ Model already downloaded: $MODEL_FILE"
    exit 0
fi

echo "Downloading model (~2 GB)..."
curl -L --progress-bar -o "$MODEL_FILE" "$MODEL_URL"
echo ""
echo "✅ Download complete."
echo "Run: cargo run --release"
