# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This file will be sourced by all .t tests. Put general purposed functions
# here.

_repocount=0

if [ -n "$USE_MONONOKE" ] ; then
  . "$TESTDIR/../../mononoke/tests/integration/library.sh"
fi

sl() {
  HGIDENTITY=sl hg "$@"
}

dummysshcmd() {
  if [ -n "$DUMMYSSH" ]
  then
    echo "$DUMMYSSH"
  else
    echo "$PYTHON $TESTDIR/dummyssh"
  fi
}

# Create a new repo
newrepo() {
  reponame="$1"
  shift
  if [ -z "$reponame" ]; then
    _repocount=$((_repocount+1))
    reponame=repo$_repocount
  fi
  mkdir "$TESTTMP/$reponame"
  cd "$TESTTMP/$reponame"
  hg init "$@"
}

newclientrepo() {
  reponame="$1"
  server="$2"
  shift
  shift
  bookmarks="$@"
  if [ -z "$reponame" ]; then
    _repocount=$((_repocount+1))
    reponame=repo$_repocount
  fi
  if [ -z "$server" ]; then
      server="test:${reponame}_server"
  fi
  hg clone --config "init.use-rust=True" --config "clone.use-rust=True" --shallow -q "$server" "$TESTTMP/$reponame"

  cd "$TESTTMP/$reponame"
  for book in $bookmarks ; do
      hg pull -q -B $book
  done
  hg up -q tip
}

# create repo connected to remote repo ssh://user@dummy/server.
# `newserver server` needs to be called at least once before this call to setup ssh repo
newremoterepo() {
  newrepo "$@"
  echo remotefilelog >> .hg/requires
  enable treemanifest remotefilelog pushrebase remotenames
  setconfig treemanifest.sendtrees=True treemanifest.treeonly=True
  if [ -n "$USE_MONONOKE" ] ; then
    setconfig paths.default=mononoke://$(mononoke_address)/server
  else
    setconfig paths.default=ssh://user@dummy/server
  fi
}

newserver() {
  local reponame="$1"
  if [ -n "$USE_MONONOKE" ] ; then
    REPONAME=$reponame setup_common_config
    mononoke
    MONONOKE_START_TIMEOUT=60 wait_for_mononoke "$TESTTMP/$reponame"
  elif [ -f "$TESTTMP/.eagerepo" ] ; then
    # Do nothing, it will be setup at access time
    true
  else
    mkdir "$TESTTMP/$reponame"
    cd "$TESTTMP/$reponame"
    hg --config extensions.lz4revlog= \
      --config extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py \
      --config experimental.narrow-heads=false \
      --config visibility.enabled=false \
      init
    enable lz4revlog remotefilelog remotenames
    setconfig \
       remotefilelog.reponame="$reponame" remotefilelog.server=True \
       treemanifest.rustmanifest=True \
       treemanifest.server=True treemanifest.treeonly=True \
       infinitepush.server=yes infinitepush.reponame="$reponame" \
       infinitepush.indextype=disk infinitepush.storetype=disk \
       experimental.narrow-heads=false \
       extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  fi
}

clone() {
  servername="$1"
  clientname="$2"
  shift 2
  cd "$TESTTMP"
  remotecmd="hg"
  if [ -f "$TESTTMP/.eagerepo" ] ; then
      serverurl="test:$servername"
  elif [ -n "$USE_MONONOKE" ] ; then
      serverurl="mononoke://$(mononoke_address)/$servername"
  else
      serverurl="ssh://user@dummy/$servername"
  fi

  hg clone -q --shallow "$serverurl" "$clientname" "$@" \
    --config "init.use-rust=True" \
    --config "extensions.lz4revlog=" \
    --config "extensions.remotefilelog=" \
    --config "extensions.remotenames=" \
    --config "extensions.treemanifest=" \
    --config "remotefilelog.reponame=$servername" \
    --config "treemanifest.treeonly=True" \
    --config "ui.ssh=$(dummysshcmd)" \
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
rustmanifest=True
sendtrees=True
treeonly=True

[ui]
ssh=$(dummysshcmd)

[tweakdefaults]
rebasekeepdate=True
EOF

  if [ -n "$COMMITCLOUD" ]; then
    hg --cwd $clientname cloud join -q
  fi
}

switchrepo() {
    reponame="$1"
    cd $TESTTMP/$reponame
}

# Set configuration for feature
configure() {
  for name in "$@"
  do
    case "$name" in
      dummyssh)
        export DUMMYSSH_STABLE_ORDER=1
        setconfig ui.ssh="$(dummysshcmd)"
        ;;
      mutation)
        setconfig \
            experimental.evolution=obsolete \
            mutation.enabled=true mutation.record=true mutation.date="0 0" \
            visibility.enabled=true
        ;;
      mutation-norecord)
        setconfig \
            experimental.evolution=obsolete \
            mutation.enabled=true mutation.record=false mutation.date="0 0" \
            visibility.enabled=true
        ;;
      evolution)
         setconfig \
            experimental.evolution="createmarkers, allowunstable" \
            mutation.enabled=false \
            visibility.enabled=false
        ;;
      noevolution)
         setconfig \
            experimental.evolution=obsolete \
            mutation.enabled=false \
            visibility.enabled=false
        ;;
      commitcloud)
        enable commitcloud infinitepush
        setconfig commitcloud.hostname=testhost
        setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
        setconfig commitcloud.remotebookmarkssync=True
        COMMITCLOUD=1
        ;;
      narrowheads)
        configure noevolution mutation-norecord
        setconfig experimental.narrow-heads=true
        ;;
      selectivepull)
        enable remotenames
        setconfig remotenames.selectivepull=True
        setconfig remotenames.selectivepulldefault=master
        ;;
      modern)
        enable amend
        setconfig remotenames.rename.default=remote
        setconfig remotenames.hoist=remote
        setconfig experimental.changegroup3=True
        configure dummyssh commitcloud narrowheads selectivepull
        ;;
      modernclient)
        touch $TESTTMP/.eagerepo
        enable remotefilelog
        setconfig remotefilelog.http=True
        setconfig treemanifest.http=True
        configure modern
    esac
  done
}

# Enable extensions
enable() {
  for name in "$@"
  do
    setconfig "extensions.$name="
  done
}

# Disable extensions
disable() {
  for name in "$@"
  do
    setconfig "extensions.$name=!"
    if [[ $name == "treemanifest" ]]; then
        setconfig treemanifest.sendtrees=False treemanifest.treeonly=False
    fi
  done
}

# Like "hg debugdrawdag", but do not leave local tags in the repo and define
# nodes as environment variables.
_drawdagcount=0
drawdag() {
  _drawdagcount=$((_drawdagcount+1))
  local env_path="$TESTTMP/.drawdag-${_drawdagcount}"
  hg debugdrawdag "$@" --config remotenames.autopullhoistpattern= --no-bookmarks --write-env="$env_path" "$@"
  local _exitcode="$?"
  [[ -f "$env_path" ]] && source "$env_path"
  return $_exitcode
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

# Set config item, but always in the main hgrc
setglobalconfig() {
  ( cd "$TESTTMP" ; setconfig "$@" )
}

# Set config items that enable modern features.
setmodernconfig() {
  enable remotenames amend
  setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true mutation.date="0 0" experimental.evolution=obsolete remotenames.rename.default=remote
}

# Read config from stdin (usually a heredoc).
readconfig() {
  local hgrcpath
  if [ -e ".hg" ]
  then
    hgrcpath=".hg/hgrc"
  else
    hgrcpath="$HGRCPATH"
  fi
  cat >> "$hgrcpath"
}

# Read global config from stdin (usually a heredoc).
readglobalconfig() {
  cat >> "$HGRCPATH"
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
  hg log --graph -T "{node|short} {desc|firstline}" | sed \$d
}

tglog() {
  hg log -G -T "{node|short} '{desc}' {bookmarks}" "$@"
}

tglogp() {
  hg log -G -T "{node|short} {phase} '{desc}' {bookmarks}" "$@"
}

tglogm() {
  hg log -G -T "{node|short} '{desc|firstline}' {bookmarks} {join(mutations % '(Rewritten using {operation} into {join(successors % \'{node|short}\', \', \')})', ' ')}" "$@"
}
