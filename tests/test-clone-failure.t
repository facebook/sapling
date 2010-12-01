No local source

  $ hg clone a b
  abort: repository a not found!
  [255]

No remote source

  $ hg clone http://127.0.0.1:3121/a b
  abort: error: Connection refused
  [255]
  $ rm -rf b # work around bug with http clone

Inaccessible source

  $ mkdir a
  $ chmod 000 a
  $ hg clone a b
  abort: repository a not found!
  [255]

Inaccessible destination

  $ mkdir b
  $ cd b
  $ hg init
  $ hg clone . ../a
  abort: Permission denied: ../a
  [255]
  $ cd ..
  $ chmod 700 a
  $ rm -r a b

Source of wrong type

  $ if "$TESTDIR/hghave" -q fifo; then
  >     mkfifo a
  >     hg clone a b
  >     rm a
  > else
  >     echo "abort: repository a not found!"
  > fi
  abort: repository a not found!

Default destination, same directory

  $ mkdir q
  $ cd q
  $ hg init
  $ cd ..
  $ hg clone q
  destination directory: q
  abort: destination 'q' is not empty
  [255]

destination directory not empty

  $ mkdir a 
  $ echo stuff > a/a
  $ hg clone q a
  abort: destination 'a' is not empty
  [255]

leave existing directory in place after clone failure

  $ hg init c
  $ cd c
  $ echo c > c
  $ hg commit -A -m test
  adding c
  $ chmod -rx .hg/store/data
  $ cd ..
  $ mkdir d
  $ hg clone c d 2> err
  [255]
  $ test -d d
  $ test -d d/.hg
  [1]

reenable perm to allow deletion

  $ chmod +rx c/.hg/store/data
