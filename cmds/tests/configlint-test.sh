#!/bin/bash

CONFIGLINT="$1"; shift
DIR="$1"; shift

if [ !  -d "$DIR" ]; then
  echo "No test fixture dir $DIR found" 1>&2
  exit 1
fi

for TEST in "$DIR"/*; do
  echo "Testing $TEST"
  case "$TEST" in
  *fixtures/OK-*) expected=0;;
  *fixtures/BAD-*) expected=1;;
  esac

  "$CONFIGLINT" --mononoke-config-path "$TEST"

  if [ "$?" != "$expected" ]; then
    exit 1
  fi
done
