#!/bin/bash

set -e

cd `git rev-parse --show-toplevel`

buck build eden-cli eden-daemon
OUTPUT_DIR=buck-out/eden
mkdir -p $OUTPUT_DIR

EDEN_CLI=`buck targets --show-output eden-cli | awk '{print $2}'`
EDEN_DAEMON=`buck targets --show-output eden-daemon | awk '{print $2}'`

cp "$EDEN_CLI" "$OUTPUT_DIR/eden"
cp "$EDEN_DAEMON" "$OUTPUT_DIR/daemon"
sudo chmod 4755 "$OUTPUT_DIR/daemon"

echo "Eden executable available in $OUTPUT_DIR/eden."
