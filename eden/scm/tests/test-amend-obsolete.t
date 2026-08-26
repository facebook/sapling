#require no-eden

  $ enable amend rebase

Create a commit and amend it to produce an obsolete predecessor:

  $ newclientrepo
  $ echo a > a
  $ sl add a
  $ sl commit -m "original commit"
  $ sl log -r . -T '{node|short}\n'
  87ce07975dfa
  $ sl amend -m "amended commit"
  $ sl amend -m "amended commit again"
  $ sl debugmutation -r "all()"
   *  58bce2fd05ef3404d5fb87d8fa94a8e4fdfc331e amend by test at 1970-01-01T00:00:00 from:
      4ccb9bde2b77c1549b886c8f34d05caeccc3e298 amend by test at 1970-01-01T00:00:00 from:
      87ce07975dfa08ef73b58855de6c810a6c7c20a5

Go back to the obsolete commit:

  $ sl go 87ce07975dfa
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Agent: amend on obsolete commit should abort:

  $ echo b > b
  $ sl add b
  $ CODING_AGENT_METADATA=id=test_agent sl amend -m "agent amend"
  abort: changing an old version of a commit will diverge your stack:
  - 87ce07975dfa -> 58bce2fd05ef (rewrite)
  (switch to the newer version listed above, or run 'sl graft' with the old commit hash to deliberately fork it; 'sl sl' shows the latest graph)
  [255]

Agent: commit --amend on obsolete commit should abort:

  $ CODING_AGENT_METADATA=id=test_agent sl commit --amend -m "agent commit --amend"
  abort: changing an old version of a commit will diverge your stack:
  - 87ce07975dfa -> 58bce2fd05ef (rewrite)
  (switch to the newer version listed above, or run 'sl graft' with the old commit hash to deliberately fork it; 'sl sl' shows the latest graph)
  [255]

Interactive user choosing No should abort:

  $ sl amend --config ui.interactive=true -m "user amend no" <<EOF
  > n
  > EOF
  warning: changing an old version of a commit will diverge your stack:
  - 87ce07975dfa -> 58bce2fd05ef (rewrite)
  proceed with amend (Yn)?  n
  abort: aborted by user
  [255]

Interactive user choosing Yes should proceed:

  $ sl amend --config ui.interactive=true -m "user amend yes" <<EOF
  > y
  > EOF
  warning: changing an old version of a commit will diverge your stack:
  - 87ce07975dfa -> 58bce2fd05ef (rewrite)
  proceed with amend (Yn)?  y

Should not block SL_AUTOMATION

  $ sl go 87ce07975dfa
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ sl add c
  $ SL_AUTOMATION=true sl am -m "automation script amend"

Config override should allow amending obsolete commits:

  $ sl go 87ce07975dfa
  0 files updated, 0 files merged, * files removed, 0 files unresolved (glob)
  $ echo d > d
  $ sl add d
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config commit.reject-modifying-obsolete=False -m "config override amend"

Explicitly allowing divergence should allow amending obsolete commits:

  $ sl go 87ce07975dfa
  0 files updated, 0 files merged, * files removed, 0 files unresolved (glob)
  $ echo e > e
  $ sl add e
  $ CODING_AGENT_METADATA=id=test_agent sl amend --config experimental.evolution.allowdivergence=true -m "allowdivergence amend"

Set up an obsolete commit with one live successor:

  $ make_obsolete_commit() {
  >   newclientrepo
  >   echo base > base
  >   sl commit -Aqm base
  >   echo original > original
  >   sl commit -Aqm original
  >   SOURCE=$(sl log -r . -T '{node}')
  >   sl amend -qm successor
  > }

  $ make_obsolete_commit_with_visible_descendant() {
  >   newclientrepo
  >   echo base > base
  >   sl commit -Aqm base
  >   BASE=$(sl log -r . -T '{node}')
  >   echo original > original
  >   sl commit -Aqm original
  >   SOURCE=$(sl log -r . -T '{node}')
  >   echo descendant > descendant
  >   sl commit -Aqm descendant
  >   sl go -q "$BASE"
  >   echo successor > successor
  >   sl commit -Aqm successor
  >   SUCCESSOR=$(sl log -r . -T '{node}')
  >   sl debugobsolete "$SOURCE" "$SUCCESSOR"
  > }

Agent: uncommit should reject rewriting an obsolete commit:

  $ make_obsolete_commit
  $ sl go -q $SOURCE
  $ CODING_AGENT_METADATA=id=test_agent sl uncommit
  abort: changing an old version of a commit will diverge your stack:
  - * -> * (amend) (glob)
  (switch to the newer version listed above, or run 'sl graft' with the old commit hash to deliberately fork it; 'sl sl' shows the latest graph)
  [255]
  $ sl log -r . -T '{desc|firstline}\n'
  original

Agent: rebase should reject all rewrites before starting when a later commit is obsolete:

  $ newclientrepo
  $ echo foundation > foundation
  $ sl commit -Aqm foundation
  $ echo first > first
  $ sl commit -Aqm first
  $ FIRST=$(sl log -r . -T '{node}')
  $ echo original > original
  $ sl commit -Aqm original
  $ SOURCE=$(sl log -r . -T '{node}')
  $ sl amend -qm successor
  $ sl go -q "$FIRST^"
  $ echo destination > destination
  $ sl commit -Aqm destination
  $ CODING_AGENT_METADATA=id=test_agent sl rebase -r "$FIRST::$SOURCE" -d .
  abort: changing an old version of a commit will diverge your stack:
  - * -> * (amend) (glob)
  (switch to the newer version listed above, or run 'sl graft' with the old commit hash to deliberately fork it; 'sl sl' shows the latest graph)
  [255]
  $ sl log -r "successors($FIRST) - $FIRST" -T '{desc|firstline}\n'
  $ sl log -r . -T '{desc|firstline}\n'
  destination
  $ sl log -r "successors($SOURCE) - obsolete()" -T '{desc|firstline}\n'
  successor

Agent should reject rewriting an obsolete commit after its successor lands:

  $ make_obsolete_commit
  $ SUCCESSOR=$(sl log -r . -T '{node}')
  $ sl debugmakepublic $SUCCESSOR
  $ sl go -q $SOURCE
  $ CODING_AGENT_METADATA=id=test_agent sl amend -qm "post-land rewrite"
  abort: changing an old version of a commit will diverge your stack:
  - * -> * (amend) (glob)
  (switch to the newer version listed above, or run 'sl graft' with the old commit hash to deliberately fork it; 'sl sl' shows the latest graph)
  [255]
  $ sl log -r . -T '{desc|firstline}\n'
  original

Direct commitctx should ignore a mutation predecessor that is not stored locally:

  $ cat > $TESTTMP/commitctx-missing-predecessor.py <<'EOF'
  > from sapling import context, hg, mutation, node, ui as uimod
  > ui = uimod.ui.load()
  > repo = hg.repository(ui, ".")
  > unknown = node.bin("f" * 40)
  > mutinfo = mutation.record(repo, {}, [unknown], op="rewrite")
  > ctx = context.memctx(repo, [repo["."]], "missing predecessor", [], lambda *args: None, mutinfo=mutinfo)
  > committed = repo.commitctx(ctx)
  > print(repo[committed].description())
  > EOF
  $ sl debugpython -- $TESTTMP/commitctx-missing-predecessor.py
  missing predecessor

Agent: commit currently creates a child on an obsolete parent:

  $ make_obsolete_commit
  $ sl go -q $SOURCE
  $ echo child > child
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm child
  $ sl log -r '.^ & obsolete()' -T '{desc|firstline}\n'
  original
  $ sl log -r . -T '{desc|firstline}\n'
  child

Agent should allow creating a child on an obsolete parent with visible descendants:

  $ make_obsolete_commit_with_visible_descendant
  $ sl go -q --config commit.reject-modifying-obsolete=false "$SOURCE"
  $ echo another-child > another-child
  $ CODING_AGENT_METADATA=id=test_agent sl commit -Aqm another-child
  $ sl log -r '.^ + .' -T '{desc|firstline}\n'
  original
  another-child

Agent: goto currently checks out an obsolete commit without warning:

  $ make_obsolete_commit
  $ CODING_AGENT_METADATA=id=test_agent sl go $SOURCE
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -r . -T '{desc|firstline}\n'
  original

Agent should allow going to an obsolete commit with visible descendants:

  $ make_obsolete_commit_with_visible_descendant
  $ CODING_AGENT_METADATA=id=test_agent sl go "$SOURCE"
  * files updated, 0 files merged, * files removed, 0 files unresolved (glob)
  $ sl log -r . -T '{desc|firstline}\n'
  original
