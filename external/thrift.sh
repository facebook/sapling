#!/bin/bash
#
# Copyright 2004-present Facebook. All Rights Reserved.
#
# This is a small wrapper script to invoke the python-based fbthrift compiler
# from it's build location in externals/fbthrift.
#
# Buck needs to be configured with a single command to use for invoking thrift,
# so we point it at this wrapper script.
FBTHRIFT_DIR=$(dirname "$0")/fbthrift
PLATFORM='linux-x86_64-2.7'

export PYTHONPATH="$FBTHRIFT_DIR/thrift/compiler/py/build/lib.$PLATFORM"
python "$FBTHRIFT_DIR/thrift/compiler/py/main.py" "$@"
