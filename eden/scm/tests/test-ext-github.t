#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ enable github
  $ enable ghstack

Build up a non-github repo

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ hg ci -Am addfile
  adding a1

Confirm 'github_repo' does not error
  $ hg log -r. -T '{github_repo}'
  False (no-eol)

Confirm pull request creation will fail
  $ hg pr submit
  abort: not a Git repo
  [255]
  $ hg ghstack
  abort: not a Git repo
  [255]
