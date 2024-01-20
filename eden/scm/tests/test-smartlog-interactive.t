#debugruntest-compatible
#require no-windows
  $ enable smartlog
  $ configure modernclient
  $ disable commitcloud
  $ newclientrepo
  $ hg debugdrawdag <<'EOS'
  > c d
  > |/
  > b
  > |
  > a
  > EOS
  $ export HGRCPATH=fb=static
  $ cat > transcript <<EOF
  > j
  > j
  > q
  > EOF

  $ hg sl -i < transcript
  ===== Screen Refresh =====
  o  f4016ed9f (Not backed up)  1970-01-01 00:00  test  d
  │  d
  │
  │ o  a82ac2b38 (Not backed up)  1970-01-01 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7 (Not backed up)  1970-01-01 00:00  test  b
  │  b
  │
  o  b173517d0 (Not backed up)  1970-01-01 00:00  test  a
     a
  ===== Screen Refresh =====
  o  f4016ed9f (Not backed up)  1970-01-01 00:00  test  d
  │  d
  │
  │ o  a82ac2b38 (Not backed up)  1970-01-01 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7 (Not backed up)  1970-01-01 00:00  test  b
  │  b
  │
  o  b173517d0 (Not backed up)  1970-01-01 00:00  test  a
     a
  ===== Screen Refresh =====
  o  f4016ed9f (Not backed up)  1970-01-01 00:00  test  d
  │  d
  │
  │ o  a82ac2b38 (Not backed up)  1970-01-01 00:00  test  c
  ├─╯  c
  │
  o  488e1b7e7 (Not backed up)  1970-01-01 00:00  test  b
  │  b
  │
  o  b173517d0 (Not backed up)  1970-01-01 00:00  test  a
     a
