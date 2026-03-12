#!/usr/bin/env bash
# E2E smoke test: starts two HVOC nodes and verifies thread + DM flow.
#
# Usage:
#   cargo build
#   bash tests/e2e.sh
#
# Requirements: curl, jq

set -uo pipefail

HVOC="${HVOC_BIN:-./target/release/hvoc-cli}"
if [[ ! -x "$HVOC" ]]; then
  HVOC="./target/debug/hvoc-cli"
fi
if [[ ! -f "$HVOC" ]]; then
  echo "ERROR: hvoc-cli binary not found. Run 'cargo build' first."
  exit 1
fi

TMPDIR_BASE=$(mktemp -d)
NODE_A_DIR="$TMPDIR_BASE/node_a"
NODE_B_DIR="$TMPDIR_BASE/node_b"
PORT_A=17734
PORT_B=17735
PID_A=""
PID_B=""

cleanup() {
  echo ""
  echo "Cleaning up..."
  [[ -n "$PID_A" ]] && kill "$PID_A" 2>/dev/null || true
  [[ -n "$PID_B" ]] && kill "$PID_B" 2>/dev/null || true
  sleep 1
  rm -rf "$TMPDIR_BASE" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== HVOC E2E Test ==="
echo "Node A: port $PORT_A, data: $NODE_A_DIR"
echo "Node B: port $PORT_B, data: $NODE_B_DIR"
echo ""

# Start node A (suppress background output)
mkdir -p "$NODE_A_DIR"
"$HVOC" --data-dir "$NODE_A_DIR" serve --bind "127.0.0.1:$PORT_A" >/dev/null 2>&1 &
PID_A=$!

# Start node B
mkdir -p "$NODE_B_DIR"
"$HVOC" --data-dir "$NODE_B_DIR" serve --bind "127.0.0.1:$PORT_B" >/dev/null 2>&1 &
PID_B=$!

# Wait for nodes to be ready
echo "Waiting for nodes to start..."
for i in $(seq 1 30); do
  if curl -sf "http://127.0.0.1:$PORT_A/api/identity" >/dev/null 2>&1 && \
     curl -sf "http://127.0.0.1:$PORT_B/api/identity" >/dev/null 2>&1; then
    echo "Both nodes are up."
    break
  fi
  if [[ $i -eq 30 ]]; then
    echo "ERROR: nodes failed to start within 30s."
    exit 1
  fi
  sleep 1
done

API_A="http://127.0.0.1:$PORT_A"
API_B="http://127.0.0.1:$PORT_B"

PASS=0
FAIL=0

assert_eq() {
  local label="$1" actual="$2" expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    echo "  PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $label (expected '$expected', got '$actual')"
    FAIL=$((FAIL + 1))
  fi
}

assert_not_empty() {
  local label="$1" actual="$2"
  if [[ -n "$actual" && "$actual" != "null" ]]; then
    echo "  PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $label (empty or null)"
    FAIL=$((FAIL + 1))
  fi
}

# ──── Test 1: Create identity on Node A ─────────────────────────────────────
echo ""
echo "--- Test: Create identity on Node A ---"
RESULT_A=$(curl -sf -X POST "$API_A/api/identity" \
  -H 'Content-Type: application/json' \
  -d '{"handle": "alice", "passphrase": "test123"}') || RESULT_A="{}"
AUTHOR_A=$(echo "$RESULT_A" | jq -r '.author_id // empty')
assert_not_empty "identity created on A" "$AUTHOR_A"

# ──── Test 2: Create identity on Node B ─────────────────────────────────────
echo ""
echo "--- Test: Create identity on Node B ---"
RESULT_B=$(curl -sf -X POST "$API_B/api/identity" \
  -H 'Content-Type: application/json' \
  -d '{"handle": "bob", "passphrase": "test456"}') || RESULT_B="{}"
AUTHOR_B=$(echo "$RESULT_B" | jq -r '.author_id // empty')
assert_not_empty "identity created on B" "$AUTHOR_B"

# ──── Test 3: Unlock identity on Node A ─────────────────────────────────────
echo ""
echo "--- Test: Unlock identity on Node A ---"
UNLOCK=$(curl -sf -X POST "$API_A/api/identity/unlock" \
  -H 'Content-Type: application/json' \
  -d "{\"author_id\": \"$AUTHOR_A\", \"passphrase\": \"test123\"}") || UNLOCK="{}"
UNLOCKED=$(echo "$UNLOCK" | jq -r '.author_id // empty')
assert_eq "unlock returns correct author_id" "$UNLOCKED" "$AUTHOR_A"

# ──── Test 4: Create thread on Node A ───────────────────────────────────────
echo ""
echo "--- Test: Create thread on Node A ---"
THREAD=$(curl -sf -X POST "$API_A/api/threads" \
  -H 'Content-Type: application/json' \
  -d '{"title": "E2E Test Thread", "body": "Hello from E2E", "tags": ["test"]}') || THREAD="{}"
THREAD_ID=$(echo "$THREAD" | jq -r '.thread_id // empty')
assert_not_empty "thread created" "$THREAD_ID"

# ──── Test 5: Get thread from Node A ────────────────────────────────────────
echo ""
echo "--- Test: Get thread from Node A ---"
FETCHED=$(curl -sf "$API_A/api/threads/$THREAD_ID") || FETCHED="{}"
TITLE=$(echo "$FETCHED" | jq -r '.thread.title // empty')
assert_eq "thread title matches" "$TITLE" "E2E Test Thread"

# ──── Test 6: List posts for thread ─────────────────────────────────────────
echo ""
echo "--- Test: List posts for thread ---"
POSTS=$(curl -sf "$API_A/api/threads/$THREAD_ID/posts") || POSTS='{"posts":[]}'
POST_COUNT=$(echo "$POSTS" | jq '.posts | length')
assert_eq "thread has 1 opening post" "$POST_COUNT" "1"
POST_BODY=$(echo "$POSTS" | jq -r '.posts[0].body // empty')
assert_eq "opening post body" "$POST_BODY" "Hello from E2E"

# ──── Test 7: Create reply post ─────────────────────────────────────────────
echo ""
echo "--- Test: Create reply post ---"
REPLY=$(curl -sf -X POST "$API_A/api/threads/$THREAD_ID/posts" \
  -H 'Content-Type: application/json' \
  -d '{"body": "Reply from E2E"}') || REPLY="{}"
REPLY_ID=$(echo "$REPLY" | jq -r '.post_id // empty')
assert_not_empty "reply created" "$REPLY_ID"

POSTS2=$(curl -sf "$API_A/api/threads/$THREAD_ID/posts") || POSTS2='{"posts":[]}'
POST_COUNT2=$(echo "$POSTS2" | jq '.posts | length')
assert_eq "thread now has 2 posts" "$POST_COUNT2" "2"

# ──── Test 8: Search threads ────────────────────────────────────────────────
echo ""
echo "--- Test: Search threads ---"
SEARCH=$(curl -sf "$API_A/api/threads?q=E2E") || SEARCH='{"threads":[]}'
SEARCH_COUNT=$(echo "$SEARCH" | jq '.threads | length')
assert_eq "search finds 1 thread" "$SEARCH_COUNT" "1"

# ──── Test 9: Delete post (tombstone) ───────────────────────────────────────
echo ""
echo "--- Test: Delete post (tombstone) ---"
DEL=$(curl -sf -X DELETE "$API_A/api/posts/$REPLY_ID") || DEL="{}"
DEL_STATUS=$(echo "$DEL" | jq -r '.status // empty')
assert_eq "post deleted" "$DEL_STATUS" "deleted"

POSTS3=$(curl -sf "$API_A/api/threads/$THREAD_ID/posts") || POSTS3='{"posts":[]}'
POST_COUNT3=$(echo "$POSTS3" | jq '.posts | length')
assert_eq "thread back to 1 post after delete" "$POST_COUNT3" "1"

# ──── Test 10: Add contact on Node B ────────────────────────────────────────
echo ""
echo "--- Test: Add contact on Node B ---"
# First unlock B
curl -sf -X POST "$API_B/api/identity/unlock" \
  -H 'Content-Type: application/json' \
  -d "{\"author_id\": \"$AUTHOR_B\", \"passphrase\": \"test456\"}" >/dev/null 2>&1 || true

CONTACT=$(curl -sf -X POST "$API_B/api/contacts" \
  -H 'Content-Type: application/json' \
  -d "{\"author_id\": \"$AUTHOR_A\"}") || CONTACT="{}"
CONTACT_STATUS=$(echo "$CONTACT" | jq -r '.status // empty')
assert_not_empty "contact added on B" "$CONTACT_STATUS"

# ──── Test 11: List identities ──────────────────────────────────────────────
echo ""
echo "--- Test: List identities ---"
IDS=$(curl -sf "$API_A/api/identity/list") || IDS='{"identities":[]}'
ID_COUNT=$(echo "$IDS" | jq '.identities | length')
assert_eq "Node A has 1 identity" "$ID_COUNT" "1"

# ──── Summary ───────────────────────────────────────────────────────────────
echo ""
echo "================================"
echo "  PASSED: $PASS"
echo "  FAILED: $FAIL"
echo "================================"

if [[ $FAIL -gt 0 ]]; then
  exit 1
fi
echo "All E2E tests passed."
