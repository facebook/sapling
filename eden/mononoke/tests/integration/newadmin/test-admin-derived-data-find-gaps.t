# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ADDITIONAL_DERIVED_DATA="content_manifests" setup_common_config "blob_sqlite"

A linear history of 10 commits. Generations are 1 (A) .. 10 (J), tip is `main`.
  $ mononoke_testtool drawdag -R repo > /dev/null <<'EOF'
  > A-B-C-D-E-F-G-H-I-J
  > # bookmark: J main
  > # bookmark: C c
  > # bookmark: E e
  > # bookmark: F f
  > # bookmark: G g
  > # bookmark: H h
  > # bookmark: I i
  > EOF

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:derived_data_use_content_manifests": true
  >   }
  > }
  > EOF

content_manifests derives from its fsnodes predecessor, so
--unsafe-derive-untopologically lets us derive single commits while their
ancestors stay underived. Build a gap in the middle of history: derive A,B,C
(gen 1..3), leave D,E (gen 4,5) underived, then derive F..J (gen 6..10).

  $ mononoke_admin derived-data -R repo derive -T fsnodes -B main
  $ mononoke_admin derived-data -R repo derive -T content_manifests -B c
  $ mononoke_admin derived-data -R repo derive -T content_manifests --unsafe-derive-untopologically -B f -B g -B h -B i -B main

A small step samples every commit. The highest underived sample (E, gen 5) is
reported once with the full measured gap size (D and E = 2); D is then skipped
rather than re-checked.
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 2>/dev/null | grep '^GAP' | sed 's/ [0-9a-f]*$//'
  GAP generation=5 size=2
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 2>&1 >/dev/null | grep done
  done: checked 9 boundary commits, found 1 gap(s) totalling 2 underived commit(s)

A coarser step can step over the gap: --step 3 samples generations 10,9,6,3
(all derived), so the gap at 4,5 is missed.
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 3 2>/dev/null | grep '^GAP' | sed 's/ [0-9a-f]*$//'
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 3 2>&1 >/dev/null | grep done
  done: checked 4 boundary commits, found 0 gap(s) totalling 0 underived commit(s)

count-underived is blind to the gap: the tip is derived, so it assumes
monotonicity and reports 0.
  $ mononoke_admin derived-data -R repo count-underived -T content_manifests -B main | sed 's/^[0-9a-f]*:/main:/'
  main: 0

`derive` on the tip is no help either -- same derived-frontier logic, sees the
tip derived, does nothing. The gap remains.
  $ mononoke_admin derived-data -R repo derive -T content_manifests -B main
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 2>/dev/null | grep '^GAP' | sed 's/ [0-9a-f]*$//'
  GAP generation=5 size=2

Fixing the gap requires deriving the tip of the gap (E): the frontier then stops
at the last derived commit below it (C), so D and E get derived.
  $ mononoke_admin derived-data -R repo derive -T content_manifests -B e
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 2>/dev/null | grep '^GAP' | sed 's/ [0-9a-f]*$//'
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 2>&1 >/dev/null | grep done
  done: checked 10 boundary commits, found 0 gap(s) totalling 0 underived commit(s)

With everything derived, a small --report-interval emits periodic progress lines
for the clean stretches.
  $ mononoke_admin derived-data -R repo find-derivation-gaps -T content_manifests -B main --step 1 --report-interval 3 2>/dev/null | grep '^CLEAN'
  CLEAN no gaps from generation 10 to 7
  CLEAN no gaps from generation 7 to 4
  CLEAN no gaps from generation 4 to 1
