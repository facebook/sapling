#chg-compatible
#require bash

  $ enable progress
  $ setconfig extensions.progresstest="$TESTDIR/progresstest.py"
  $ setconfig progress.delay=0 progress.assume-tty=true progress.lockstep=True progress.renderer=rust:simple

simple test
  $ hg progresstest 4 4
   (clear) (no-eol)
     Progress test  [>              ]  0/4 cycles  loop 0\x1b[55D (clear) (no-eol) (esc)
     Progress test  [===>           ]  1/4 cycles  loop 1\x1b[55D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  2/4 cycles  loop 2\x1b[55D (clear) (no-eol) (esc)
     Progress test  [===========>   ]  3/4 cycles  loop 3\x1b[55D (clear) (no-eol) (esc)
     Progress test  [===============]  4/4 cycles  loop 4\x1b[55D (clear) (no-eol) (esc)

test nested topics
  $ hg progresstest --nested 2 2
   (clear) (no-eol)
     Progress test  [>              ]  0/2 cycles  loop 0\x1b[55D (clear) (no-eol) (esc)
     Progress test  [>              ]  0/2 cycles  loop 0\r (esc)
   Nested progress  [>              ]  0/2   nest 0\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [>              ]  0/2 cycles  loop 0\r (esc)
   Nested progress  [=======>       ]  1/2   nest 1\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [>              ]  0/2 cycles  loop 0\r (esc)
   Nested progress  [===============]  2/2   nest 2\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  1/2 cycles  loop 1\r (esc)
   Nested progress  [===============]  2/2   nest 2\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  1/2 cycles  loop 1\r (esc)
   Nested progress  [>              ]  0/2   nest 0\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  1/2 cycles  loop 1\r (esc)
   Nested progress  [=======>       ]  1/2   nest 1\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  1/2 cycles  loop 1\r (esc)
   Nested progress  [===============]  2/2   nest 2\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [===============]  2/2 cycles  loop 2\r (esc)
   Nested progress  [===============]  2/2   nest 2\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [===============]  2/2 cycles  loop 2\r (esc)
   Nested progress  [>              ]  0/2   nest 0\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [===============]  2/2 cycles  loop 2\r (esc)
   Nested progress  [=======>       ]  1/2   nest 1\x1b[A\x1b[49D (clear) (no-eol) (esc)
     Progress test  [===============]  2/2 cycles  loop 2\r (esc)
   Nested progress  [===============]  2/2   nest 2\x1b[A\x1b[49D (clear) (no-eol) (esc)


test count over total
  $ hg progresstest 4 2
   (clear) (no-eol)
     Progress test  [>              ]  0/2 cycles  loop 0\x1b[55D (clear) (no-eol) (esc)
     Progress test  [=======>       ]  1/2 cycles  loop 1\x1b[55D (clear) (no-eol) (esc)
     Progress test  [===============]  2/2 cycles  loop 2\x1b[55D (clear) (no-eol) (esc)
     Progress test  [<=>            ]  3/2 cycles  loop 3\x1b[55D (clear) (no-eol) (esc)
     Progress test  [<=>            ]  4/2 cycles  loop 4\x1b[55D (clear) (no-eol) (esc)

test rendering with bytes
  $ hg bytesprogresstest
   (clear) (no-eol)
    Bytes progress  [>              ]  0B/1111MB  0 bytes\x1b[55D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  10B/1111MB  10 bytes\x1b[57D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  250B/1111MB  250 bytes\x1b[59D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  999B/1111MB  999 bytes\x1b[59D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  1000B/1111MB  1000 bytes\x1b[61D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  1024B/1111MB  1024 bytes\x1b[61D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  22KB/1111MB  22000 bytes\x1b[61D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  1048KB/1111MB  1048576 bytes\x1b[65D (clear) (no-eol) (esc)
    Bytes progress  [>              ]  1474KB/1111MB  1474560 bytes\x1b[65D (clear) (no-eol) (esc)
    Bytes progress  [=>             ]  123MB/1111MB  123456789 bytes\x1b[66D (clear) (no-eol) (esc)
    Bytes progress  [=======>       ]  555MB/1111MB  555555555 bytes\x1b[66D (clear) (no-eol) (esc)
    Bytes progress  [=============> ]  1000MB/1111MB  1000000000 bytes\x1b[68D (clear) (no-eol) (esc)
    Bytes progress  [===============]  1111MB/1111MB  1111111111 bytes\x1b[68D (clear) (no-eol) (esc)

test unicode topic
  $ hg --encoding utf-8 progresstest 4 4 --unicode
   (clear) (no-eol)
              あいうえ  [>              ]  0/4 cycles  あい\x1b[57D (clear) (no-eol) (esc)
              あいうえ  [===>           ]  1/4 cycles  あいう\x1b[59D (clear) (no-eol) (esc)
              あいうえ  [=======>       ]  2/4 cycles  あいうえ\x1b[61D (clear) (no-eol) (esc)
              あいうえ  [===========>   ]  3/4 cycles  あい\x1b[57D (clear) (no-eol) (esc)
              あいうえ  [===============]  4/4 cycles  あいう\x1b[59D (clear) (no-eol) (esc)

test iter adapter
  $ hg iterprogresstest
   (clear) (no-eol)
           Numbers  [===>           ]  1/4 \x1b[41D (clear) (no-eol) (esc)
           Numbers  [=======>       ]  2/4 \x1b[41D (clear) (no-eol) (esc)
           Numbers  [===========>   ]  3/4 \x1b[41D (clear) (no-eol) (esc)
           Numbers  [===============]  4/4 \x1b[41D (clear) (no-eol) (esc)
