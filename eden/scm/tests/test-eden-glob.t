
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

test eden glob

  $ newclientrepo
# EdenAPI eagerepo implementation for glob is currently mocked out so don't need to add things to repo yet
  $ eden glob '**/*.txt'
  foo.txt
  baz.txt
  $ eden glob **/*.rs
  bar.rs
