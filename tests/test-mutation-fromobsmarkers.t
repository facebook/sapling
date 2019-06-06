  $ setconfig extensions.treemanifest=!
  $ enable amend rebase
  $ setconfig ui.ssh="$PYTHON \"$TESTDIR/dummyssh\"" ui.interactive=true

Create a commit graph using obsmarkers.

  $ newrepo
  $ drawdag << EOS
  >       O   J M      # amend: A -> D -> F
  >       |   | |      # amend: B -> C
  > B C E G   I L      # rebase: C -> E
  > |/ / /    | |      # split: A -> H, I
  > A D F     H K N    # rebase: C -> J
  >  \|/       \|/     # fold: I, J -> M
  >   Z         Z      # split: H -> K, L
  >                    # fold: K, L, M -> N
  >                    # split: E -> G, O
  > EOS

It's possible for obsmarkers to get duplicated with slightly different
timestamps, due to floating-point number precision issues.  This can
cause problems for mutation import, so fake the situation by deliberately
duplicating obsmarkers.

  $ cat > $TESTTMP/dupobsmarkers.py <<EOF
  > from edenscm.mercurial import obsolete, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'debugdupobsmarkers')
  > def debugdupobsmarkers(ui, repo, **opts):
  >     newmarkers = []
  >     for marker in repo.obsstore._all:
  >         newmarker = list(marker)
  >         newmarker[4] = (newmarker[4][0] - 0.001, newmarker[4][1])
  >         newmarkers.append(tuple(newmarker))
  >     with repo.lock(), repo.svfs("obsstore", "ab") as f:
  >             f.write(b"".join(obsolete.encodemarkers(newmarkers, False, repo.obsstore._version)))
  > EOF

  $ hg debugdupobsmarkers --config extensions.dupobsmarkers=$TESTTMP/dupobsmarkers.py

Backfill the obsmarkers into mutation information.

  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"
  $ hg debugmutationfromobsmarkers
  wrote 10 of 10 entries for 10 commits

The successors and predecessors information should be correct.

  $ hg unhide $B
  $ tglogm
  o  15: 11164ffef7a9 'O'
  |
  o  10: e1beb503e4fb 'G'
  |
  | x  7: 917a077edb8d 'B'  (Rewritten using rewrite into 69a19cab35b2) (Rewritten using split into e1beb503e4fb, 11164ffef7a9)
  | |
  | | o  6: 69a19cab35b2 'N'
  | | |
  o---+  3: 847007ced9a7 'F'
   / /
  x /  1: ac2f7407182b 'A'  (Rewritten using rewrite into 69a19cab35b2) (Rewritten using rewrite into 847007ced9a7)
  |/
  o  0: 48b9aae0607f 'Z'
  
  $ hg debugmutation $O
    11164ffef7a9840cc182930dae0e032875937b6a split by test at 1970-01-01T00:00:01 (split into this and: e1beb503e4fb1cec5df43ac57edfcff177d705ec) (from obsmarker) from:
      e900f94a0435abcada5fcbc21f0ff399981ad817 rebase by test at 1970-01-01T00:00:00 (from obsmarker) from:
        daf025dd63009f8b02a358a8b98a29347717170f amend by test at 1969-12-31T23:59:59 (from obsmarker) from:
          917a077edb8d775c96bc95d34025c800b243ce6f
          917a077edb8d775c96bc95d34025c800b243ce6f
        daf025dd63009f8b02a358a8b98a29347717170f amend by test at 1969-12-31T23:59:59 (from obsmarker) from:
          917a077edb8d775c96bc95d34025c800b243ce6f
          917a077edb8d775c96bc95d34025c800b243ce6f

BUG! There are duplicate predecessors.  Successors-sets and predecessors-set calculations are exponential!
