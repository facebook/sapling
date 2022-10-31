#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

INFINITEPUSH_TESTDIR="${RUN_TESTS_LIBRARY:-"$TESTDIR"}"

scratchnodes() {
  for node in `find ../repo/.hg/scratchbranches/index/nodemap -type f | LC_ALL=C sort`; do
     echo ${node##*/} `cat $node`
  done
}

scratchbookmarks() {
  for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap -type f | LC_ALL=C sort`; do
     echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  done
}

setupcommon() {
  cat >> $HGRCPATH << EOF
[extensions]
commitcloud=
infinitepush=
pullcreatemarkers=
[ui]
ssh=$(dummysshcmd)
[infinitepush]
branchpattern=re:scratch/.*
bgssh=$(dummysshcmd) -bgssh
[remotenames]
autopullhoistpattern=re:^[a-z0-9A-Z/]*$
hoist=default
EOF
}

setupserver() {
cat >> .hg/hgrc << EOF
[infinitepush]
server=yes
EOF
}

waitbgbackup() {
  sleep 1
  hg debugwaitbackup
}

mkcommitautobackup() {
    echo $1 > $1
    hg add $1
    hg ci -m $1 --config infinitepushbackup.autobackup=True
}

setuplogdir() {
  mkdir $TESTTMP/logs
  chmod 0755 $TESTTMP/logs
  chmod +t $TESTTMP/logs
}

debugsshcall() {
  sed -n '/^running .*dummyssh.*$/p'
}
