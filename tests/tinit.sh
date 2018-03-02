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
