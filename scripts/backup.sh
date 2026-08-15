#!/usr/bin/env bash
# 每日 pg_dump 备份（docs/architecture.md §11）：压缩后上传 S3 兼容对象存储，
# 本地与远端均保留 BACKUP_RETENTION_DAYS（默认 30）天。
#
# 环境变量（compose 注入）：
#   数据库：PGHOST / PGPORT / PGUSER / PGPASSWORD / PGDATABASE
#   对象存储（可选，未配置时仅保留本地卷 /backups）：
#     S3_ENDPOINT（如 https://<account>.r2.cloudflarestorage.com）
#     S3_REGION（默认 us-east-1）、S3_BUCKET、S3_ACCESS_KEY、S3_SECRET_KEY
#   BACKUP_RETENTION_DAYS：保留天数，默认 30
set -euo pipefail

RETENTION="${BACKUP_RETENTION_DAYS:-30}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
FILE="/backups/xueban-${STAMP}.sql.gz"
KEY="xueban/${STAMP}.sql.gz"

# ---- 1. 导出并压缩 ----
mkdir -p /backups
pg_dump --no-owner --no-privileges | gzip > "${FILE}"

# ---- 2. 本地保留 N 天 ----
find /backups -name 'xueban-*.sql.gz' -mtime "+${RETENTION}" -delete

# ---- 3. S3 上传与远端清理（未配置 S3_* 则跳过） ----
if [[ -z "${S3_ENDPOINT:-}${S3_BUCKET:-}${S3_ACCESS_KEY:-}${S3_SECRET_KEY:-}" ]]; then
    echo "[backup] S3 未配置，仅保留本地备份 ${FILE}"
    exit 0
fi
: "${S3_REGION:=us-east-1}"

hex_sha256() { openssl dgst -sha256 -hex | sed 's/^.* //'; }

# AWS Signature V4 请求（S3 兼容对象存储）。
# 用法：s3_request <METHOD> <KEY> <QUERY> [PAYLOAD_FILE]
s3_request() {
    local method="$1" key="$2" query="$3" payload_file="${4:-}"
    local host amz_date date_stamp payload_hash canonical_uri
    host="${S3_ENDPOINT#*://}"
    amz_date="$(date -u +%Y%m%dT%H%M%SZ)"
    date_stamp="${amz_date%%T*}"
    if [[ -n "${payload_file}" ]]; then
        payload_hash="$(hex_sha256 < "${payload_file}")"
    else
        payload_hash="$(printf '' | hex_sha256)"
    fi
    canonical_uri="/${S3_BUCKET}/${key}"
    local canonical_headers="host:${host}\nx-amz-content-sha256:${payload_hash}\nx-amz-date:${amz_date}\n"
    local signed_headers="host;x-amz-content-sha256;x-amz-date"
    local canonical_request="${method}\n${canonical_uri}\n${query}\n${canonical_headers}\n${signed_headers}\n${payload_hash}"
    local scope="${date_stamp}/${S3_REGION}/s3/aws4_request"
    local string_to_sign="AWS4-HMAC-SHA256\n${amz_date}\n${scope}\n$(printf '%b' "${canonical_request}" | hex_sha256)"
    local k_date k_region k_service k_signing signature auth
    k_date="$(printf '%s' "${date_stamp}" | openssl dgst -sha256 -hmac "AWS4${S3_SECRET_KEY}" -hex | sed 's/^.* //')"
    k_region="$(printf '%s' "${S3_REGION}" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${k_date}" -hex | sed 's/^.* //')"
    k_service="$(printf '%s' 's3' | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${k_region}" -hex | sed 's/^.* //')"
    k_signing="$(printf '%s' 'aws4_request' | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${k_service}" -hex | sed 's/^.* //')"
    signature="$(printf '%b' "${string_to_sign}" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${k_signing}" -hex | sed 's/^.* //')"
    auth="AWS4-HMAC-SHA256 Credential=${S3_ACCESS_KEY}/${scope}, SignedHeaders=${signed_headers}, Signature=${signature}"
    local url="${S3_ENDPOINT}/${S3_BUCKET}/${key}"
    [[ -n "${query}" ]] && url="${url}?${query}"
    local args=(
        -fsS -X "${method}"
        -H "Host: ${host}"
        -H "x-amz-date: ${amz_date}"
        -H "x-amz-content-sha256: ${payload_hash}"
        -H "Authorization: ${auth}"
    )
    if [[ -n "${payload_file}" ]]; then
        curl "${args[@]}" --data-binary "@${payload_file}" "${url}"
    else
        curl "${args[@]}" "${url}"
    fi
}

s3_request PUT "${KEY}" "" "${FILE}"
echo "[backup] 已上传 s3://${S3_BUCKET}/${KEY}"

# 远端清理：删除早于保留期的 key（key 名为 xueban/YYYYMMDDTHHMMSSZ.sql.gz，按日期前缀比较）。
cutoff="$(date -u -d "-${RETENTION} days" +%Y%m%d)"
while IFS= read -r k; do
    stamp="$(basename "${k}" .sql.gz | cut -c1-8)"
    if [[ "${stamp}" < "${cutoff}" ]]; then
        s3_request DELETE "${k}" ""
        echo "[backup] 已删除过期远端备份 ${k}"
    fi
done < <(s3_request GET "" "list-type=2" | grep -o '<Key>[^<]*</Key>' | sed 's#</\?Key>##g')
