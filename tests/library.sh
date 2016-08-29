python -c 'import lz4' || exit 80

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
[server]
preferuncompressed=True
[experimental]
changegroup3=True
EOF
export HGRCPATH=$PWD/.hgrc

hgcloneshallow() {
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

hginit() {
  local name
  name=$1
  shift
  hg init $name $@
}

clearcache() {
  rm -rf $CACHEDIR/*
}
