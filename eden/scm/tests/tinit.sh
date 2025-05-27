# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This file will be sourced by all .t tests. Put general purposed functions
# here.

_repocount=0

if [ -n "$USE_MONONOKE" ] && [ -z "$DEBUGRUNTEST_ENABLED" ] ; then
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
    if [ -n "$USE_MONONOKE" ] ; then
      server="${reponame}"
    else
      server="${reponame}_server"
    fi
  fi
  if [ -z "$USE_MONONOKE" ] ; then
    remflog="--config remotefilelog.reponame=${reponame}"
  else
    newserver "${server}"
    remflog=""
  fi
  hg clone --config "clone.use-rust=True" $remflog -q "test:${server}" "$TESTTMP/$reponame"

  local drawdaginput=""
  while IFS= read line
  do
    drawdaginput="${drawdaginput}:${line}\n"
  done
  if [ -n "${drawdaginput}" ]; then
      cd "$TESTTMP/${server#*:}"
      echo "${drawdaginput}" | drawdag
  fi

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
  enable pushrebase
  if [ -n "$USE_MONONOKE" ] ; then
    setconfig paths.default=mononoke://$(mononoke_address)/server
  else
    setconfig paths.default=ssh://user@dummy/server
  fi
}

newserver() {
  local reponame="$1"
  mkdir -p "$TESTTMP/.servers"
  if [ -f "$TESTTMP/.servers/$reponame" ]; then
    return 0
  fi
  touch "$TESTTMP/.servers/$reponame"
  if [ -n "$USE_MONONOKE" ] ; then
    REPONAME=$reponame setup_common_config
    mononoke
    MONONOKE_START_TIMEOUT=60 wait_for_mononoke
    REPOID=${REPOID:-0}
    export REPOID=$((REPOID+1))
  elif [ -f "$TESTTMP/.eagerepo" ] ; then
    hg init "$TESTTMP/$reponame" --config format.use-eager-repo=true
    cd "$TESTTMP/$reponame"
  else
    mkdir "$TESTTMP/$reponame"
    cd "$TESTTMP/$reponame"
    hg --config experimental.narrow-heads=false \
      --config visibility.enabled=false \
      init
    setconfig \
       remotefilelog.reponame="$reponame" remotefilelog.server=True \
       infinitepush.server=yes infinitepush.reponame="$reponame" \
       infinitepush.indextype=disk infinitepush.storetype=disk \
       experimental.narrow-heads=false
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

  hg clone -q "$serverurl" "$clientname" "$@" \
    --config "remotefilelog.reponame=$servername" \
    --config "ui.ssh=$(dummysshcmd)" \
    --config "ui.remotecmd=$remotecmd"

  cat >> $clientname/.hg/hgrc <<EOF
[phases]
publish=False

[remotefilelog]
reponame=$servername

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
            mutation.enabled=true mutation.record=true \
            visibility.enabled=true
        ;;
      mutation-norecord)
        setconfig \
            experimental.evolution=obsolete \
            mutation.enabled=true mutation.record=false \
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
        enable commitcloud
        setconfig commitcloud.hostname=testhost
        setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
        setconfig commitcloud.remotebookmarkssync=True
        COMMITCLOUD=1
        ;;
      narrowheads)
        configure noevolution mutation-norecord
        setconfig experimental.narrow-heads=true
        ;;
      modern)
        enable amend
        setconfig remotenames.rename.default=remote
        setconfig remotenames.hoist=remote
        setconfig experimental.changegroup3=True
        configure dummyssh commitcloud narrowheads
        ;;
      modernclient)
        touch $TESTTMP/.eagerepo
        setconfig remotefilelog.http=True
        setconfig treemanifest.http=True
        configure modern
    esac
  done
}

eagerepo() {
  configure modernclient
  setconfig format.use-eager-repo=True
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
  done
}

# Like "hg debugdrawdag", but do not leave local tags in the repo and define
# nodes as environment variables.
_drawdagcount=0
drawdag() {
  _drawdagcount=$((_drawdagcount+1))
  local env_path="$TESTTMP/.drawdag-${_drawdagcount}"
  sl debugdrawdag --config remotenames.autopullhoistpattern= --no-bookmarks --write-env="$env_path" "$@"
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
  hg debugpython -- "$RUNTESTDIR/setconfig.py" "$@"
}

# Set config item, but always in the main hgrc
setglobalconfig() {
  ( cd "$TESTTMP" ; setconfig "$@" )
}

# Set config items that enable modern features.
setmodernconfig() {
  enable amend
  setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true experimental.evolution=obsolete remotenames.rename.default=remote
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
