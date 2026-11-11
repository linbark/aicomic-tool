#!/usr/bin/env bash
# 低负载批量抽取：分块跑、跑完卸模型、块间冷却冷却，避免长时间满载导致过热/重启。
#
#   CHUNK=20 GAP=300 RPM=4 ROUNDS=20 bash inspiration/scripts/gentle-vision.sh
#
# 每块：nice 15 跑 CHUNK 张 -> 卸载模型 -> 睡 GAP 秒。重跑会从断点继续。
set -euo pipefail
cd "$(dirname "$0")/../.."

DB=${DB:-data/inspiration/inspiration.db}
ROOT=${ROOT:-data/inspiration}
MODEL=${MODEL:-qwen3-vl:8b-instruct}
BASE=${BASE:-http://127.0.0.1:11434/v1}
CHUNK=${CHUNK:-20}
GAP=${GAP:-300}
RPM=${RPM:-4}
ROUNDS=${ROUNDS:-20}
BIN=./inspiration/target/release/ie

for r in $(seq 1 "$ROUNDS"); do
  before=$(sqlite3 "$DB" "SELECT count(*) FROM observation WHERE extractor='vision-llm' AND extractor_version='$MODEL';")
  echo "== 第 $r 块：当前 $before 张已标注，跑 $CHUNK 张 =="
  nice -n 15 "$BIN" vision --db "$DB" --root "$ROOT" \
    --provider openai --base-url "$BASE" --model "$MODEL" \
    --rpm "$RPM" --limit "$CHUNK" || true
  # 卸载模型，释放内存与 GPU
  curl -sS -m 10 http://127.0.0.1:11434/api/generate \
    -d "{\"model\":\"$MODEL\",\"keep_alive\":0}" >/dev/null 2>&1 || true
  after=$(sqlite3 "$DB" "SELECT count(*) FROM observation WHERE extractor='vision-llm' AND extractor_version='$MODEL';")
  echo "== 本块完成：+$((after - before))，累计 $after =="
  [ "$r" -lt "$ROUNDS" ] && sleep "$GAP"
done
