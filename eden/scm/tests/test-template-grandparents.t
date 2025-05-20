  $ newrepo

  $ hg debugdrawdag <<'EOS'
  >   f g
  >   |/
  > i e d
  > | |/
  > h c
  > |/
  > b
  > |
  > a
  > EOS

Log the whole revset where direct parents are present except for the root. Add --debug to confirm whether the subdag is computed.

  $ hg log -T '{desc} parents: [{parents % "{desc}"}] grandparents: [{grandparents % "{desc}"}]\n' --debug
  commands.log(): finished computing subdag
  g parents: [e] grandparents: []
  f parents: [e] grandparents: []
  i parents: [h] grandparents: []
  e parents: [c] grandparents: []
  d parents: [c] grandparents: []
  h parents: [b] grandparents: []
  c parents: [b] grandparents: []
  b parents: [a] grandparents: []
  a parents: [] grandparents: []

Specify revs where direct parents are NOT always present.

  $ hg log -r 'a + h + c + d + g' -T '{desc} parents: [{parents % "{desc}"}] grandparents: [{grandparents % "{desc}"}]\n' --debug
  commands.log(): finished computing subdag
  a parents: [] grandparents: []
  h parents: [b] grandparents: [a]
  c parents: [b] grandparents: [a]
  d parents: [c] grandparents: []
  g parents: [e] grandparents: [c]

Test log --graph which doesn't involve subdag computation.

  $ hg log --graph -r 'a + h + c + d + g' -T '{desc} parents: [{parents % "{desc}"}] grandparents: [{grandparents % "{desc}"}]\n' --debug
  o  g parents: [e] grandparents: [c]
  ╷
  ╷ o  d parents: [c] grandparents: []
  ╭─╯
  │ o  h parents: [b] grandparents: [a]
  │ ╷
  o ╷  c parents: [b] grandparents: [a]
  ├─╯
  o  a parents: [] grandparents: []

  >>> assert 'computing subdag' not in _

Confirm that the subdag is only computed when "grandparents" is requested in the template.

  $ hg log -T '{desc} parents: [{parents % "{desc}"}]\n' --debug
  g parents: [e]
  f parents: [e]
  i parents: [h]
  e parents: [c]
  d parents: [c]
  h parents: [b]
  c parents: [b]
  b parents: [a]
  a parents: []

  >>> assert 'computing subdag' not in _
