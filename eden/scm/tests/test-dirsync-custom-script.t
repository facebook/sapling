#require no-eden

  $ configure modern
  $ enable dirsync amend
  $ newrepo

Setup dirsync

  $ cat > .hgdirsync << EOF
  > [dirsync]
  > sync1.a = a
  > sync1.b = b
  > [dirsync-scripts]
  > sync1 = sync-script.py
  > EOF

  $ cat > sync-script.py << 'EOF'
  > def mirror_path(src_dir, dst_dir, src_rel):
  >     if src_dir == "a/":
  >         # a -> b mirror
  >         # .log -> .txt
  >         if src_rel.endswith('.log'):
  >             return src_rel[:-4] + '.txt'
  >         # drop .zip files
  >         if src_rel.endswith('.zip'):
  >             return None
  >     else:
  >         # b -> a mirror
  >         # .txt -> log
  >         if src_rel.endswith('.txt'):
  >             return src_rel[:-4] + '.log'
  >     # keep other paths as-is
  >     return src_rel
  > def mirror_data(src_dir, dst_dir, src_rel, src_data: bytes) -> bytes:
  >     # Replace 2024 (a) with 2025 (b).
  >     if src_dir == "a/":
  >         return src_data.replace(b'2024', b'2025')
  >     else:
  >         return src_data.replace(b'2025', b'2024')
  > EOF

  $ sl commit -m 'init dirsync' -A .hgdirsync sync-script.py

By default, [dirsync-scripts] in .hgdirsync is ignored for security reasons:

  $ mkdir a
  $ cd a
  $ echo 'Test 2024' > z.log
  $ hg add -q .

Since the scripts are disabled, dirsync will not rename '.log' to '.txt' or replace 2024 with 2025:

  $ hg commit -m 'test dirsync 1'
  mirrored adding 'a/z.log' to 'b/z.log'
  $ cat ../b/z.log
  Test 2024

Enable in-repo scripts:

  $ setconfig dirsync.allow-in-repo-scripts=true

Try out dirsync:

  $ echo 'The year is 2024.' > x.log
  $ touch y.zip
  $ hg add -q .

Dirsync should respect the script. Rename '.log' to '.txt', skip '.zip' and update the file content:

  $ hg commit -m 'test dirsync 2'
  mirrored adding 'a/x.log' to 'b/x.txt'
  $ cd ../b
  $ cat x.txt
  The year is 2025.

Try out syncing in the other direction:

  $ echo 'Another 2025.' >> x.txt
  $ hg amend
  mirrored adding 'b/x.txt' to 'a/x.log'
  $ cat ../a/x.log
  The year is 2024.
  Another 2024.
