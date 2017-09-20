${PYTHON:-python} -c 'import lz4' || exit 80

CACHEDIR=$PWD/hgcache
cat >> $HGRCPATH <<EOF
[remotefilelog]
cachepath=$CACHEDIR
debug=True
historypackv1=True
datapackversion=1
[extensions]
remotefilelog=
rebase=
mq=
[ui]
ssh=python "$TESTDIR/dummyssh"
[server]
preferuncompressed=True
[experimental]
changegroup3=True
[rebase]
singletransaction=True
EOF

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
datapackversion=1
[phases]
publish=False
EOF
}

hgcloneshallowlfs() {
  local name
  local dest
  local lfsdir
  orig=$1
  shift
  dest=$1
  shift
  lfsdir=$1
  shift
  hg clone --shallow --config "extensions.lfs=" --config "lfs.url=$lfsdir" --config remotefilelog.reponame=master $orig $dest $@
  cat >> $dest/.hg/hgrc <<EOF
[extensions]
lfs=
[lfs]
url=$lfsdir
[remotefilelog]
reponame=master
datapackversion=1
[phases]
publish=False
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

mkcommit() {
  echo "$1" > "$1"
  hg add "$1"
  hg ci -m "$1"
}

ls_l() {
  $PYTHON $TESTDIR/ls-l.py "$@"
}
