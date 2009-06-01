#!/bin/sh
. $(dirname $0)/common.sh

for b in `hg branches -a | cut -f 1 -d ' ' | grep -v closed-branches` ; do
    hg co $b || break
    echo Verifying $b
    verify_current_revision keep > /dev/null && \
        echo $b verified. || \
        (echo $b failed. && /bin/rm -rf * && hg up -C $b)
done
