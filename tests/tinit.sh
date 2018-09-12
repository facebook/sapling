# This file will be sourced by all .t tests. Put general purposed functions
# here.

_repocount=0

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

# Enable extensions or features
enable() {
  for name in "$@"; do
    if [ "$name" = obsstore ]; then
      cat >> $HGRCPATH << EOF
[experimental]
evolution = createmarkers, allowunstable
EOF
    else
      cat >> $HGRCPATH << EOF
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
  setconfig errorredirect.script='printf "%s" "$TRACE" | tail -1'
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
