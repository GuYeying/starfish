#!/bin/bash
# 连拍模式：检测到 starfish ANCO CREATE 立即连拍 8 张（无初始延迟）
cd /d/Projects/Rust/starfish
LAST=$(grep -c "ANCO CREATE.*starfish" wt_hilog_capture.log 2>/dev/null || echo 0)
echo "baseline create count: $LAST"
for i in $(seq 1 300); do
  N=$(grep -c "ANCO CREATE.*starfish" wt_hilog_capture.log 2>/dev/null || echo 0)
  if [ "$N" -gt "$LAST" ]; then
    LAST=$N
    echo "detected launch #$N, bursting..."
    for k in 1 2 3 4 5 6 7 8; do
      MSYS_NO_PATHCONV=1 hdc shell "snapshot_display -f /data/local/tmp/burst_${N}_${k}.jpeg" >/dev/null 2>&1
      MSYS_NO_PATHCONV=1 hdc file recv /data/local/tmp/burst_${N}_${k}.jpeg "burst_${N}_${k}.jpeg" >/dev/null 2>&1
      echo "shot $k done: $(date +%H:%M:%S.%N)"
    done
    echo "burst complete for launch #$N"
  fi
  sleep 0.5
done
echo "watcher done"
