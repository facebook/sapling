
  $ enable amend morestatus
  $ setconfig tweakdefaults.showupdated=true

Create a commit with a Differential Revision line:

  $ newclientrepo
  $ echo a > a
  $ sl add a
  $ sl commit -m "$(printf 'first commit\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"

Agent: amend -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "new message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (use `jf template` to modify commit message fields or use 'jf unlink' to remove the associated phabricator diff)
  [255]

Agent: metaedit -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl metaedit -m "another message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (use `jf template` to modify commit message fields or use 'jf unlink' to remove the associated phabricator diff)
  [255]

Interactive user choosing No should abort (amend):

  $ sl amend --config ui.interactive=true -m "drop diff number" <<EOF
  > n
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  n
  abort: aborted by user
  [255]

Interactive user choosing No should abort (metaedit):

  $ sl metaedit --config ui.interactive=true -m "drop diff number" <<EOF
  > n
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  n
  abort: aborted by user
  [255]

Interactive user choosing Yes should proceed (amend):

  $ sl amend --config ui.interactive=true -m "drop diff number via amend" <<EOF
  > y
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  5a4d097da8bb -> 78d316c8be37 "drop diff number via amend"

Restore diff number for next test:

  $ sl amend -m "$(printf 'restored\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  78d316c8be37 -> 9ea48f174b42 "restored"

Interactive user choosing Yes should proceed (metaedit):

  $ sl metaedit --config ui.interactive=true -m "drop diff number via metaedit" <<EOF
  > y
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  9ea48f174b42 -> 8f89e739bba4 "drop diff number via metaedit"

Non-interactive defaults to Yes (amend):

Restore diff number:

  $ sl amend -m "$(printf 'restored again\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  8f89e739bba4 -> f6c732d5b9eb "restored again"

  $ sl amend -m "non-interactive drop"
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  f6c732d5b9eb -> cad5328c7ead "non-interactive drop"

Amend -m preserving Differential Revision should succeed:

  $ sl amend -m "$(printf 'new message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  cad5328c7ead -> 334772907fae "new message"

Metaedit -m preserving Differential Revision should succeed:

  $ sl metaedit -m "$(printf 'another message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  334772907fae -> ded7f3602a29 "another message"

Config override should allow dropping:

  $ sl amend --config fbcodereview.allow-diff-revision-drop=true -m "message without diff number"
  ded7f3602a29 -> 1704a68c6e26 "message without diff number"

Restore diff number before testing amend without -m:

  $ sl amend -m "$(printf 'restored for amend test\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  1704a68c6e26 -> 9d95226810a0 "restored for amend test"
  $ echo b > b
  $ sl add b
  $ sl amend
  9d95226810a0 -> f90ed971e155 "restored for amend test"

New commit without Differential Revision should not be affected:

  $ echo c > c
  $ sl add c
  $ sl commit -m "plain commit without diff number"
  $ sl amend -m "updated plain commit"
  3e03e53c536f -> cc642c658d2a "updated plain commit"
