#!/bin/bash
# 软约束：只告警，不失败。阈值 src 2000 / tests 2500
fail=0
echo "== src (warn at >2000) =="
for f in $(find crates -name "*.rs" -not -path "*tests*" -not -path "*target*"); do
  n=$(wc -l < "$f")
  if [ "$n" -gt 2000 ]; then echo "WARN $n $f (consider split)"; fi
done
echo "== tests (warn at >2500) =="
for f in $(find crates -path "*tests/*.rs"); do
  n=$(wc -l < "$f")
  if [ "$n" -gt 2500 ]; then echo "WARN $n $f"; fi
done
echo "budget check done (soft, always pass)"
exit 0
