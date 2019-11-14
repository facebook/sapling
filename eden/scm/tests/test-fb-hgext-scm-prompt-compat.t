Initialize scm prompt compatibility layer
  $ . $TESTDIR/../contrib/scm-prompt.sh

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
   (00000000) (no-eol)
  $ cmd hg commit -m 'c1'
   (5cad84d1) (no-eol)
  $ cmd hg book active
   (active) (no-eol)

Test old mode
  $ export WANT_OLD_SCM_PROMPT
  $ WANT_OLD_SCM_PROMPT=1
  $ cmd hg book -i
  5cad84d1 (no-eol)
  $ cmd hg book active
  active (no-eol)

Test format string
  $ oldcmd() {
  >   "$@"
  >   _dotfiles_scm_info "g g %s g g\n"
  > }
  $ hg init repo
  $ oldcmd cd repo
  g g empty g g

Test main prompt with no format string
  $ _scm_prompt
   (empty) (no-eol)
