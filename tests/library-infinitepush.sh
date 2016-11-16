scratchnodes() {
  for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
     echo ${node##*/} `cat $node`
  done
}

scratchbookmarks() {
  for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
     echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  done
}

setupcommon() {
  extpath=`dirname $TESTDIR`
  cp -r $extpath/infinitepush $TESTTMP
  cp -r $extpath/infinitepush $TESTTMP

  cat >> $HGRCPATH << EOF
[extensions]
infinitepush=$TESTTMP/infinitepush
[ui]
ssh = python "$TESTDIR/dummyssh"
[infinitepush]
branchpattern=re:scratch/.*
EOF
}

setupserver() {
cat >> .hg/hgrc << EOF
[infinitepush]
server=yes
indextype=disk
storetype=disk
EOF
}
