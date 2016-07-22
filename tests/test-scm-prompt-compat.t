Initialize scm prompt compatibility layer
  $ . $TESTDIR/../scripts/scm-prompt.sh

  $ cmd() {
  >   "$@"
  >   _dotfiles_scm_info
  > }

A few basic tests
  $ _dotfiles_scm_info
  $ hg init repo
  $ cmd cd repo
   (empty) (no-eol)
  $ echo a > a
  $ cmd hg add a
   (0000000) (no-eol)
  $ cmd hg commit -m 'c1'
   (5cad84d) (no-eol)
  $ cmd hg book active
   (active) (no-eol)

Test old mode
  $ export WANT_OLD_SCM_PROMPT
  $ WANT_OLD_SCM_PROMPT=1
  $ cmd hg book -i
  5cad84d (no-eol)
  $ cmd hg book active
  active (no-eol)
