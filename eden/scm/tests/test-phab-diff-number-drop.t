
  $ enable amend fbcodereview histedit morestatus rebase
  $ setconfig tweakdefaults.showupdated=true

Create a commit with a Differential Revision line:

  $ newclientrepo
  $ echo a > a
  $ sl add a
  $ HGPLAIN=1 sl commit -m "$(printf 'first commit\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"

Agent: amend -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl amend --config devel.print-metrics=commit.baddiffid -m "new message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  commit.baddiffid.agent_rejected: 1
  [255]

Agent: commit --amend -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl ci --amend -m "ci amend message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]

Agent: commit --amend -l dropping diff number should abort:

  $ printf 'commit amend logfile message without diff number\n' > commit-message.txt
  $ CODING_AGENT_METADATA=id=test_agent sl commit --amend -l commit-message.txt
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]

Agent: metaedit -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl metaedit -m "another message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]

Interactive user choosing No should abort (amend):

  $ sl amend --config ui.interactive=true --config devel.print-metrics=commit.baddiffid -m "drop diff number" <<EOF
  > n
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  n
  abort: aborted by user
  commit.baddiffid.human_prompt_no: 1
  [255]

Interactive user choosing No should abort (metaedit):

  $ sl metaedit --config ui.interactive=true -m "drop diff number" <<EOF
  > n
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  n
  abort: aborted by user
  [255]

Interactive user choosing Yes should proceed (amend):

  $ sl amend --config ui.interactive=true --config devel.print-metrics=commit.baddiffid -m "drop diff number via amend" <<EOF
  > y
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  5a4d097da8bb -> 78d316c8be37 "drop diff number via amend"
  commit.baddiffid.human_prompt_yes: 1

Restore diff number for next test:

  $ HGPLAIN=1 sl amend -m "$(printf 'restored\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  78d316c8be37 -> 9ea48f174b42 "restored"

Interactive user choosing Yes should proceed (metaedit):

  $ sl metaedit --config ui.interactive=true -m "drop diff number via metaedit" <<EOF
  > y
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  9ea48f174b42 -> 8f89e739bba4 "drop diff number via metaedit"

Non-interactive defaults to Yes (amend):

Restore diff number:

  $ HGPLAIN=1 sl amend -m "$(printf 'restored again\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  8f89e739bba4 -> f6c732d5b9eb "restored again"

  $ sl amend -m "non-interactive drop"
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  y
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  f6c732d5b9eb -> cad5328c7ead "non-interactive drop"

HGPLAIN should allow restoring a Differential Revision:

  $ HGPLAIN=1 sl amend --config devel.print-metrics=commit.baddiffid -m "$(printf 'new message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  cad5328c7ead -> 334772907fae "new message"
  commit.baddiffid.automation_allowed: 1

Metaedit -m preserving Differential Revision should succeed:

  $ sl metaedit -m "$(printf 'another message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  334772907fae -> ded7f3602a29 "another message"

Agent: changing to an unrelated Differential Revision should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "$(printf 'unrelated diff\n\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')"
  abort: commit rewrite introduces unexpected phabricator diff number(s) 'D23456'; predecessor diff number(s): 'D12345'
  (run 'jf unlink' before the rewrite, then run 'jf link --diff D23456' afterward to change the association intentionally)
  [255]

Config override should allow dropping:

  $ sl amend --config fbcodereview.allow-diff-revision-drop=true --config devel.print-metrics=commit.baddiffid -m "message without diff number"
  ded7f3602a29 -> 1704a68c6e26 "message without diff number"
  commit.baddiffid.config_allowed: 1

Restore diff number before testing amend without -m:

  $ HGPLAIN=1 sl amend -m "$(printf 'restored for amend test\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
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

Ordinary rebase preserves the Differential Revision across repeated rebases:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo dest1 > dest1
  $ sl commit -Aqm dest1
  $ DEST1=$(sl log -r . -T '{node}')
  $ sl go -q $BASE
  $ echo dest2 > dest2
  $ sl commit -Aqm dest2
  $ DEST2=$(sl log -r . -T '{node}')
  $ sl go -q $BASE
  $ echo source > source
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'source\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -r . -d $DEST1
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -q -r . -d $DEST2
  $ sl log -r . -T '[{phabdiff}] {desc|firstline}\n'
  [D12345] source

A bare D number binds the commit the same way jf parses it:

  $ newclientrepo
  $ echo bare > bare
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'bare\n\nDifferential Revision: D12345')"
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]
  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "drops bare diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]

Indented or quoted Differential Revision lines are not bindings, matching jf:

  $ sl amend -m "$(printf 'bare\n\nSummary: quoting another commit message:\n  Differential Revision: https://phabricator.intern.facebook.com/D99999\n\nDifferential Revision: D12345')"
  * -> * "bare" (glob)
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Agent: fold dropping every predecessor Differential Revision should abort:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ echo second > second
  $ sl commit -Aqm second
  $ CODING_AGENT_METADATA=id=test_agent sl fold --from .^ -m folded >/dev/null
  abort: commit message drops phabricator diff number 'D12345'
  (choose one of the predecessor diff numbers ('D12345') for the final commit; to keep no diff number after folding, collapsing, or editing history, run 'jf unlink' before the rewrite)
  [255]
  $ sl log -r .^ -T '[{phabdiff}]\n'
  [D12345]

Fold may deliberately retain one Differential Revision from two linked predecessors:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ echo second > second
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'second\n\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')"
  $ CODING_AGENT_METADATA=id=test_agent sl fold --from .^ --reuse-message .^ >/dev/null
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Agent: fresh commit introducing a Differential Revision should abort:

  $ newclientrepo
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ echo second > second
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm "$(printf 'second\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  abort: commit introduces phabricator diff number(s) 'D12345'
  (create the commit without a Differential Revision line, then run 'jf link --diff D12345' to associate it intentionally)
  [255]

Config override should allow introducing a Differential Revision:

  $ CODING_AGENT_METADATA=id=test_agent sl commit --config fbcodereview.allow-diff-revision-drop=true -Aqm "$(printf 'second\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ sl log -r '.^ + .' -T '[{phabdiff}]\n'
  [D12345]
  [D12345]

Rewriting one existing duplicate should succeed:

  $ echo amended >> second
  $ CODING_AGENT_METADATA=id=test_agent sl amend
  * -> * "second" (glob)
  $ sl log -r '.^ + .' -T '[{phabdiff}]\n'
  [D12345]
  [D12345]

HGPLAIN can create a duplicate Differential Revision:

  $ echo plain > plain
  $ HGPLAIN=1 CODING_AGENT_METADATA=id=test_agent sl commit -Aqm "$(printf 'plain\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ sl log -r '.~2 + .^ + .' -T '[{phabdiff}]\n'
  [D12345]
  [D12345]
  [D12345]

Human approval of repeated Differential Revision lines should prompt once:

  $ newclientrepo
  $ echo first > first
  $ sl commit --config ui.interactive=true -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')" <<EOF
  > y
  > EOF
  commit message contains multiple phabricator diff numbers 'D12345', proceed (Yn)?  y
  warning: commit message contains multiple phabricator diff numbers 'D12345'
  (keep exactly one Differential Revision line in the commit message; to change the association, run 'jf unlink' before the rewrite and 'jf link --diff D<number>' afterward)

Agent guidance uses a non-literal placeholder when several diff numbers are present:

  $ newclientrepo
  $ echo first > first
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')"
  abort: commit message contains multiple phabricator diff numbers 'D12345', 'D23456'
  (keep exactly one Differential Revision line in the commit message; to change the association, run 'jf unlink' before the rewrite and 'jf link --diff D<number>' afterward)
  [255]

Graft should not invoke the copied-message hook when it has no changes:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo source > source
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'source\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ SOURCE=$(sl log -r . -T '{node}')
  $ sl go -q $BASE
  $ echo source > source
  $ sl commit -Aqm same-tree
  $ NOOP_DESTINATION=$(sl log -r . -T '{node}')
  $ sl graft -q $SOURCE
  note: graft of * created no changes to commit (glob)
  $ test "$(sl log -r . -T '{node}')" = "$NOOP_DESTINATION"

Agent: graft should unlink the copied commit:

  $ sl go -q $BASE
  $ echo destination > destination
  $ sl commit -Aqm destination
  $ DESTINATION=$(sl log -r . -T '{node}')
  $ CODING_AGENT_METADATA=id=test_agent sl graft --config devel.print-metrics=commit.baddiffid $SOURCE >/dev/null
  note: removed phabricator diff number 'D12345' from the commit copied by graft; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_agent_unlinked: 1
  $ sl log -r "$SOURCE + ." -T '[{phabdiff}]\n'
  [D12345]
  []

Agent warn mode should keep the copied diff number with a warning:

  $ sl go -q $DESTINATION
  $ CODING_AGENT_METADATA=id=test_agent sl graft -q --config fbcodereview.bad-diff-id-agent-mode=warn --config devel.print-metrics=commit.baddiffid $SOURCE
  warning: copying this commit with graft associates multiple commits with 'D12345'
  (run 'jf unlink' from the new commit to remove the duplicate association)
  commit.baddiffid.copy_agent_warned: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Unrecognized agent mode should keep the copied diff number:

  $ sl go -q $DESTINATION
  $ CODING_AGENT_METADATA=id=test_agent sl graft -q --config fbcodereview.bad-diff-id-agent-mode=false $SOURCE
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Agent: graft should not transform a user-supplied message:

  $ sl go -q $DESTINATION
  $ CODING_AGENT_METADATA=id=test_agent sl graft -q -m "$(printf 'custom\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')" $SOURCE
  abort: commit introduces phabricator diff number(s) 'D12345'
  (create the commit without a Differential Revision line, then run 'jf link --diff D12345' to associate it intentionally)
  [255]
  $ test "$(sl log -r . -T '{node}')" = "$DESTINATION"
  $ sl go -qC $DESTINATION

Human graft prompt should offer to remove the diff number:

  $ sl go -q $DESTINATION
  $ sl graft -q --config ui.interactive=true --config devel.print-metrics=commit.baddiffid $SOURCE <<EOF
  > r
  > EOF
  copying this commit would associate multiple commits with 'D12345'; [r]emove the diff number from the new commit, [p]roceed with the duplicate association, or [c]ancel?  r
  note: removed phabricator diff number 'D12345' from the commit copied by graft; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_human_unlinked: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  []

Human warning mode should keep the copied diff number without prompting:

  $ sl go -q $DESTINATION
  $ sl graft -q --config ui.interactive=true --config fbcodereview.bad-diff-id-human-mode=warn --config devel.print-metrics=commit.baddiffid $SOURCE
  warning: copying this commit with graft associates multiple commits with 'D12345'
  (run 'jf unlink' from the new commit to remove the duplicate association)
  commit.baddiffid.copy_human_warned: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Unrecognized human mode should keep the copied diff number without prompting:

  $ sl go -q $DESTINATION
  $ sl graft -q --config ui.interactive=true --config fbcodereview.bad-diff-id-human-mode=off $SOURCE
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Human graft prompt should default to proceeding with a warning:

  $ sl go -q $DESTINATION
  $ sl graft -q --config devel.print-metrics=commit.baddiffid $SOURCE
  copying this commit would associate multiple commits with 'D12345'; [r]emove the diff number from the new commit, [p]roceed with the duplicate association, or [c]ancel?  p
  warning: copying this commit with graft associates multiple commits with 'D12345'
  (run 'jf unlink' from the new commit to remove the duplicate association)
  commit.baddiffid.copy_human_proceeded: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Human graft prompt should allow cancelling:

  $ sl go -q $DESTINATION
  $ sl graft -q --config ui.interactive=true --config devel.print-metrics=commit.baddiffid $SOURCE <<EOF
  > c
  > EOF
  copying this commit would associate multiple commits with 'D12345'; [r]emove the diff number from the new commit, [p]roceed with the duplicate association, or [c]ancel?  c
  abort: aborted by user
  commit.baddiffid.copy_human_cancelled: 1
  [255]
  $ test "$(sl log -r . -T '{node}')" = "$DESTINATION"

Disabling unlink-copied-diff-revisions should restore the old copy behavior:

  $ sl go -qC $DESTINATION
  $ CODING_AGENT_METADATA=id=test_agent sl graft -q --config fbcodereview.unlink-copied-diff-revisions=false $SOURCE
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]

Human abort mode should reject the copy outright:

  $ sl go -qC $DESTINATION
  $ sl graft -q --config fbcodereview.bad-diff-id-human-mode=abort --config devel.print-metrics=commit.baddiffid $SOURCE
  abort: copying this commit with graft would associate multiple commits with 'D12345'
  (run 'jf unlink' on the source commit first, or set 'fbcodereview.bad-diff-id-human-mode=prompt' to choose interactively)
  commit.baddiffid.copy_human_rejected: 1
  [255]
  $ test "$(sl log -r . -T '{node}')" = "$DESTINATION"

Agent: graft of an obsolete commit should keep an otherwise-lost diff number:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo lost > lost
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'lost\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ LOST=$(sl log -r . -T '{node}')
  $ echo keepalive > keepalive
  $ sl commit -Aqm keepalive
  $ sl go -q $LOST
  $ HGPLAIN=1 sl amend -q --no-rebase -m "lost without diff number"
  $ sl go -q $BASE
  $ echo elsewhere > elsewhere
  $ sl commit -Aqm elsewhere
  hint[amend-restack]: descendants of eabcd03bef92 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ CODING_AGENT_METADATA=id=test_agent sl graft --config devel.print-metrics=commit.baddiffid $LOST >/dev/null
  commit.baddiffid.copy_recovery_kept: 1
  $ sl log -r . -T '[{phabdiff}] {desc|firstline}\n'
  [D12345] lost

Agent: graft of an obsolete commit should unlink when a successor keeps the diff number:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo lost > lost
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'lost\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ LOST=$(sl log -r . -T '{node}')
  $ echo keepalive > keepalive
  $ sl commit -Aqm keepalive
  $ sl go -q $LOST
  $ HGPLAIN=1 sl amend -q --no-rebase -m "$(printf 'lost v2\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ sl go -q $BASE
  $ echo elsewhere > elsewhere
  $ sl commit -Aqm elsewhere
  hint[amend-restack]: descendants of eabcd03bef92 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ CODING_AGENT_METADATA=id=test_agent sl graft --config devel.print-metrics=commit.baddiffid $LOST >/dev/null
  note: removed phabricator diff number 'D12345' from the commit copied by graft; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_agent_unlinked: 1
  $ sl log -r . -T '[{phabdiff}] {desc|firstline}\n'
  [] lost

Agent: graft should unlink when an unrelated draft carries the diff number:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo lost > lost
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'lost\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ LOST=$(sl log -r . -T '{node}')
  $ echo keepalive > keepalive
  $ sl commit -Aqm keepalive
  $ sl go -q $LOST
  $ HGPLAIN=1 sl amend -q --no-rebase -m "lost without diff number"
  $ sl go -q $BASE
  $ echo unrelated > unrelated
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'unrelated carrier\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ sl go -q $BASE
  $ echo elsewhere > elsewhere
  $ sl commit -Aqm elsewhere
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ CODING_AGENT_METADATA=id=test_agent sl graft --config devel.print-metrics=commit.baddiffid $LOST >/dev/null
  note: removed phabricator diff number 'D12345' from the commit copied by graft; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_agent_unlinked: 1
  $ sl log -r . -T '[{phabdiff}] {desc|firstline}\n'
  [] lost

Agent: rebase --keep should unlink the copied commit:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo source > source
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'source\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ SOURCE=$(sl log -r . -T '{node}')
  $ sl go -q $BASE
  $ echo destination > destination
  $ sl commit -Aqm destination
  $ DESTINATION=$(sl log -r . -T '{node}')
  $ CODING_AGENT_METADATA=id=test_agent sl rebase --keep --config devel.print-metrics=commit.baddiffid -r $SOURCE -d $DESTINATION >/dev/null
  note: removed phabricator diff number 'D12345' from the commit copied by rebase --keep; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_agent_unlinked: 1
  $ sl log -r "$SOURCE + children($DESTINATION)" -T '[{phabdiff}]\n'
  [D12345]
  []

Agent: native rebase --keep should unlink the copied commit:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ BASE=$(sl log -r . -T '{node}')
  $ echo source > source
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'source\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ SOURCE=$(sl log -r . -T '{node}')
  $ sl go -q $BASE
  $ echo destination > destination
  $ sl commit -Aqm destination
  $ DESTINATION=$(sl log -r . -T '{node}')
  $ CODING_AGENT_METADATA=id=test_agent sl rebase --keep --config nativecheckout.rebaseonenative=true --config devel.print-metrics=commit.baddiffid -r $SOURCE -d $DESTINATION >/dev/null
  note: removed phabricator diff number 'D12345' from the commit copied by rebase --keep; the new commit is not linked to a phabricator diff
  commit.baddiffid.copy_agent_unlinked: 1
  $ sl log -r "$SOURCE + children($DESTINATION)" -T '[{phabdiff}]\n'
  [D12345]
  []

Agent: split copying one Differential Revision to every successor should abort:

  $ newclientrepo
  $ echo first > first
  $ echo second > second
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'to split\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ CODING_AGENT_METADATA=id=test_agent sl split --config ui.interactive=true <<EOF >/dev/null
  > y
  > y
  > n
  > y
  > EOF
  transaction abort!
  rollback completed
  abort: commit rewrite creates duplicate phabricator diff number(s) 'D12345'
  (keep the Differential Revision line on exactly one successor and remove it from the other successor commit messages)
  [255]
  $ sl log -r '.^ + .' -T '{desc}\n---\n'
  to split
  
  Differential Revision: https://phabricator.intern.facebook.com/D12345
  ---

A split may retain the Differential Revision on exactly one successor:

  $ cat > edit-second-split-message.py <<'EOF'
  > import sys
  > from pathlib import Path
  > state = Path("split-message-edited")
  > if state.exists():
  >     message = Path(sys.argv[1])
  >     lines = message.read_text().splitlines()
  >     message.write_text(
  >         "\n".join(
  >             line
  >             for line in lines
  >             if not line.startswith("Differential Revision:")
  >         )
  >         + "\n"
  >     )
  > state.touch()
  > EOF
  $ HGEDITOR="$PYTHON edit-second-split-message.py" CODING_AGENT_METADATA=id=test_agent sl split --config ui.interactive=true <<EOF >/dev/null
  > y
  > y
  > n
  > y
  > EOF
  $ sl log -r '.^ + .' -T '[{phabdiff}]\n'
  [D12345]
  []

Agent: single-successor split dropping the Differential Revision should abort:

  $ newclientrepo
  $ echo first > first
  $ echo second > second
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'to split once\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ cat > drop-split-message.py <<'EOF'
  > import sys
  > from pathlib import Path
  > message = Path(sys.argv[1])
  > lines = message.read_text().splitlines()
  > message.write_text(
  >     "\n".join(
  >         line
  >         for line in lines
  >         if not line.startswith("Differential Revision:")
  >     )
  >     + "\n"
  > )
  > EOF
  $ HGEDITOR="$PYTHON drop-split-message.py" CODING_AGENT_METADATA=id=test_agent sl split --config ui.interactive=true <<EOF >/dev/null
  > y
  > y
  > y
  > y
  > EOF
  transaction abort!
  rollback completed
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]
  $ sl log -r . -T '[{phabdiff}] {desc|firstline}\n'
  [D12345] to split once

Agent: rebase collapse dropping every predecessor Differential Revision should abort:

  $ newclientrepo
  $ echo base > base
  $ sl commit -Aqm base
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ echo second > second
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'second\n\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')"
  $ FIRST=$(sl log -r .^ -T '{node}')
  $ SECOND=$(sl log -r . -T '{node}')
  $ CODING_AGENT_METADATA=id=test_agent sl rebase --collapse -s .^ -d .~2 -m collapsed >/dev/null
  abort: commit rewrite loses phabricator diff number(s) 'D12345', 'D23456'
  (choose one of the predecessor diff numbers ('D12345', 'D23456') for the final commit; to keep no diff number after folding, collapsing, or editing history, run 'jf unlink' before the rewrite)
  [255]
  $ sl log -r "$FIRST + $SECOND" -T '[{phabdiff}]\n'
  [D12345]
  [D23456]

The rejection happens before the rebase starts, so no rebase is in progress:

  $ sl rebase --abort
  no remote bookmarks, cleanup skipped.
  abort: no rebase in progress
  [255]

Rebase collapse may deliberately retain either predecessor Differential Revision:

  $ CODING_AGENT_METADATA=id=test_agent sl rebase --collapse -s .^ -d .~2 -m "$(printf 'collapsed\n\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')" >/dev/null
  $ sl log -r . -T '[{phabdiff}]\n'
  [D23456]

Agent: histedit mess dropping the Differential Revision should abort:

  $ newclientrepo
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ cat > drop-message.py <<'EOF'
  > import sys
  > with open(sys.argv[1], "wb") as message_file:
  >     message_file.write(b"changed\n")
  > EOF
  $ NODE=$(sl log -r . -T '{node}')
  $ HGEDITOR="$PYTHON drop-message.py" CODING_AGENT_METADATA=id=test_agent sl histedit --commands - <<EOF >/dev/null
  > mess $NODE
  > EOF
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]
  $ sl log -r $NODE -T '[{phabdiff}]\n'
  [D12345]

Agent and human modes can warn instead of blocking or prompting:

  $ newclientrepo
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config fbcodereview.bad-diff-id-agent-mode=warn --config devel.print-metrics=commit.baddiffid -m "agent warning"
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  * -> * "agent warning" (glob)
  commit.baddiffid.agent_warned: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  []

Unrecognized agent and human modes should ignore the guard:

  $ HGPLAIN=1 sl amend -m "$(printf 'restored\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  * -> * "restored" (glob)
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config fbcodereview.bad-diff-id-agent-mode=off -m "agent mode off"
  * -> * "agent mode off" (glob)
  $ sl log -r . -T '[{phabdiff}]\n'
  []

  $ HGPLAIN=1 sl amend -m "$(printf 'restored again\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  * -> * "restored again" (glob)
  $ sl amend --config ui.interactive=true --config fbcodereview.bad-diff-id-human-mode=false -m "human mode false"
  * -> * "human mode false" (glob)
  $ sl log -r . -T '[{phabdiff}]\n'
  []

  $ HGPLAIN=1 sl amend -m "$(printf 'restored for warning\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  * -> * "restored for warning" (glob)
  $ sl amend --config ui.interactive=true --config fbcodereview.bad-diff-id-human-mode=warn --config devel.print-metrics=commit.baddiffid -m "human warning"
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  * -> * "human warning" (glob)
  commit.baddiffid.human_warned: 1
  $ sl log -r . -T '[{phabdiff}]\n'
  []

Humans may opt into abort mode and agents may opt into prompt mode:

  $ HGPLAIN=1 sl amend -m "$(printf 'restored for abort\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  * -> * "restored for abort" (glob)
  $ sl amend --config fbcodereview.bad-diff-id-human-mode=abort -m "human abort"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  [255]
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config ui.interactive=true --config fbcodereview.bad-diff-id-agent-mode=prompt -m "agent prompt" <<EOF
  > n
  > EOF
  commit message drops phabricator diff number 'D12345', proceed (Yn)?  n
  abort: aborted by user
  [255]
  $ sl log -r . -T '[{phabdiff}]\n'
  [D12345]
