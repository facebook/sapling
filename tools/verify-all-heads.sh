#!/bin/sh
. $(dirname $0)/common.sh

for b in `hg branches | cut -f 1 -d ' '` ; do
    hg co $b || break
    echo Verifying $b
    verify_current_revision keep > /dev/null || break
    echo $b Verified.
done
