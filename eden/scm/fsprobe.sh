#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

REAL_PATH=$(realpath "$0")
DIR_NAME=$(dirname "$REAL_PATH")
FSPROBE=${DIR_NAME}/build/cargo-target/release/fsprobe

if [ ! -f "$FSPROBE" ]; then
    echo "Cannot find release version of fsprobe at $FSPROBE"
    echo "Make sure you build fsprobe first by running cargo build --release in eden/scm/exec/fsprobe"
    exit 1
fi

COMMAND=$1
PLANS=~/.fsprobe/
cd "$(hg root)"

shift || :
case $COMMAND in
  "generate")
    echo "Plans are stored in $PLANS"
    mkdir -p $PLANS
    find . -name TARGETS | awk '$0="cat "$0' > "$PLANS/cat.targets"
    echo "Generated read files plan with$(wc -l $PLANS/cat.targets) actions"
    head -20000 "$PLANS/cat.targets" > "$PLANS/cat.targets.20k"
    head -10000 "$PLANS/cat.targets" > "$PLANS/cat.targets.10k"
  ;;
  "list")
    ls "$PLANS" | cat
  ;;
  "run")
    PLAN=$1
    shift || :
    echo "Running $PLAN"
    $FSPROBE "$PLANS/$PLAN" "$@"
  ;;
  *)
    echo "Usage: fsprobe.sh generate | list | run | help"
    echo
    echo "Note: you must run fsprobe.sh in the repository that you want to test"
    echo "For example, if you have eden mount at ~/fbsource, you need to cd ~/fbsource and run fsprobe.sh from there to test speed of this directory"
    exit 1
    ;;
esac
