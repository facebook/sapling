#debugruntest-compatible
#require git no-windows no-eden

  $ . $TESTDIR/git.sh
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git
  $ setconfig ghrevset.autopull=True
  $ enable smartlog amend github ghstack
  $ setconfig extensions.mock_ghrevset=$TESTDIR/github/mock_ghrevset.py
  $ setconfig templatealias.sl_github="\"{desc} {node|short} {if(github_pull_request_url, '#{github_pull_request_number}')}\""

Prepare upstream server repo w/ two commits on "main":

  $ git init -q upstream
  $ cd upstream
  $ git branch -m main
  $ echo foo > foo
  $ git add foo
  $ git commit -qa -m foo
  $ git checkout -qb neobranch
  $ echo fork-existing-branch > neobranch
  $ git add neobranch
  $ git commit -qa -m neobranch
  $ git checkout main -q
  $ echo bar > bar
  $ git add bar
  $ git commit -qa -m bar

Clone git repo as Sapling repo
  $ cd ..
  $ sl clone --git -q file://$TESTTMP/upstream client
  $ cd client
  $ tglog
  @  ada74d65d813 'bar'
  │
  o  af065c3057b1 'foo'

Make sure revset works by autopulling by using goto
  $ sl goto pr7 -q
  $ sl smartlog -T "{sl_github}"
  o  bar ada74d65d813
  │
  │ @  neobranch 4ce18fc3106a #7
  ├─╯
  o  foo af065c3057b1

Amending a local commit should maintain PR no
  $ echo meh > meh
  $ sl amend
  $ sl goto main -q
  $ sl goto pr7 -q
  $ sl smartlog -T "{sl_github}"
  o  bar ada74d65d813
  │
  │ @  neobranch 902a89e783d4 #7
  ├─╯
  o  foo af065c3057b1

Looking locally for a commit should be possible even if upstream is not available
  $ rm -rf ../upstream
  $ sl goto main -q
  $ sl goto pr7 -q
  $ sl log -l 1 -T "{sl_github}\n"
  neobranch 902a89e783d4 #7
