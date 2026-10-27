#require no-eden

Look up revisions on an eager remote through its peer.

  $ configure modern
  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ sl push -q -r $B --to master --create

By bookmark, full hash and hash prefix:

  $ sl id -r master test:e1
  112478962961
  $ sl id -r $A test:e1
  426bada5c675
  $ sl id -r 426bada5 test:e1
  426bada5c675

Unknown names, including names that are not hex or longer than a hash:

  $ sl id -r unknown test:e1
  abort: unknown revision 'unknown'!
  [255]
  $ sl id -r 426bada5c67598ca65036d57d9e4b64b0c1ce7a0a test:e1
  abort: unknown revision '426bada5c67598ca65036d57d9e4b64b0c1ce7a0a'!
  [255]

Known:

  $ sl debugknown test:e1 $A 1111111111111111111111111111111111111111
  10
