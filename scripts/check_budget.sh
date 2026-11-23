#!/bin/bash
set -e
fail=0
echo "== src <=200 =="
for f in $(find crates -name "*.rs" -not -path "*tests*" -not -path "*target*"); do
  n=$(wc -l < "$f")
  if [ "$n" -gt 200 ]; then echo "OVER $n $f"; fail=1; else echo "ok $n $f"; fi
done
echo "== tests <=250 =="
for f in $(find crates -path "*tests/*.rs"); do
  n=$(wc -l < "$f")
  if [ "$n" -gt 250 ]; then echo "OVER $n $f"; fail=1; else echo "ok $n $f"; fi
done
exit $fail
