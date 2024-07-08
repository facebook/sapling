
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
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS

# EdenAPI eagerepo implementation for glob is currently mocked out so don't need to add things to repo yet
test eden glob

  $ eden debug logging eden/fs/service=DBG4 > /dev/null
  $ eden glob '**/*.txt' --list-only-files
  baz.txt
  foo.txt
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
  .dps/very.dot
  .more.dot
  .stop.dot
  i/.mean/slow.dot
  slowly/.and.by.slow.dot
  throw.dot

Test local files
  $ eden glob **/*.local --list-only-files
  $ touch local.local
  $ eden glob **/*.local --list-only-files
  local.local
# Test that local files do not show up when using revision
  $ eden glob **/*.local --list-only-files --revision 0000000000000000000000000000000000000000

# Test that local file dtype changes register
  $ hg checkout $A > /dev/null
  $ touch bar.rs
  $ hg add bar.rs
  $ hg amend 2> /dev/null
  $ hg checkout 072da8606ee7 > /dev/null
  $ eden glob **/*.rs --dtype --list-only-files
  bar.rs Regular
  $ mv bar.rs barlink.rs
  $ ln -s barlink.rs bar.rs
  $ eden glob **/*.rs --dtype --list-only-files
  bar.rs Symlink
  barlink.rs Regular
