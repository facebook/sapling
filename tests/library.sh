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
[ui]
ssh=python "$TESTDIR/dummyssh"
remotecmd=$TESTDIR/../../hg/hg
[server]
preferuncompressed=True
EOF
export HGRCPATH=$PWD/.hgrc

function hgcloneshallow() {
  local name
  local dest
  orig=$1
  shift
  dest=$1
  shift
  hg clone --shallow --config remotefilelog.reponame=master $orig $dest $@
  cat >> $dest/.hg/hgrc <<EOF
[remotefilelog]
reponame=master
EOF
}

function hginit() {
  local name
  name=$1
  shift
  hg init $name $@
}

function clearcache() {
  rm -rf $CACHEDIR/*
}
