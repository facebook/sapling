#chg-compatible
#debugruntest-compatible

A script to generate nasty diff worst-case scenarios:

  $ cat > s.py <<EOF
  > import random, sys
  > random.seed(int(sys.argv[-1]))
  > for x in range(100000):
  >     print
  >     if random.randint(0, 100) >= 50:
  >         x += 1
  >     print(hex(x))
  > EOF

  $ hg init a
  $ cd a

Check in a big file:

  $ hg debugpython -- ../s.py 1 > a
  $ hg ci -qAm0

Modify it:

  $ hg debugpython -- ../s.py 2 > a

Time a check-in, should never take more than 10 seconds user time:

  $ hg ci --time -m1
  time: real .* secs .user [0-9][.].* sys .* (re)
