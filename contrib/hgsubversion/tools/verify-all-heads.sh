#!/bin/sh
. $(dirname $0)/common.sh

for b in `hg branches -aq` ; do
    hg co $b || break
    echo verifying $b
    hg svn verify
done
