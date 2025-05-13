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
  Creating a ".sl" repo with Git compatible storage. For full "git" compatibility, create repo using "git init". See https://sapling-scm.com/docs/git/git_support_modes for more information. (no-eol)
