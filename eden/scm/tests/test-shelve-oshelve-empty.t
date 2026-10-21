
#require no-eden

A corrupt <name>.oshelve file (empty, bad hex, no key=value, or an empty node)
must not break {shelvename} or shelved() for every other shelve. This test
asserts the INCORRECT behavior: every such file crashes sl log.

  $ enable shelve
  $ newclientrepo
  $ echo a > a
  $ hg commit -Aqm a
  $ echo b > b
  $ hg add b
  $ hg shelve -q -n broken
  $ echo c > c
  $ hg add c
  $ hg shelve -q -n good
  $ hg log -r . -T '{shelvename}\n'
  
  $ hg log -r 'shelved()' -T '{shelvename}\n'
  broken
  good

Empty .oshelve (the production case):

  $ : > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n' 2>&1 | grep -E '^KeyError'
  KeyError: 'node'
  $ hg log -r 'shelved()' -T '{shelvename}\n' 2>&1 | grep -E '^KeyError'
  KeyError: 'node'

Node value that is not hex:

  $ echo 'node=zz' > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n' 2>&1 | grep -E '^TypeError'
  TypeError: Non-hexadecimal digit found
  $ hg log -r 'shelved()' -T '{shelvename}\n' 2>&1 | grep -E '^TypeError'
  TypeError: Non-hexadecimal digit found

Line without '=':

  $ echo 'garbage' > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n' 2>&1 | grep -E '^sapling.error.CorruptedState'
  sapling.error.CorruptedState: dictionary update sequence element #0 has length 1; 2 is required
  $ hg log -r 'shelved()' -T '{shelvename}\n' 2>&1 | grep -E '^sapling.error.CorruptedState'
  sapling.error.CorruptedState: dictionary update sequence element #0 has length 1; 2 is required
