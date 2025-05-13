#require git no-eden

  $ eagerepo
Can init with --git in an existing directory
  $ mkdir init-git-nonempty
  $ cd init-git-nonempty
  $ echo hello > hello

  $ sl init --git .
  $ sl status
  ? hello

Re-init is an error
  $ sl init --git
  abort: repository `$TESTTMP/init-git-nonempty` already exists
  [255]

Test without any options in oss mode
  $ mkdir $TESTTMP/git_fallback_repo
  $ cd $TESTTMP/git_fallback_repo
  $ setconfig init.prefer-git=true
  $ sl init
  abort: please use 'sl init --git .' for a better experience
  [255]
