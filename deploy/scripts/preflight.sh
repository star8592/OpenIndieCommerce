#!/usr/bin/env bash
set -euo pipefail
: "${OIC_DB:?OIC_DB is required}"
: "${OIC_PUBLIC_BASE_URL:?OIC_PUBLIC_BASE_URL is required}"
[[ "$OIC_PUBLIC_BASE_URL" == https://* ]] || { echo 'OIC_PUBLIC_BASE_URL must use https' >&2; exit 1; }
for k in OIC_SELLER_LEGAL_NAME OIC_SUPPORT_EMAIL OIC_SUPPORT_PHONE; do
  [[ -n "${!k:-}" ]] || { echo "$k is required for the public commerce/legal site" >&2; exit 1; }
done
[[ "$OIC_SUPPORT_EMAIL" == *@* ]] || { echo 'OIC_SUPPORT_EMAIL must look like an email address' >&2; exit 1; }
admin_file="${OIC_ADMIN_TOKEN_FILE:-}"
[[ -n "$admin_file" && -s "$admin_file" ]] || { echo 'OIC_ADMIN_TOKEN_FILE must point to a non-empty file' >&2; exit 1; }
if [[ -n "${OIC_ENTITLEMENT_KEY_SECRET_FILE:-}" ]]; then
  [[ -s "$OIC_ENTITLEMENT_KEY_SECRET_FILE" ]] || { echo 'OIC_ENTITLEMENT_KEY_SECRET_FILE is set but missing/empty' >&2; exit 1; }
  [[ $(wc -c <"$OIC_ENTITLEMENT_KEY_SECRET_FILE") -ge 32 ]] || { echo 'entitlement key secret must be at least 32 bytes' >&2; exit 1; }
fi
if [[ -n "${OIC_PADDLE_CLIENT_TOKEN:-}" || -n "${OIC_PADDLE_WEBHOOK_SECRET_FILE:-}" ]]; then
  [[ -n "${OIC_PADDLE_CLIENT_TOKEN:-}" ]] || { echo 'Paddle webhook configured but client token is missing' >&2; exit 1; }
  [[ -s "${OIC_PADDLE_WEBHOOK_SECRET_FILE:-/nonexistent}" ]] || { echo 'Paddle webhook secret file is missing' >&2; exit 1; }
fi
if [[ -n "${OIC_ZPAY_PID:-}" || -n "${OIC_ZPAY_KEY_FILE:-}" ]]; then
  [[ -n "${OIC_ZPAY_PID:-}" ]] || { echo 'ZPAY key configured but pid is missing' >&2; exit 1; }
  [[ -s "${OIC_ZPAY_KEY_FILE:-/nonexistent}" ]] || { echo 'ZPAY key file is missing' >&2; exit 1; }
fi
echo 'OIC_PRODUCTION_PREFLIGHT_OK'
