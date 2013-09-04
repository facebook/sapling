cp `echo $HGRCPATH` ./
CACHEDIR=$PWD/hgcache
cat >> .hgrc <<EOF
[remotefilelog]
cachepath=$CACHEDIR
debug=True
[extensions]
remotefilelog=$TESTDIR/../remotefilelog
rebase=
mq=
EOF
export HGRCPATH=$PWD/.hgrc

function hgcloneshallow() {
  orig=$1
  shift
  dest=$1
  shift
  hg clone --shallow $orig $dest $@
  cat >> $dest/.hg/hgrc <<EOF
[remotefilelog]
cachepath=$PWD/hgcache
debug=True
[extensions]
remotefilelog=$TESTDIR/../remotefilelog
EOF
}

function hginit() {
  name=$1
  shift
  hg init $name $@
  cat >> $name/.hg/hgrc <<EOF
[remotefilelog]
cachepath=$PWD/hgcache
debug=True
[extensions]
remotefilelog=$TESTDIR/../remotefilelog
EOF
}

function clearcache() {
  rm -rf $CACHEDIR/*
}
