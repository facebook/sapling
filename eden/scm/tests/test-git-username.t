#require git no-eden

Test that we validate certain username errors.

  $ export RUST_BACKTRACE=0
  $ configure modern
  $ . $TESTDIR/git.sh

  $ newrepo '' --git
  $ sl config --quiet --local 'ui.username=Oopsie Daisy <oopsie@example'
  $ unset HGUSER

  $ touch foo
  $ sl commit -Aqm foo
  abort: invalid name (mismatched brackets): "Oopsie Daisy <oopsie@example"
  [255]
