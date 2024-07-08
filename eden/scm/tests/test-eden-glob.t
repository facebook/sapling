
#require eden

setup backing repo

  $ cat > $TESTTMP/.edenrc <<EOF
  > [glob]
  > use-edenapi-suffix-query = true
  > EOF
#if no-windows
  $ eden restart 2>1 > /dev/null
#else
  $ eden --home-dir $TESTTMP restart 2>1 > /dev/null
#endif
  $ newclientrepo

# EdenAPI eagerepo implementation for glob is currently mocked out so don't need to add things to repo yet
test eden glob

  $ eden debug logging eden/fs/service=DBG4 > /dev/null
  $ eden glob '**/*.txt' --list-only-files
  foo.txt
  baz.txt
  $ mkdir depth1
  $ cd depth1
# return nothing due to not being in repo root
  $ eden glob **/*.rs --list-only-files
# Add repo flag to use root instead of cwd
  $ eden glob **/*.rs --list-only-files --repo $TESTTMP/repo1
  bar.rs
  $ mkdir depth2
  $ cd depth2
  $ eden glob **/*.dot --list-only-files --repo $TESTTMP/repo1
  throw.dot
  $ cd ../..
  $ eden glob **/*.dot --include-dot-files --list-only-files
  throw.dot
  .more.dot
  .stop.dot
  .dps/very.dot
  slowly/.and.by.slow.dot
  i/.mean/slow.dot
  $ eden glob **/*.local --list-only-files
  $ touch local.local
  $ eden glob **/*.local --list-only-files
  local.local
