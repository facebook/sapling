#chg-compatible
#require bash

  $ enable progress
  $ setconfig extensions.progresstest="$TESTDIR/progresstest.py"
  $ setconfig progress.delay=0 progress.assume-tty=true progress.lockstep=True progress.renderer=rust:simple

simple test
  $ hg progresstest 4 4
     Progress test  [>              ]  0/4 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [===>           ]  1/4 cycles  loop 1\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [=======>       ]  2/4 cycles  loop 2\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [===========>   ]  3/4 cycles  loop 3\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [===============]  4/4 cycles  loop 4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test nested topics
  $ hg progresstest --nested 2 2
     Progress test  [>              ]  0/2 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  [J   Progress test  [>              ]  0/2 cycles  loop 0
   Nested progress  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [>              ]  0/2 cycles  loop 0
   Nested progress  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [>              ]  0/2 cycles  loop 0
   Nested progress  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [=======>       ]  1/2 cycles  loop 1
   Nested progress  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [=======>       ]  1/2 cycles  loop 1
   Nested progress  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [=======>       ]  1/2 cycles  loop 1
   Nested progress  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [=======>       ]  1/2 cycles  loop 1
   Nested progress  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [===============]  2/2 cycles  loop 2
   Nested progress  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [===============]  2/2 cycles  loop 2
   Nested progress  [>              ]  0/2   nest 0\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [===============]  2/2 cycles  loop 2
   Nested progress  [=======>       ]  1/2   nest 1\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  [J   Progress test  [===============]  2/2 cycles  loop 2
   Nested progress  [===============]  2/2   nest 2\r (no-eol) (esc)
  \x1b[1A\r (no-eol) (esc)
  \x1b[J (no-eol) (esc)


test count over total
  $ hg progresstest 4 2
     Progress test  [>              ]  0/2 cycles  loop 0\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [=======>       ]  1/2 cycles  loop 1\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [===============]  2/2 cycles  loop 2\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [<=>            ]  3/2 cycles  loop 3\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J   Progress test  [<=>            ]  4/2 cycles  loop 4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test rendering with bytes
  $ hg bytesprogresstest
    Bytes progress  [>              ]  0B/1111MB  0 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  10B/1111MB  10 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  250B/1111MB  250 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  999B/1111MB  999 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  1000B/1111MB  1000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  1024B/1111MB  1024 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  22KB/1111MB  22000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  1048KB/1111MB  1048576 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [>              ]  1474KB/1111MB  1474560 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [=>             ]  123MB/1111MB  123456789 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [=======>       ]  555MB/1111MB  555555555 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [=============> ]  1000MB/1111MB  1000000000 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J  Bytes progress  [===============]  1111MB/1111MB  1111111111 bytes\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test unicode topic
  $ hg --encoding utf-8 progresstest 4 4 --unicode
              あいうえ  [>              ]  0/4 cycles  あい\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J            あいうえ  [===>           ]  1/4 cycles  あいう\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J            あいうえ  [=======>       ]  2/4 cycles  あいうえ\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J            あいうえ  [===========>   ]  3/4 cycles  あい\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J            あいうえ  [===============]  4/4 cycles  あいう\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)

test iter adapter
  $ hg iterprogresstest
           Numbers  [===>           ]  1/4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J         Numbers  [=======>       ]  2/4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J         Numbers  [===========>   ]  3/4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J         Numbers  [===============]  4/4\r (no-eol) (esc)
  \r (no-eol) (esc)
  \x1b[J (no-eol) (esc)
