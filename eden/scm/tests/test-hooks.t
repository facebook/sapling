#chg-compatible
#require no-windows

  $ newrepo

  $ setconfig 'hooks.pre-files=echo PRE_ARGS: $HG_ARGS' 'hooks.post-files=echo POST_ARGS: $HG_ARGS'
  $ sl files "with ' space" hello
  PRE_ARGS: files 'with ' \'' space' hello
  POST_ARGS: files 'with ' \'' space' hello
  [1]

  $ setconfig 'hooks.fail-files=echo FAIL_ARGS: $HG_ARGS'
  $ sl files -r X "with ' space" hello 2>/dev/null
  PRE_ARGS: files -r X 'with ' \'' space' hello
  FAIL_ARGS: files -r X 'with ' \'' space' hello
  [255]
