#!/usr/bin/env bash
# 单模型全量语义抽取（计费层用）：先把旧观察清空，保证一批数据来自同一个模型，
# 再按 shard 并发跑，最后打印覆盖统计。
#
#   GEMINI_API_KEY=... MODEL=gemini-3.5-flash SHARDS=3 RPM=30 bash inspiration/scripts/run-pass.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

DB=${DB:-data/inspiration/inspiration.db}
ROOT=${ROOT:-data/inspiration}
MODEL=${MODEL:-gemini-3.5-flash}
SHARDS=${SHARDS:-3}
RPM=${RPM:-30}
BIN=./inspiration/target/release/ie

: "${GEMINI_API_KEY:?需要 GEMINI_API_KEY 环境变量}"
[ -x "$BIN" ] || { echo "先构建：cd inspiration && cargo build --release --offline"; exit 1; }

cp "$DB" "$DB.bak-$(date +%Y%m%d%H%M)"   # 先备份，失败可回滚
sqlite3 "$DB" "PRAGMA foreign_keys=ON; DELETE FROM observation; DELETE FROM work_primitive; DELETE FROM device_usage;"
"$BIN" extract --db "$DB" --root "$ROOT"

pids=()
for i in $(seq 0 $((SHARDS - 1))); do
  "$BIN" vision --db "$DB" --root "$ROOT" --model "$MODEL" --rpm "$RPM" --shard "$i/$SHARDS" \
    > "/tmp/ie-vision-$i.log" 2>&1 &
  pids+=($!)
done
echo "已启动 $SHARDS 个分片，日志：/tmp/ie-vision-*.log"
wait "${pids[@]}" || true
"$BIN" stats --db "$DB"
