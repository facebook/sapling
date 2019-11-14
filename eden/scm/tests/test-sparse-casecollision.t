#require no-icasefs

Test sparse profiles in combination with case-collisions outside of the
profile.

  $ cat > force_case_insensitivity.py <<EOF
  > # We force the issue at update time, by monkey-patching util.fscasesensitive
  > # and util.normcase to act like a case-insensitive filesystem
  > from edenscm.mercurial import encoding, util
  > util.fscasesensitive = lambda *args: False
  > util.normcase = lambda p: encoding.upper(p)
  > EOF

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > EOF

  $ mkdir profiles
  $ cat > profiles/sparse_profile <<EOF
  > [exclude]
  > colliding_dir
  > EOF
  $ hg add profiles -q
  $ hg commit -m 'profiles'

  $ mkdir colliding_dir
  $ cd colliding_dir

  $ echo a > a
  $ echo A > A
  $ hg add A a
  warning: possible case-folding collision for colliding_dir/a
  $ hg commit -m '#1'
  $ cd ..
  $ hg up -r 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

The case collision is ignored when the sparse profile is enabled:

  $ cat >> .hg/hgrc <<EOF
  > force_case_insensitivity=../force_case_insensitivity.py
  > EOF
  $ hg up -r 1
  abort: case-folding collision between colliding_dir/[Aa] and colliding_dir/[aA] (re)
  [255]
  $ hg sparse --enable-profile profiles/sparse_profile
  $ hg up -r 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

