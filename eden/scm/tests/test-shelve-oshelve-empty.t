
#require no-eden

A corrupt <name>.oshelve file (empty, bad hex, no key=value, or an empty node)
must not break {shelvename} or shelved() for every other shelve: the broken
shelve is skipped and the healthy one is still reported.

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
  $ hg log -r . -T '{shelvename}\n'
  
  $ hg log -r 'shelved()' -T '{shelvename}\n'
  good

Node value that is not hex:

  $ echo 'node=zz' > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n'
  
  $ hg log -r 'shelved()' -T '{shelvename}\n'
  good

Line without '=':

  $ echo 'garbage' > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n'
  
  $ hg log -r 'shelved()' -T '{shelvename}\n'
  good

Empty node value:

  $ echo 'node=' > .sl/shelved/broken.oshelve
  $ hg log -r . -T '{shelvename}\n'
  
  $ hg log -r 'shelved()' -T '{shelvename}\n'
  good

The healthy shelve still unshelves:

  $ hg unshelve good
  $ hg status
  A c
