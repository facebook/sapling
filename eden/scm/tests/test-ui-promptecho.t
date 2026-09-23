#require no-eden

  $ eagerepo
  $ newclientrepo repo
  $ unset CODING_AGENT_METADATA
  $ cat >> $HGRCPATH <<'EOF'
  > [ui]
  > interactive=true
  > %unset promptecho
  > EOF

Non-terminal input is echoed by default so captured output includes the
response.

  $ printf 'answer\n' | sl debugshell -c 'print(ui.prompt("response?"))'
  response? answer
  answer

Explicit configuration still takes precedence.

  $ printf 'answer\n' | sl --config ui.promptecho=false debugshell -c 'print(ui.prompt("response?"))'
  response? answer
