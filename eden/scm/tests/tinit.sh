# This file will be sourced by all .t tests. Put general purposed functions
# here.

_repocount=0

if [ -n "$USE_MONONOKE" ] ; then
  . "$TESTDIR/../../../scm/mononoke/tests/integration/library.sh"
fi

# Create a new repo
newrepo() {
  reponame="$1"
  if [ -z "$reponame" ]; then
    _repocount=$((_repocount+1))
    reponame=repo$_repocount
  fi
  mkdir "$TESTTMP/$reponame"
  cd "$TESTTMP/$reponame"
  hg init
}

newserver() {
  local reponame="$1"
  if [ -n "$USE_MONONOKE" ] ; then
    REPONAME=$reponame setup_mononoke_config
    mononoke
    wait_for_mononoke "$TESTTMP/$reponame"
  else
    mkdir "$TESTTMP/$reponame"
    cd "$TESTTMP/$reponame"
    hg init --config extensions.lz4revlog=

    cat >> ".hg/hgrc" <<EOF
[extensions]
lz4revlog=
remotefilelog=
remotenames=
treemanifest=

[remotefilelog]
reponame=$reponame
server=True

[treemanifest]
flatcompat=False
rustmanifest=True
server=True
treeonly=True
EOF
  fi
}

clone() {
  servername="$1"
  clientname="$2"
  shift 2
  cd "$TESTTMP"
  remotecmd="hg"
  if [ -n "$USE_MONONOKE" ] ; then
    remotecmd="$MONONOKE_HGCLI"
  fi
  hg clone -q --shallow "ssh://user@dummy/$servername" "$clientname" "$@" \
    --config "extensions.lz4revlog=" \
    --config "extensions.remotefilelog=" \
    --config "extensions.remotenames=" \
    --config "extensions.treemanifest=" \
    --config "remotefilelog.reponame=$servername" \
    --config "treemanifest.treeonly=True" \
    --config "ui.ssh=$TESTDIR/dummyssh" \
    --config "ui.remotecmd=$remotecmd"

  cat >> $clientname/.hg/hgrc <<EOF
[extensions]
lz4revlog=
remotefilelog=
remotenames=
treemanifest=
tweakdefaults=

[phases]
publish=False

[remotefilelog]
reponame=$servername

[treemanifest]
flatcompat=False
rustmanifest=True
sendtrees=True
treeonly=True

[ui]
ssh=$TESTDIR/dummyssh

[tweakdefaults]
rebasekeepdate=True
EOF

  if [ -n "$USE_MONONOKE" ] ; then
      cat >> $clientname/.hg/hgrc <<EOF
[ui]
remotecmd=$MONONOKE_HGCLI
EOF
  fi
}

switchrepo() {
    reponame="$1"
    cd $TESTTMP/$reponame
}

# Enable extensions or features
enable() {
  local rcpath
  # .hg/hgrc may not exist yet, so just check for requires
  if [ -f .hg/requires ]; then
    rcpath=.hg/hgrc
  else
    rcpath="$HGRCPATH"
  fi
  for name in "$@"; do
    if [ "$name" = obsstore ]; then
      cat >> $rcpath << EOF
[experimental]
evolution = createmarkers, allowunstable
EOF
    else
      cat >> $rcpath << EOF
[extensions]
$name=
EOF
    fi
  done
}

# Like "hg debugdrawdag", but do not leave local tags in the repo and define
# nodes as environment variables.
# This is useful if the test wants to hide those commits because tags would
# make commits visible. The function will set environment variables so
# commits can still be referred as $TAGNAME.
drawdag() {
  hg debugdrawdag "$@"
  eval `hg tags -T '{tag}={node}\n'`
  rm -f .hg/localtags
}

# Simplify error reporting so crash does not show a traceback.
# This is useful to match error messages without the traceback.
shorttraceback() {
  enable errorredirect
  setconfig errorredirect.script='printf "%s" "$TRACE" | tail -1 1>&2'
}

# Set config items like --config way, instead of using cat >> $HGRCPATH
setconfig() {
  python "$RUNTESTDIR/setconfig.py" "$@"
}

# Create a new extension
newext() {
  extname="$1"
  if [ -z "$extname" ]; then
    _extcount=$((_extcount+1))
    extname=ext$_extcount
  fi
  cat > "$TESTTMP/$extname.py"
  setconfig "extensions.$extname=$TESTTMP/$extname.py"
}

showgraph() {
  hg log --graph -T "{rev} {node|short} {desc|firstline}" | sed \$d
}

tglog() {
  hg log -G -T "{rev}: {node|short} '{desc}' {bookmarks} {branches}" "$@"
}

tglogp() {
  hg log -G -T "{rev}: {node|short} {phase} '{desc}' {bookmarks} {branches}" "$@"
}

tglogm() {
  hg log -G -T "{rev}: {node|short} '{desc|firstline}' {bookmarks} {join(mutations % '(Rewritten using {operation} into {join(successors % \'{node|short}\', \', \')})', ' ')}" "$@"
}
