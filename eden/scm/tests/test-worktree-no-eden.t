#require no-eden

  $ newrepo repo

empty or unknown subcommands keep their existing errors

  $ sl worktree
  abort: you need to specify a subcommand (run with --help to see a list)
  [255]

  $ sl worktree unknown
  abort: unknown worktree subcommand 'unknown'
  [255]

all worktree subcommands require an Eden-backed repo

  $ sl worktree add $TESTTMP/linked
  abort: worktree commands require an EdenFS-backed repository
  [255]

  $ sl worktree list
  abort: worktree commands require an EdenFS-backed repository
  [255]

  $ sl worktree label $TESTTMP/linked test
  abort: worktree commands require an EdenFS-backed repository
  [255]

  $ sl worktree remove $TESTTMP/linked -y
  abort: worktree commands require an EdenFS-backed repository
  [255]
