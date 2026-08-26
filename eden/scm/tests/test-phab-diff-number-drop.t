
  $ enable amend fbcodereview histedit morestatus rebase
  $ setconfig tweakdefaults.showupdated=true

Create a commit with a Differential Revision line:

  $ newclientrepo
  $ echo a > a
  $ sl add a
  $ HGPLAIN=1 sl commit -m "$(printf 'first commit\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"

Agent: amend -m dropping diff number should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "new message without diff number"
  abort: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
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
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  5a4d097da8bb -> 78d316c8be37 "drop diff number via amend"

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

  $ HGPLAIN=1 sl amend -m "$(printf 'new message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  cad5328c7ead -> 334772907fae "new message"

Metaedit -m preserving Differential Revision should succeed:

  $ sl metaedit -m "$(printf 'another message\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  334772907fae -> ded7f3602a29 "another message"

Agent: changing to an unrelated Differential Revision should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "$(printf 'unrelated diff\n\nDifferential Revision: https://phabricator.intern.facebook.com/D23456')"
  abort: commit rewrite introduces unexpected phabricator diff number(s) 'D23456'; predecessor diff number(s): 'D12345'
  (run 'jf unlink' before the rewrite, then run 'jf link --diff D23456' afterward to change the association intentionally)
  [255]

Config override should allow dropping:

  $ sl amend --config fbcodereview.allow-diff-revision-drop=true -m "message without diff number"
  ded7f3602a29 -> 1704a68c6e26 "message without diff number"

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

Fresh commit can currently reuse a Differential Revision:

  $ newclientrepo
  $ echo first > first
  $ HGPLAIN=1 sl commit -Aqm "$(printf 'first\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
  $ echo second > second
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm "$(printf 'second\n\nDifferential Revision: https://phabricator.intern.facebook.com/D12345')"
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

Rebase --keep can currently copy a Differential Revision:

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
  $ CODING_AGENT_METADATA=id=test_agent sl rebase --keep -r $SOURCE -d $DESTINATION >/dev/null
  $ sl log -r "$SOURCE + children($DESTINATION)" -T '[{phabdiff}]\n'
  [D12345]
  [D12345]

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
  $ sl rebase --abort
  no remote bookmarks, cleanup skipped.
  rebase aborted

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
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config fbcodereview.bad-diff-id-agent-mode=warn -m "agent warning"
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  * -> * "agent warning" (glob)
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
  $ sl amend --config ui.interactive=true --config fbcodereview.bad-diff-id-human-mode=warn -m "human warning"
  warning: commit message drops phabricator diff number 'D12345'
  (run 'jf unlink' to intentionally remove the associated diff; use 'jf template' to edit other commit message fields)
  * -> * "human warning" (glob)
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
