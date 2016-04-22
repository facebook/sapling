#require no-pure

A script to generate nasty diff worst-case scenarios:

  $ cat > s.py <<EOF
  > import random
  > for x in xrange(100000):
  >     print
  >     if random.randint(0, 100) >= 50:
  >         x += 1
  >     print hex(x)
  > EOF

  $ hg init a
  $ cd a

Check in a big file:

  $ python ../s.py > a
  $ hg ci -qAm0

Modify it:

  $ python ../s.py > a

Time a check-in, should never take more than 10 seconds user time:

  $ hg ci --time -m1
  time: real .* secs .user [0-9][.].* sys .* (re)
