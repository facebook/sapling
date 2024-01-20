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
  > k
  > j
  > q
  > EOF

  $ hg sl -i --config ui.color=debug < transcript
  ===== Screen Refresh =====
  o  [sl.highlighted|[sl.draft|f4016ed9f] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|d]
  │  [sl.desc|d]
  │  ]
  │ o  [sl.draft|a82ac2b38] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|c]
  ├─╯  [sl.desc|c]
  │
  o  [sl.draft|488e1b7e7] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|b]
  │  [sl.desc|b]
  │
  o  [sl.draft|b173517d0] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|a]
     [sl.desc|a]
  ===== Screen Refresh =====
  o  [sl.highlighted|[sl.draft|f4016ed9f] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|d]
  │  [sl.desc|d]
  │  ]
  │ o  [sl.draft|a82ac2b38] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|c]
  ├─╯  [sl.desc|c]
  │
  o  [sl.draft|488e1b7e7] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|b]
  │  [sl.desc|b]
  │
  o  [sl.draft|b173517d0] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|a]
     [sl.desc|a]
  ===== Screen Refresh =====
  o  [sl.highlighted|[sl.draft|f4016ed9f] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|d]
  │  [sl.desc|d]
  │  ]
  │ o  [sl.draft|a82ac2b38] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|c]
  ├─╯  [sl.desc|c]
  │
  o  [sl.draft|488e1b7e7] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|b]
  │  [sl.desc|b]
  │
  o  [sl.draft|b173517d0] [sl.backupfail|(Not backed up)]  [sl.date|1970-01-01 00:00]  [sl.user|test]  [sl.book|a]
     [sl.desc|a]
