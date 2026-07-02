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

Test a matrix of:
- Python, Rust commands (log, version).
- Shell, Python hooks.
- External script, and base64 form.

  $ cd
  $ cat > hook.py << 'EOF'
  > def myhook(repo, io, **kwargs):
  >     io.write('hook 2\n')
  > EOF

  $ newrepo
  $ for cmd in version log; do
  >   setconfig "hooks.pre-${cmd}.sh1=echo hook 1"
  >   setconfig hooks.pre-${cmd}.py1=python:../hook.py:myhook
  >   setconfig hooks.pre-${cmd}.sh2=base64:$(echo echo hook 3 | base64 -w0 -)
  >   setconfig hooks.pre-${cmd}.py2=python:base64:$(sed 's/2/4/' ../hook.py | base64 -w0 -):myhook
  > done

  $ sl version -q
  hook 1
  hook 2
  hook 3
  hook 4
  ...

  $ sl log -r . -q -T '\n'
  hook 1
  hook 2
  hook 3
  hook 4
