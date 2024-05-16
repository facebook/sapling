#chg-compatible
#require no-windows
#debugruntest-incompatible
  $ configure modernclient
  $ newclientrepo

  $ setconfig 'hooks.pre-files=echo PRE_ARGS: $HG_ARGS' 'hooks.post-files=echo POST_ARGS: $HG_ARGS'
  $ hg files "with ' space" hello
  PRE_ARGS: files 'with '\'' space' hello
  POST_ARGS: files 'with '\'' space' hello
  [1]

  $ setconfig 'hooks.fail-files=echo FAIL_ARGS: $HG_ARGS'
  $ setconfig 'extensions.crash=crash.py'
  $ cat > crash.py <<EOF
  > import sapling
  > # Make command crash
  > sapling.cmdutil.files = None
  > EOF
  $ hg files "with ' space" hello 2>/dev/null
  PRE_ARGS: files 'with '\'' space' hello
  FAIL_ARGS: files 'with '\'' space' hello
  [1]
