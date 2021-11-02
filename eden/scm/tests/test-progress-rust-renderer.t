#chg-compatible

  $ enable progress
  $ setconfig extensions.progresstest="$TESTDIR/progresstest.py"
  $ setconfig progress.delay=0 progress.assume-tty=true progress.lockstep=True progress.renderer=rust:simple

simple test
  $ hg progresstest 4 4
      Progress  [>              ]  0/4 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [===>           ]  1/4 cycles  loop 1\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [=======>       ]  2/4 cycles  loop 2\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [===========>   ]  3/4 cycles  loop 3\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [===============]  4/4 cycles  loop 4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test nested topics
  $ hg progresstest --nested 2 2
      Progress  [>              ]  0/2 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  [J    Progress  [>              ]  0/2 cycles  loop 0
        Nested  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [>              ]  0/2 cycles  loop 0
        Nested  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [>              ]  0/2 cycles  loop 0
        Nested  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [=======>       ]  1/2 cycles  loop 1
        Nested  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [=======>       ]  1/2 cycles  loop 1
        Nested  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [=======>       ]  1/2 cycles  loop 1
        Nested  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [=======>       ]  1/2 cycles  loop 1
        Nested  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [===============]  2/2 cycles  loop 2
        Nested  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [===============]  2/2 cycles  loop 2
        Nested  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [===============]  2/2 cycles  loop 2
        Nested  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J    Progress  [===============]  2/2 cycles  loop 2
        Nested  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  \x1b[J (no-eol) (esc)


test count over total
  $ hg progresstest 4 2
      Progress  [>              ]  0/2 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [=======>       ]  1/2 cycles  loop 1\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [===============]  2/2 cycles  loop 2\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [<=>            ]  3/2 cycles  loop 3\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J    Progress  [<=>            ]  4/2 cycles  loop 4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test rendering with bytes
  $ hg bytesprogresstest
         Bytes  [>              ]  0B/1111MB  0 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  10B/1111MB  10 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  250B/1111MB  250 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  999B/1111MB  999 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  1000B/1111MB  1000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  1024B/1111MB  1024 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  22KB/1111MB  22000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  1048KB/1111MB  1048576 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [>              ]  1474KB/1111MB  1474560 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [=>             ]  123MB/1111MB  123456789 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [=======>       ]  555MB/1111MB  555555555 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [=============> ]  1000MB/1111MB  1000000000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J       Bytes  [===============]  1111MB/1111MB  1111111111 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test unicode topic
  $ hg --encoding utf-8 progresstest 4 4 --unicode
          あいうえ  [>              ]  0/4 cycles  あい\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J        あいうえ  [===>           ]  1/4 cycles  あいう\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J        あいうえ  [=======>       ]  2/4 cycles  あいうえ\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J        あいうえ  [===========>   ]  3/4 cycles  あい\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J        あいうえ  [===============]  4/4 cycles  あいう\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)
