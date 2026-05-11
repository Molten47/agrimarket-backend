#!/usr/bin/env bash
# scripts/gen_keys.sh
# Generates RS256 private/public key pair for JWT signing.
# Paste the output values into your .env file.

set -euo pipefail

echo "🔑 Generating RS256 key pair for AgriMarket..."
echo ""

# Generate private key
openssl genrsa -out /tmp/agrimarket_private.pem 2048 2>/dev/null

# Derive public key
openssl rsa -in /tmp/agrimarket_private.pem -pubout -out /tmp/agrimarket_public.pem 2>/dev/null

# Base64 encode (single line, no wrapping)
PRIVATE_B64=$(base64 -w 0 /tmp/agrimarket_private.pem 2>/dev/null || base64 /tmp/agrimarket_private.pem | tr -d '\n')
PUBLIC_B64=$(base64 -w 0 /tmp/agrimarket_public.pem 2>/dev/null || base64 /tmp/agrimarket_public.pem | tr -d '\n')

# Clean up temp files
rm /tmp/agrimarket_private.pem /tmp/agrimarket_public.pem

echo "Add these to your .env file:"
echo ""
echo "JWT_PRIVATE_KEY_B64=${PRIVATE_B64}"
echo ""
echo "JWT_PUBLIC_KEY_B64=${PUBLIC_B64}"
echo ""
echo "✅ Done. Keys are NOT saved to disk — copy them now."
