#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

Remotenames extension has a shortcut that makes heads discovery work faster.
Unfortunately that may result in sending public commits to the server. This
test covers the issue.

  $ . $TESTDIR/library.sh
  $ . $TESTDIR/infinitepush/library.sh
  $ enable remotenames

  $ setupcommon

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

Setup server with a few commits and one remote bookmark.
  $ hg init repo
  $ cd repo
  $ setupserver
  $ mkcommit first
  $ hg book remotebook
  $ hg up -q .
  $ mkcommit second
  $ mkcommit third
  $ mkcommit fourth
  $ hg bookmark master
  $ cd ..

Create new client
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client -q
  $ cd client

Create scratch commit and back it up.
  $ hg up -q -r 'desc(third)'
  $ mkcommit scratch
  $ hg log -r . -T '{node}\n'
  ce87a066ebc28045311cd1272f5edc0ed80d5b1c
  $ hg log --graph -T '{desc}'
  @  scratch
  │
  │ o  fourth
  ├─╯
  o  third
  │
  o  second
  │
  o  first
  
  $ hg cloud backup
  backing up stack rooted at ce87a066ebc2
  commitcloud: backed up 1 commit
  remote: pushing 1 commit:
  remote:     ce87a066ebc2  scratch
  $ cd ..

Create second client
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client2 -q
  $ cd client2
  $ enable remotenames

Pull to get remote names
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  $ hg book --remote
     default/master            05fb75d88dcd
     default/remotebook        b75a450e74d5

Strip public commits from the repo (still needed?)
  $ hg debugstrip -q -r 'desc(second):'
  $ hg log --graph -T '{desc}'
  @  first
  
Download scratch commit. It also downloads a few public commits
  $ hg up -q ce87a066ebc28045311cd1272f5edc0ed80d5b1c
  $ hg log --graph -T '{desc}'
  @  scratch
  │
  │ o  fourth
  ├─╯
  o  third
  │
  o  second
  │
  o  first
  
  $ hg book --remote
     default/master            05fb75d88dcd
     default/remotebook        b75a450e74d5

Run cloud backup and make sure only scratch commits are backed up.
  $ hg cloud backup
  nothing to back up
  $ mkcommit scratch2
  $ hg cloud backup
  backing up stack rooted at ce87a066ebc2
  commitcloud: backed up 1 commit
  remote: pushing 2 commits:
  remote:     ce87a066ebc2  scratch
  remote:     4dbf2c8dd7d9  scratch2
