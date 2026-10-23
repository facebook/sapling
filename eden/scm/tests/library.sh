# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

CACHEDIR="$TESTTMP/hgcache"
export DUMMYSSH_STABLE_ORDER=1
cat >> $HGRCPATH <<EOF
[remotefilelog]
cachepath=$CACHEDIR
debug=True
[extensions]
remotefilelog=
rebase=
[ui]
ssh=$(dummysshcmd)
[experimental]
changegroup3=True
[rebase]
singletransaction=True
EOF

_cachecount=0

newcachedir() {
  _cachecount=$((_cachecount+1))
  CACHEDIR="$TESTTMP/hgcache$_cachecount"
  setconfig remotefilelog.cachepath="$CACHEDIR"
}

hgcloneshallow() {
  local name
  local dest
  orig=$1
  shift
  dest=$1
  shift
  sl clone --config remotefilelog.reponame=master $orig $dest $@
  cat >> "$dest/$(_dotdir_configfile "$dest")" <<EOF
[remotefilelog]
reponame=master
[phases]
publish=False
EOF
}

hginit() {
  local name
  name=$1
  shift
  sl init $name $@ --config remotefilelog.reponame=master
}

clearcache() {
  rm -rf $CACHEDIR/*
}

mkcommit() {
  echo "$1" > "$1"
  sl add "$1"
  sl ci -m "$1"
}

ls_l() {
  $PYTHON $TESTDIR/ls-l.py "$@"
}

findfilessorted() {
  find "$1" -type f | sort
}
