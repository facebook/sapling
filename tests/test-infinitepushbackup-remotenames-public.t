Remotenames extension has a shortcut that makes heads discovery work faster.
Unfortunately that may result in sending public commits to the server. This
test covers the issue.

  $ . $TESTDIR/require-ext.sh remotenames
  $ . $TESTDIR/library.sh
  $ . $TESTDIR/infinitepush/library.sh
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > infinitepush=
  > infinitepushbackup=
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ scratchnodes() {
  >    for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
  >        echo ${node##*/}
  >    done
  > }
  $ scratchbookmarks() {
  >    for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
  >        echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  >    done
  > }
  $ enableremotenames() {
  >    printf '[extensions]\nremotenames=\n' >> .hg/hgrc
  > }

Setup server with a few commits and one remote bookmark. This remotebookmark
may be used by remotenames extension in fastheaddiscovery heuristic
  $ hg init repo
  $ cd repo
  $ setupserver
  $ mkcommit first
  $ hg book remotebook
  $ hg up -q .
  $ mkcommit second
  $ mkcommit third
  $ mkcommit fourth
  $ cd ..

Create new client
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client -q
  $ cd client
  $ enableremotenames

Create scratch commit and back it up.
  $ hg up -q -r 'desc(third)'
  $ mkcommit scratch
  $ hg log -r . -T '{node}\n'
  ce87a066ebc28045311cd1272f5edc0ed80d5b1c
  $ hg log --graph -T '{desc}'
  @  scratch
  |
  | o  fourth
  |/
  o  third
  |
  o  second
  |
  o  first
  
  $ hg pushbackup
  starting backup * (glob)
  backing up stack rooted at ce87a066ebc2
  remote: pushing 1 commit:
  remote:     ce87a066ebc2  scratch
  finished in * (glob)
  $ cd ..

Create second client
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client2 -q
  $ cd client2
  $ enableremotenames

Pull to get remote names
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  $ hg book --remote
     default/remotebook        0:b75a450e74d5

Strip public commits from the repo, otherwise fastheaddiscovery heuristic will
be skipped
  $ hg strip -q -r '1:'
  $ hg log --graph -T '{desc}'
  @  first
  
Download scratch commit. It also downloads a few public commits
  $ hg up -q ce87a066ebc28045311cd1272f5edc0ed80d5b1c
  'ce87a066ebc28045311cd1272f5edc0ed80d5b1c' does not exist locally - looking for it remotely...
  'ce87a066ebc28045311cd1272f5edc0ed80d5b1c' found remotely
  pull finished in * sec (glob)
  $ hg log --graph -T '{desc}'
  @  scratch
  |
  o  third
  |
  o  second
  |
  o  first
  
  $ hg book --remote
     default/remotebook        0:b75a450e74d5

Run pushbackup and make sure only scratch commit is backed up.
  $ hg pushbackup
  starting backup * (glob)
  backing up stack rooted at ce87a066ebc2
  remote: pushing 1 commit:
  remote:     ce87a066ebc2  scratch
  finished in * (glob)
