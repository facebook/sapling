#!/bin/bash
#-*- coding: utf-8 -*-
#
# Generate invalid_utf8.svndump
#

#check svnadmin version, must be >= 1.7
SVNVERSION=$(svnadmin --version | head -n 1 | cut -d \  -f 3)
if [[ "$SVNVERSION" < '1.7' ]] ; then
    echo "You MUST have svn 1.7 or above to use this script"
    exit 1
fi

set -x

TMPDIR=$(mktemp -d)
WD=$(pwd)

cd $TMPDIR

svnadmin create failrepo
svn co file://$PWD/failrepo fail
(
   cd fail
   touch A
   svn add A
   svn ci -m blabargrod
)
svnadmin --pre-1.6-compatible create invalid_utf8
svnadmin dump failrepo | \
    sed "s/blabargrod/$(echo blåbærgrød | iconv -f utf-8 -t latin1)/g" | \
    svnadmin load --bypass-prop-validation invalid_utf8

tar cz -C invalid_utf8 -f "$WD"/invalid_utf8.tar.gz .
