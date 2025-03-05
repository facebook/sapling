
#require no-eden

#inprocess-hg-incompatible

  $ eagerepo
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
  hint[ghstack-deprecation]: 
  ┌───────────────────────────────────────────────────────────────┐
  │ Native ghstack command in Sapling will be removed.            │
  │ Please use `.git` mode [1] and upstream ghstack [2] instead.  │
  │ [1]: https://sapling-scm.com/docs/git/git_support_modes/      │
  │ [2]: https://github.com/ezyang/ghstack                        │
  └───────────────────────────────────────────────────────────────┘
  abort: not a Git repo
  [255]
