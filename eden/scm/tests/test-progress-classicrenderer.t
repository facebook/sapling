#chg-compatible

  $ enable progress
  $ setconfig extensions.progresstest="$TESTDIR/progresstest.py"
  $ setconfig progress.delay=0 progress.changedelay=2 progress.refresh=1 progress.assume-tty=true

simple test
  $ hg progresstest 4 4
  progress test [============>                                          ] 1/4 04s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 03s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 02s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

no progress with --quiet
  $ hg progresstest --quiet 4 4

test nested short-lived topics (which shouldn't display with changedelay)
  $ hg progresstest --nested 4 4
  progress test [                                                           ] 0/4\r (no-eol) (esc)
  \x1b[Kprogress test [                                                           ] 0/4\r (no-eol) (esc)
  \x1b[Kprogress test [                                                           ] 0/4\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 1/4 13s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 1/4 16s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 1/4 19s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 1/4 22s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 09s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 10s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 11s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 12s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 06s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test nested long-lived topics
  $ hg progresstest --nested 8 8
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  \x1b[Kprogress test [                                                           ] 0/8\r (no-eol) (esc)
  \x1b[Kprogress test [                                                           ] 0/8\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 29s\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 36s\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 43s\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 50s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 25s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 28s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 31s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 34s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 21s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 22s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 24s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 26s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 17s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 18s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 19s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 20s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 15s\r (no-eol) (esc)
  \x1b[Knested progress [==============================>                      ] 3/5 03s\r (no-eol) (esc)
  \x1b[Knested progress [=========================================>           ] 4/5 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 5/5 01s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test nested shortlived topics without changedelay
  $ hg progresstest --nested --config progress.changedelay=0 8 8
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 29s\r (no-eol) (esc)
  \x1b[Kprogress test [=====>                                                 ] 1/8 36s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 25s\r (no-eol) (esc)
  \x1b[Kprogress test [============>                                          ] 2/8 28s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 21s\r (no-eol) (esc)
  \x1b[Kprogress test [===================>                                   ] 3/8 22s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 17s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 4/8 18s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  \x1b[Kprogress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  \x1b[Knested progress [=========>                                           ] 1/5 05s\r (no-eol) (esc)
  \x1b[Knested progress [====================>                                ] 2/5 04s\r (no-eol) (esc)
  \x1b[Knested progress [==============================>                      ] 3/5 03s\r (no-eol) (esc)
  \x1b[Knested progress [=========================================>           ] 4/5 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 5/5 01s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Kprogress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  \x1b[Knested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  \x1b[Knested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test format options
  $ hg progresstest --config progress.format='number item-3 bar' 4 4
  1/4 p 1 [================>                                                    ]\r (no-eol) (esc)
  \x1b[K2/4 p 2 [=================================>                                   ]\r (no-eol) (esc)
  \x1b[K3/4 p 3 [==================================================>                  ]\r (no-eol) (esc)
  \x1b[K4/4 p 4 [====================================================================>]\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test format options and indeterminate progress
  $ hg progresstest --config progress.format='number item bar' -- 4 -1
  1 loop 1               [ <=>                                                  ]\r (no-eol) (esc)
  \x1b[K2 loop 2               [  <=>                                                 ]\r (no-eol) (esc)
  \x1b[K3 loop 3               [   <=>                                                ]\r (no-eol) (esc)
  \x1b[K4 loop 4               [    <=>                                               ]\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test count over total
  $ hg progresstest 6 4
  progress test [============>                                          ] 1/4 04s\r (no-eol) (esc)
  \x1b[Kprogress test [==========================>                            ] 2/4 03s\r (no-eol) (esc)
  \x1b[Kprogress test [========================================>              ] 3/4 02s\r (no-eol) (esc)
  \x1b[Kprogress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  \x1b[Kprogress test [     <=>                                                   ] 5/4\r (no-eol) (esc)
  \x1b[Kprogress test [      <=>                                                  ] 6/4\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test rendering with bytes
  $ hg bytesprogresstest
  bytes progress test [                                  ] 10 bytes/1.03 GB 3y28w\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                ] 250 bytes/1.03 GB 14w05d\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                 ] 999 bytes/1.03 GB 5w04d\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                ] 1000 bytes/1.03 GB 7w03d\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                   ] 1.00 KB/1.03 GB 9w00d\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                   ] 21.5 KB/1.03 GB 3d13h\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                   ] 1.00 MB/1.03 GB 2h04m\r (no-eol) (esc)
  \x1b[Kbytes progress test [                                   ] 1.41 MB/1.03 GB 1h41m\r (no-eol) (esc)
  \x1b[Kbytes progress test [==>                                ]  118 MB/1.03 GB 1m13s\r (no-eol) (esc)
  \x1b[Kbytes progress test [=================>                   ]  530 MB/1.03 GB 11s\r (no-eol) (esc)
  \x1b[Kbytes progress test [================================>    ]  954 MB/1.03 GB 02s\r (no-eol) (esc)
  \x1b[Kbytes progress test [====================================>] 1.03 GB/1.03 GB 01s\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)
test immediate completion
  $ hg progresstest 0 0

test unicode topic
  $ hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='topic number'
  あいうえ 1/4\r (no-eol) (esc)
  \x1b[Kあいうえ 2/4\r (no-eol) (esc)
  \x1b[Kあいうえ 3/4\r (no-eol) (esc)
  \x1b[Kあいうえ 4/4\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test line trimming when progress topic contains multi-byte characters
  $ hg --encoding utf-8 progresstest 4 4 --unicode --config progress.width=12 --config progress.format='topic number'
  あいうえ 1/4\r (no-eol) (esc)
  \x1b[Kあいうえ 2/4\r (no-eol) (esc)
  \x1b[Kあいうえ 3/4\r (no-eol) (esc)
  \x1b[Kあいうえ 4/4\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test calculation of bar width when progress topic contains multi-byte characters
  $ hg --encoding utf-8 progresstest 4 4 --unicode --config progress.width=21 --config progress.format='topic bar'
  あいうえ [=>       ]\r (no-eol) (esc)
  \x1b[Kあいうえ [===>     ]\r (no-eol) (esc)
  \x1b[Kあいうえ [=====>   ]\r (no-eol) (esc)
  \x1b[Kあいうえ [========>]\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)

test trimming progress items with they contain multi-byte characters
  $ hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='item+6'
  あいう\r (no-eol) (esc)
  \x1b[Kあいう\r (no-eol) (esc)
  \x1b[Kあい\r (no-eol) (esc)
  \x1b[Kあいう\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)
  $ hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='item-6'
  あいう\r (no-eol) (esc)
  \x1b[Kいうえ\r (no-eol) (esc)
  \x1b[Kあい\r (no-eol) (esc)
  \x1b[Kあいう\r (no-eol) (esc)
  \x1b[K (no-eol) (esc)
