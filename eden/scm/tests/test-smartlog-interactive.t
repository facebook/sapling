#require no-windows no-eden
  $ enable smartlog
  $ disable commitcloud
  $ newclientrepo
  $ hg debugdrawdag <<'EOS'
  > c d
  > |/
  > b
  > |
  > a
  > EOS
  $ export HGRCPATH="$HGRCPATH;fb=static"
  $ cat > transcript <<EOF
  > j
  > j
  > q
  > EOF

  $ hg sl -i < transcript
  ===== Screen Refresh =====
  o  f4016ed9f  Today at 00:00  test  d
  │  d
  │
  │ o  a82ac2b38  Today at 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7  Today at 00:00  test  b
  │  b
  │
  o  b173517d0  Today at 00:00  test  a
     a
  ===== Screen Refresh =====
  o  f4016ed9f  Today at 00:00  test  d
  │  d
  │
  │ o  a82ac2b38  Today at 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7  Today at 00:00  test  b
  │  b
  │
  o  b173517d0  Today at 00:00  test  a
     a
  ===== Screen Refresh =====
  o  f4016ed9f  Today at 00:00  test  d
  │  d
  │
  │ o  a82ac2b38  Today at 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7  Today at 00:00  test  b
  │  b
  │
  o  b173517d0  Today at 00:00  test  a
     a
