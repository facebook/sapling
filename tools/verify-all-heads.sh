#!/bin/sh
for b in `hg branches | cut -f 1 -d ' '` ; do
    hg co $b || break
    echo Verifying $b
    $(dirname $0)/bisect-find-bad.sh > /dev/null || break
    echo $b Verified.
done
