
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

  $ LOG=repo=trace eden glob '**/*.txt'
  foo.txt
  baz.txt
  $ eden glob **/*.rs
  bar.rs
  $ eden glob **/*.dot
  throw.dot
  $ eden glob **/*.dot --include-dot-files
  throw.dot
  .more.dot
  .stop.dot
  .dps/very.dot
  slowly/.and.by.slow.dot
  i/.mean/slow.dot
