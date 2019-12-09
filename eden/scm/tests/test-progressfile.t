#chg-compatible

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > progress=
  > progressfile=
  > progresstest=$TESTDIR/progresstest.py
  > [progress]
  > delay = 0
  > changedelay = 2
  > refresh = 1
  > assume-tty = true
  > statefile = $TESTTMP/progressstate
  > statefileappend = true
  > fakedpid = 42
  > EOF

  $ withprogress() {
  >   "$@"
  >   cat $TESTTMP/progressstate
  >   rm -f $TESTTMP/progressstate
  > }

simple test
  $ withprogress hg progresstest 4 4
  \r (no-eol) (esc)
  progress test [============>                                          ] 1/4 04s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 03s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 02s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {}, "topics": []}

no progress with --quiet
  $ withprogress hg progresstest --quiet 4 4
  {"state": {}, "topics": []}
  {"state": {}, "topics": []}
  {"state": {}, "topics": []}
  {"state": {}, "topics": []}
  {"state": {}, "topics": []}

progress output suppressed by setting the null renderer
  $ withprogress hg progresstest --config progress.renderer=none 4 4
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {}, "topics": []}

test nested short-lived topics (which shouldn't display with changedelay)
  $ withprogress hg progresstest --nested 4 4
  \r (no-eol) (esc)
  progress test [                                                           ] 0/4\r (no-eol) (esc)
  progress test [                                                           ] 0/4\r (no-eol) (esc)
  progress test [                                                           ] 0/4\r (no-eol) (esc)
  progress test [============>                                          ] 1/4 13s\r (no-eol) (esc)
  progress test [============>                                          ] 1/4 16s\r (no-eol) (esc)
  progress test [============>                                          ] 1/4 19s\r (no-eol) (esc)
  progress test [============>                                          ] 1/4 22s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 09s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 10s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 11s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 12s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 05s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 06s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 13, "estimate_str": "13s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 16, "estimate_str": "16s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 19, "estimate_str": "19s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 22, "estimate_str": "22s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 9, "estimate_str": "09s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 11, "estimate_str": "11s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 6, "estimate_str": "06s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {}, "topics": []}

test nested long-lived topics
  $ withprogress hg progresstest --nested 8 8
  \r (no-eol) (esc)
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 29s\r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 36s\r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 43s\r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 50s\r (no-eol) (esc)
  progress test [============>                                          ] 2/8 25s\r (no-eol) (esc)
  progress test [============>                                          ] 2/8 28s\r (no-eol) (esc)
  progress test [============>                                          ] 2/8 31s\r (no-eol) (esc)
  progress test [============>                                          ] 2/8 34s\r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 21s\r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 22s\r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 24s\r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 26s\r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 17s\r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 18s\r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 19s\r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 20s\r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 15s\r (no-eol) (esc)
  nested progress [==============================>                      ] 3/5 03s\r (no-eol) (esc)
  nested progress [=========================================>           ] 4/5 02s\r (no-eol) (esc)
  nested progress [====================================================>] 5/5 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 29, "estimate_str": "29s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 36, "estimate_str": "36s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 43, "estimate_str": "43s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 50, "estimate_str": "50s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 25, "estimate_str": "25s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 28, "estimate_str": "28s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 31, "estimate_str": "31s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 34, "estimate_str": "34s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 21, "estimate_str": "21s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 22, "estimate_str": "22s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 24, "estimate_str": "24s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 26, "estimate_str": "26s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 17, "estimate_str": "17s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 18, "estimate_str": "18s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 19, "estimate_str": "19s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 20, "estimate_str": "20s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 15, "estimate_str": "15s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "nest 3", "pid": 42, "pos": 3, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 4", "pid": 42, "pos": 4, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 5", "pid": 42, "pos": 5, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {}, "topics": []}

test nested shortlived topics without changedelay
  $ withprogress hg progresstest --nested --config progress.changedelay=0 8 8
  \r (no-eol) (esc)
  progress test [                                                           ] 0/8\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 29s\r (no-eol) (esc)
  progress test [=====>                                                 ] 1/8 36s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [============>                                          ] 2/8 25s\r (no-eol) (esc)
  progress test [============>                                          ] 2/8 28s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 21s\r (no-eol) (esc)
  progress test [===================>                                   ] 3/8 22s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 17s\r (no-eol) (esc)
  progress test [==========================>                            ] 4/8 18s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  progress test [=================================>                     ] 5/8 12s\r (no-eol) (esc)
  nested progress [=========>                                           ] 1/5 05s\r (no-eol) (esc)
  nested progress [====================>                                ] 2/5 04s\r (no-eol) (esc)
  nested progress [==============================>                      ] 3/5 03s\r (no-eol) (esc)
  nested progress [=========================================>           ] 4/5 02s\r (no-eol) (esc)
  nested progress [====================================================>] 5/5 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  progress test [========================================>              ] 6/8 10s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  progress test [===============================================>       ] 7/8 05s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  progress test [======================================================>] 8/8 01s\r (no-eol) (esc)
  nested progress [=========================>                           ] 1/2 02s\r (no-eol) (esc)
  nested progress [====================================================>] 2/2 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  \r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 29, "estimate_str": "29s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 36, "estimate_str": "36s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 1", "pid": 42, "pos": 1, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 25, "estimate_str": "25s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 28, "estimate_str": "28s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 2", "pid": 42, "pos": 2, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 21, "estimate_str": "21s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 22, "estimate_str": "22s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 3", "pid": 42, "pos": 3, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 3", "pid": 42, "pos": 3, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 17, "estimate_str": "17s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 18, "estimate_str": "18s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 4", "pid": 42, "pos": 4, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 4", "pid": 42, "pos": 4, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "nest 3", "pid": 42, "pos": 3, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 4", "pid": 42, "pos": 4, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 5", "pid": 42, "pos": 5, "speed_str": "1 per sec", "topic": "nested progress", "total": 5, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 10, "estimate_str": "10s", "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 6", "pid": 42, "pos": 6, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 6", "pid": 42, "pos": 6, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "loop 7", "pid": 42, "pos": 7, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 7", "pid": 42, "pos": 7, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 7", "pid": 42, "pos": 7, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"nested progress": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "nest 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": null}, "progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 8", "pid": 42, "pos": 8, "speed_str": "0 cycles/sec", "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "nest 1", "pid": 42, "pos": 1, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 8", "pid": 42, "pos": 8, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {"nested progress": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "nest 2", "pid": 42, "pos": 2, "speed_str": "1 per sec", "topic": "nested progress", "total": 2, "unit": null, "units_per_sec": 1}, "progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 8", "pid": 42, "pos": 8, "speed_str": null, "topic": "progress test", "total": 8, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test", "nested progress"]}
  {"state": {}, "topics": []}

test format options
  $ withprogress hg progresstest --config progress.format='number item-3 bar' 4 4
  \r (no-eol) (esc)
  1/4 p 1 [================>                                                    ]\r (no-eol) (esc)
  2/4 p 2 [=================================>                                   ]\r (no-eol) (esc)
  3/4 p 3 [==================================================>                  ]\r (no-eol) (esc)
  4/4 p 4 [====================================================================>]\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {}, "topics": []}

test format options and indeterminate progress
  $ withprogress hg progresstest --config progress.format='number item bar' -- 4 -1
  \r (no-eol) (esc)
  1 loop 1               [ <=>                                                  ]\r (no-eol) (esc)
  2 loop 2               [  <=>                                                 ]\r (no-eol) (esc)
  3 loop 3               [   <=>                                                ]\r (no-eol) (esc)
  4 loop 4               [    <=>                                               ]\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": null, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "progress test", "total": null, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "progress test", "total": null, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "progress test", "total": null, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "progress test", "total": null, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {}, "topics": []}

test count over total
  $ withprogress hg progresstest 6 4
  \r (no-eol) (esc)
  progress test [============>                                          ] 1/4 04s\r (no-eol) (esc)
  progress test [==========================>                            ] 2/4 03s\r (no-eol) (esc)
  progress test [========================================>              ] 3/4 02s\r (no-eol) (esc)
  progress test [======================================================>] 4/4 01s\r (no-eol) (esc)
  progress test [     <=>                                                   ] 5/4\r (no-eol) (esc)
  progress test [      <=>                                                  ] 6/4\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "loop 1", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "loop 2", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "loop 3", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "loop 4", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 5", "pid": 42, "pos": 5, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {"progress test": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "loop 6", "pid": 42, "pos": 6, "speed_str": "1 cycles/sec", "topic": "progress test", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["progress test"]}
  {"state": {}, "topics": []}

test rendering with bytes
  $ withprogress hg bytesprogresstest
  \r (no-eol) (esc)
  bytes progress test [                                  ] 10 bytes/1.03 GB 3y28w\r (no-eol) (esc)
  bytes progress test [                                ] 250 bytes/1.03 GB 14w05d\r (no-eol) (esc)
  bytes progress test [                                 ] 999 bytes/1.03 GB 5w04d\r (no-eol) (esc)
  bytes progress test [                                ] 1000 bytes/1.03 GB 7w03d\r (no-eol) (esc)
  bytes progress test [                                   ] 1.00 KB/1.03 GB 9w00d\r (no-eol) (esc)
  bytes progress test [                                   ] 21.5 KB/1.03 GB 3d13h\r (no-eol) (esc)
  bytes progress test [                                   ] 1.00 MB/1.03 GB 2h04m\r (no-eol) (esc)
  bytes progress test [                                   ] 1.41 MB/1.03 GB 1h41m\r (no-eol) (esc)
  bytes progress test [==>                                ]  118 MB/1.03 GB 1m13s\r (no-eol) (esc)
  bytes progress test [=================>                   ]  530 MB/1.03 GB 11s\r (no-eol) (esc)
  bytes progress test [================================>    ]  954 MB/1.03 GB 02s\r (no-eol) (esc)
  bytes progress test [====================================>] 1.03 GB/1.03 GB 01s\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"bytes progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "0 bytes", "pid": 42, "pos": 0, "speed_str": null, "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": null}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 111111111, "estimate_str": "3y28w", "item": "10 bytes", "pid": 42, "pos": 10, "speed_str": "10 bytes/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 10}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 8888887, "estimate_str": "14w05d", "item": "250 bytes", "pid": 42, "pos": 250, "speed_str": "125 bytes/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 125}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 3336668, "estimate_str": "5w04d", "item": "999 bytes", "pid": 42, "pos": 999, "speed_str": "333 bytes/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 333}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 4444441, "estimate_str": "7w03d", "item": "1000 bytes", "pid": 42, "pos": 1000, "speed_str": "250 bytes/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 250}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 5425343, "estimate_str": "9w00d", "item": "1024 bytes", "pid": 42, "pos": 1024, "speed_str": "204 bytes/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 204}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 303025, "estimate_str": "3d13h", "item": "22000 bytes", "pid": 42, "pos": 22000, "speed_str": "3.58 KB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 3666}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 7411, "estimate_str": "2h04m", "item": "1048576 bytes", "pid": 42, "pos": 1048576, "speed_str": "146 KB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 149796}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 6021, "estimate_str": "1h41m", "item": "1474560 bytes", "pid": 42, "pos": 1474560, "speed_str": "180 KB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 184320}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 73, "estimate_str": "1m13s", "item": "123456789 bytes", "pid": 42, "pos": 123456789, "speed_str": "13.1 MB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 13717421}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 11, "estimate_str": "11s", "item": "555555555 bytes", "pid": 42, "pos": 555555555, "speed_str": "53.0 MB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 55555555}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "1000000000 bytes", "pid": 42, "pos": 1000000000, "speed_str": "86.7 MB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 90909090}}, "topics": ["bytes progress test"]}
  {"state": {"bytes progress test": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "1111111111 bytes", "pid": 42, "pos": 1111111111, "speed_str": "88.3 MB/sec", "topic": "bytes progress test", "total": 1111111111, "unit": "bytes", "units_per_sec": 92592592}}, "topics": ["bytes progress test"]}
  {"state": {}, "topics": []}
test immediate completion
  $ withprogress hg progresstest 0 0
  {"state": {"progress test": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "loop 0", "pid": 42, "pos": 0, "speed_str": null, "topic": "progress test", "total": 0, "unit": "cycles", "units_per_sec": null}}, "topics": ["progress test"]}

test unicode topic
  $ withprogress hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='topic number'
  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 1/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 2/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 3/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 4/4\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"\u3042\u3044\u3046\u3048": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "\u3042\u3044", "pid": 42, "pos": 0, "speed_str": null, "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "\u3042\u3044\u3046\u3048", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "\u3042\u3044", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {}, "topics": []}

test line trimming when progress topic contains multi-byte characters
  $ withprogress hg --encoding utf-8 progresstest 4 4 --unicode --config progress.width=12 --config progress.format='topic number'
  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 1/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 2/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 3/4\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 4/4\r (no-eol) (esc)
              \r (no-eol) (esc)
  {"state": {"\u3042\u3044\u3046\u3048": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "\u3042\u3044", "pid": 42, "pos": 0, "speed_str": null, "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "\u3042\u3044\u3046\u3048", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "\u3042\u3044", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {}, "topics": []}

test calculation of bar width when progress topic contains multi-byte characters
  $ withprogress hg --encoding utf-8 progresstest 4 4 --unicode --config progress.width=21 --config progress.format='topic bar'
  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 [=>       ]\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 [===>     ]\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 [=====>   ]\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88 [========>]\r (no-eol) (esc)
                       \r (no-eol) (esc)
  {"state": {"\u3042\u3044\u3046\u3048": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "\u3042\u3044", "pid": 42, "pos": 0, "speed_str": null, "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "\u3042\u3044\u3046\u3048", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "\u3042\u3044", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {}, "topics": []}

test trimming progress items with they contain multi-byte characters
  $ withprogress hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='item+6'
  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"\u3042\u3044\u3046\u3048": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "\u3042\u3044", "pid": 42, "pos": 0, "speed_str": null, "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "\u3042\u3044\u3046\u3048", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "\u3042\u3044", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {}, "topics": []}
  $ withprogress hg --encoding utf-8 progresstest 4 4 --unicode --config progress.format='item-6'
  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\r (no-eol) (esc)
  \xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84  \r (no-eol) (esc)
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\r (no-eol) (esc)
                                                                                  \r (no-eol) (esc)
  {"state": {"\u3042\u3044\u3046\u3048": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "\u3042\u3044", "pid": 42, "pos": 0, "speed_str": null, "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": null}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 1, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "\u3042\u3044\u3046\u3048", "pid": 42, "pos": 2, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 2, "estimate_str": "02s", "item": "\u3042\u3044", "pid": 42, "pos": 3, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {"\u3042\u3044\u3046\u3048": {"active": true, "estimate_sec": 1, "estimate_str": "01s", "item": "\u3042\u3044\u3046", "pid": 42, "pos": 4, "speed_str": "1 cycles/sec", "topic": "\u3042\u3044\u3046\u3048", "total": 4, "unit": "cycles", "units_per_sec": 1}}, "topics": ["\u3042\u3044\u3046\u3048"]}
  {"state": {}, "topics": []}
