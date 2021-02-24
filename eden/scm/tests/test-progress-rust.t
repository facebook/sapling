#chg-compatible

  $ enable progress
  $ setconfig extensions.rustprogresstest="$TESTDIR/rustprogresstest.py"
  $ setconfig progress.delay=0 progress.changedelay=2 progress.refresh=1 progress.assume-tty=true

Test creating a progress spinner.
  $ hg rustspinnertest 5
  loop 1 [ <=>                                                              ] 1.0\r (no-eol) (esc)
  \x1b[Kloop 2 [  <=>                                                             ] 2.0\r (no-eol) (esc)
  \x1b[Kloop 3 [   <=>                                                            ] 3.0\r (no-eol) (esc)
  \x1b[Kloop 4 [    <=>                                                           ] 4.0\r (no-eol) (esc)
  \x1b[Kloop 5 [     <=>                                                          ] 5.0\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

Test creating a progress bar.
  $ hg rustprogresstest 6 6
  loop 1 [===================>                                          ] 1/3 03s\r (no-eol) (esc)
  \x1b[Kloop 2 [========================================>                     ] 2/3 02s\r (no-eol) (esc)
  \x1b[Kloop 3 [=============================================================>] 3/3 01s\r (no-eol) (esc)
  \x1b[Kloop 4 [========================================>                     ] 4/6 03s\r (no-eol) (esc)
  \x1b[Kloop 5 [==================================================>           ] 5/6 02s\r (no-eol) (esc)
  \x1b[Kloop 6 [=============================================================>] 6/6 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

Test creating an indeterminate (i.e., no total) progress bar.
  $ hg rustprogresstest -- 5 -1
  loop 1 [ <=>                                                                ] 1\r (no-eol) (esc)
  \x1b[Kloop 2 [  <=>                                                               ] 2\r (no-eol) (esc)
  \x1b[Kloop 3 [   <=>                                                              ] 3\r (no-eol) (esc)
  \x1b[Kloop 4 [    <=>                                                             ] 4\r (no-eol) (esc)
  \x1b[Kloop 5 [     <=>                                                            ] 5\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

Test formatting the number as bytes.
Note that the first iteration is not rendered because the first value is 0.
  $ hg rustbytesprogresstest
  loop 2 [                                               ] 10 bytes/1.03 GB 3y28w\r (no-eol) (esc)
  \x1b[Kloop 3 [                                             ] 250 bytes/1.03 GB 14w05d\r (no-eol) (esc)
  \x1b[Kloop 4 [                                              ] 999 bytes/1.03 GB 5w04d\r (no-eol) (esc)
  \x1b[Kloop 5 [                                             ] 1000 bytes/1.03 GB 7w03d\r (no-eol) (esc)
  \x1b[Kloop 6 [                                                ] 1.00 KB/1.03 GB 9w00d\r (no-eol) (esc)
  \x1b[Kloop 7 [                                                ] 21.5 KB/1.03 GB 3d13h\r (no-eol) (esc)
  \x1b[Kloop 8 [                                                ] 1.00 MB/1.03 GB 2h04m\r (no-eol) (esc)
  \x1b[Kloop 9 [                                                ] 1.41 MB/1.03 GB 1h41m\r (no-eol) (esc)
  \x1b[Kloop 10 [====>                                          ]  118 MB/1.03 GB 1m13s\r (no-eol) (esc)
  \x1b[Kloop 11 [=======================>                         ]  530 MB/1.03 GB 11s\r (no-eol) (esc)
  \x1b[Kloop 12 [===========================================>     ]  954 MB/1.03 GB 02s\r (no-eol) (esc)
  \x1b[Kloop 13 [================================================>] 1.03 GB/1.03 GB 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)
