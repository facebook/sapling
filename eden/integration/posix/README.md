# POSIX file system tests (ported from pjdfstest and CPython)

The tests in this directory are ports of two POSIX file system test suites
to the EdenFS integration test framework. They complement each other:

* [pjdfstest](https://github.com/pjd/pjdfstest) by Pawel Jakub Dawidek
  (BSD-2-Clause), at commit `ededbeb2b44929972898afb87474b0937f78a877`,
  checks **path-based semantics**: create, rename, unlink, modes, times and
  errno for every call. Each `tests/<group>/<NN>.t` script became one test
  method `test_<group>_<NN>_<summary>` in `<group>_test.py`
  (`truncate_test.py` holds `truncate/` and `ftruncate/`), with the script
  named in the docstring.
* CPython's `Lib/test/test_posix.py` and `Lib/test/test_os.py`, with the file
  system parts of `Lib/test/test_fcntl.py` and `Lib/test/test_mmap.py`
  (PSF License, Python 3.12.15), check the **other half of the `posix`
  module**: fd-based I/O, the `*at()` calls, space, locks, mmap, names,
  xattrs and directory iteration. Each CPython test became one method of the
  same name in `io_test.py`, `space_test.py`, `dirfd_test.py`, `fd_test.py`,
  `lock_test.py`, `mmap_test.py`, `scandir_test.py`, `names_test.py` or
  `xattr_test.py`, with the CPython module, class and test named in the
  docstring (`test_posix.PosixTester.test_fs_holes`). A few methods marked
  "EdenFS addition" have no CPython original and pin behavior found while
  porting.

The two tables at the end of this file link every script and every CPython
test to its method. Each method runs in a fresh EdenFS mount in the `Default`
(FUSE), `DefaultIoUring` and `NFS` variants.

```
buck2 test '@fbcode//mode/opt' fbcode//eden/integration/posix/...
```

## Translation

The tests call the `posix` module, Python's direct binding of the POSIX
system calls, and use plain `unittest` assertions. (`os` re-exports `posix`
on POSIX systems, but also layers portable helpers on top; the tests stay
with the raw calls.) `base.py` provides the setup every test relies on: an
empty scratch directory inside the mount as the working directory, `umask 0`,
and a few small helpers (`assert_errno`; `bind_unix_socket` for
`create_file socket`; `mkdir_p` and `rm_rf` for the shell commands of the
same name; `name_max` and `path_max` for `namegen_max` and `dirgen_max`;
`utimensat` for `UTIME_OMIT`, which `posix.utime` cannot express;
`create_file`, `read_bytes` and `open_fd` for CPython's `create_file`,
`open(...).read()` and `open`/`close` pairs; `SEEK_SET`, `SEEK_CUR` and
`SEEK_END`, which `posix` leaves to `os`). File names are the script's
variable names (`n0`, `n1`, ...); the CPython ports use the same names in
place of `TESTFN`.

| script | test |
|---|---|
| `expect 0 mkdir ${n0} 0755` | `posix.mkdir("n0", 0o755)` |
| `expect 0 create ${n0} 0644` | `posix.close(posix.open("n0", posix.O_CREAT \| posix.O_EXCL, 0o644))` |
| `expect ENOENT lstat ${n0} type` | `with self.assert_errno(errno.ENOENT): posix.lstat("n0")` |
| `expect dir,0755 lstat ${n0} type,mode` | `st = posix.lstat("n0")`, `assertTrue(stat.S_ISDIR(st.st_mode))`, `assertEqual(0o755, stat.S_IMODE(st.st_mode))` |
| `expect 0 -U 077 mkdir ${n0} 0151` | `posix.umask(0o077)`, `posix.mkdir("n0", 0o151)`, `posix.umask(0)` |
| ``ctime1=`${fstest} stat ${n0} ctime` `` ... `test_check $ctime1 -lt $ctime2` | `ctime1 = posix.stat("n0").st_ctime_ns` ... `self.assertLess(ctime1, posix.stat("n0").st_ctime_ns)` |
| `expect 0 open ${n0} O_RDWR : ftruncate 0 123` | `fd = posix.open("n0", posix.O_RDWR)`, `try: posix.ftruncate(fd, 123)`, `finally: posix.close(fd)` |
| `open . O_RDONLY : utimensat 0 ${n0} $D1 0 $D2 0 0` | `posix.utime("n0", ns=(d1 * 10**9, d2 * 10**9))` |
| `expect "0\|EINVAL" open ...` | `try: posix.close(posix.open(...))`, `except OSError as e: self.assertIn(e.errno, (errno.EINVAL,))` |
| `cd ${n1}` ... `cd ${cdir}` | paths get the `n1/` prefix |

| CPython | test |
|---|---|
| `def test_pread(self):` in `PosixTester` | `def test_pread(self)` with docstring `test_posix.PosixTester.test_pread: ...` |
| `with open(TESTFN, 'w+b') as fp: ... fp.fileno()` | `with self.open_fd("n0", posix.O_RDWR \| posix.O_CREAT) as fd:` |
| `create_file(TESTFN, b'...')`, `open(TESTFN, 'rb').read()` | `self.create_file("n0", b"...")`, `self.read_bytes("n0")` |
| `self.assertRaises(OSError, ...)` on a file system call | `with self.assert_errno(errno.X):` with the errno Linux gives |
| `self.skipTest(...)` where EdenFS deviates | the EdenFS behavior is pinned and the method says `EdenFS by design` |
| `os.walk`, `os.fwalk`, `os.makedirs`, `os.removedirs` | the same `os` functions (they have no `posix` binding; `scandir_test.py` is the only module that imports `os`) |
| `multiprocessing` worker, asyncio TCP server | `posix.fork` child, `socket.socketpair` |
| `TESTFN_UNICODE`, `TESTFN_NONASCII`, `TESTFN_UNENCODABLE` | `UNICODE_NAME`, `NONASCII_NAME`, `UNENCODABLE_NAME` in `names_test.py` |
| checks of the Python API (`TypeError`, `ValueError` before the system call, `pickle`, `repr`, `FakePath`) | left out; the table says `Python API` |

## Which cases are ported

Every pjdfstest script was run with the real `pjdfstest` binary (`prove -v`)
on a FUSE mount of the EdenFS daemon, as an unprivileged user. The steps that
cannot run in that setting were removed, the trimmed scripts were replayed
with `pjdfstest` until every remaining case passed, and that set was
translated. Of the 154 scripts that do anything on Linux, 114 are ported,
with 1,232 of their 1,813 cases; 40 scripts have no check left and are not
ported.

Every CPython class was run on the same FUSE mount (`python3.12 -m unittest
test.test_os.<Class>` with the working directory and `TMPDIR` inside the
mount), then the ported methods were run in the `Default`, `DefaultIoUring`
and `NFS` variants until every check either passed or was classified. 150
methods are ported from 150 CPython tests, and 18 methods are EdenFS
additions; the CPython tests that are not ported check the Python API, need
no file system, or are Windows/macOS only.

A left-out case is marked in place, for example `# cases 17-23: needs root`
or `# EdenFS bug: ...`; the pjdfstest numbers are the TAP case numbers the
script prints on Linux. There are three reasons, below. A check that only
observes the effect of a left-out step (`lstat uid,gid` after a `chown`,
`nlink` after a `link`) is marked `depends on a step above` and counts with
that step's reason. Where a whole CPython test is about a by-design
deviation (CPython skips the test, `self.skipTest(...)`), the EdenFS
behavior is pinned instead, so a change in that behavior shows up.

### 1. EdenFS bugs

Behavior that POSIX specifies differently. Worth fixing; the cases are
marked `EdenFS bug:` in the tests, so a fix makes them portable.

1. **Too-long names return ENOENT.** Over FUSE, an operation on an existing
   path whose last component is longer than `{NAME_MAX}` fails with ENOENT
   instead of ENAMETOOLONG. (Over NFS the client checks the length itself
   and returns ENAMETOOLONG; creating a too-long name correctly fails with
   ENAMETOOLONG everywhere.) Cases: chmod/02 5, chown/02 5 and 10,
   ftruncate/02 5, truncate/02 5, link/02 10, link/03 10, rename/01 8,
   rmdir/02 4, unlink/02 4.
2. **Truncate leaves mtime unchanged.** `truncate()`, `ftruncate()` and
   `open(O_TRUNC)` update `st_ctime` but not `st_mtime`. Case: open/00 43
   (truncate/00 and ftruncate/00 only check ctime).
3. **Link count of an unlinked open file.** `fstat` after `unlink` reports
   `st_nlink` 1 instead of 0. Case: unlink/14 4.
4. **`posix_fallocate` does not change `st_size`.** Over FUSE,
   `fallocate(fd, 0, offset, len)` (what glibc's `posix_fallocate` calls)
   returns 0 and EdenFS extends the overlay file, but
   `OverlayFileAccess::fallocate` leaves the cached file size alone, so
   `st_size` stays at the old value whenever EdenFS had the size cached (a
   new empty file; any file after a `stat`). POSIX requires the size to
   become `offset + len`. Over NFS the client has no fallocate and glibc
   extends the file itself. Case: `test_posix_fallocate` (the size check is
   left out over FUSE).
5. **One `fallocate` with a mode disables FUSE_FALLOCATE for the mount.**
   `FuseChannel::fuseFallocate` answers ENOSYS for `mode != 0`
   (`FALLOC_FL_KEEP_SIZE`, `FALLOC_FL_PUNCH_HOLE`); the kernel then stops
   sending FUSE_FALLOCATE for the life of the mount, and glibc falls back to
   its `posix_fallocate` emulation (one write per block) for every
   later call on that mount, which is what the handler exists to avoid. Not
   a POSIX deviation (the caller sees EOPNOTSUPP, then a working
   `posix_fallocate`), so `test_fallocate_keep_size` pins what the caller
   sees and notes the side effect.
6. **Over NFS, `chmod` accepts the bits FUSE refuses, and loses the sticky
   bit.** The NFS server stores S_ISUID, S_ISGID and S_ISVTX from a SETATTR
   (FUSE answers EPERM, see below); S_ISUID and S_ISGID are then reported,
   but `modeToNfsMode` never reports S_ISVTX, so `chmod(dir, 0o1755)`
   silently yields 0o755. Cases: `test_exist_ok_s_isgid_directory` runs
   CPython's setgid flow over NFS; the sticky bit has no test.

### 2. EdenFS by design

* **No hard links.** `link()` fails with EPERM over FUSE and with
  `NFS3ERR_NOTSUPP` over NFS (the Linux client turns that into errno 524,
  `ENOTSUPP`). `expect 0 link` steps and what follows from them are left
  out; link/00 keeps the `nlink == 1` checks and the error paths. The CPython
  tests that only create a hard link (`test_link_dir_fd`, `LinkTests`) pin
  the error.
* **No setuid, setgid or sticky bits.** `chmod` with any of them fails with
  EPERM over FUSE, and `open(O_CREAT)`, `mknod` and `mkfifo` strip them from
  the mode of a new file (the mounts are `nosuid`). chmod/11 is not ported
  for this reason; `test_exist_ok_s_isgid_directory` pins the EPERM. (A
  `mkdir(0o2755)` that yields 0o755 is Linux, not EdenFS: the VFS masks the
  mode of a new directory to 0o1777 on every file system.)
* **UTF-8 names only.** A name that is not valid UTF-8 fails with EILSEQ
  over FUSE (`requireUtf8Path`) and with EINVAL over NFS (`NFS3ERR_INVAL`),
  on `mkdir`, `open`, `stat` and every other call; a symlink *target* is
  stored as given. CPython's `Pep383Tests` cannot even create their
  directory; the port pins the error and runs the tests with the two UTF-8
  non-ASCII names.
* **No stored xattrs.** Over FUSE `setxattr` and `removexattr` fail with
  EOPNOTSUPP and `listxattr` is empty; `getxattr` answers `user.sha1` and
  `user.blake3` on regular files (the content hashes EdenFS keeps, also
  through a descriptor or a followed symlink) and ENODATA for everything
  else, including those two names on a directory or on the symlink itself.
  Over NFS (v3 has no xattr protocol) `getxattr`, `setxattr` and
  `removexattr` fail with EOPNOTSUPP in the client and `listxattr` is
  empty. `ExtendedAttributeTests` skips itself; the port pins all of this,
  and `test_getxattr_user_sha1`/`test_getxattr_user_blake3` pin the two
  hashes as a contract.
* **No O_TMPFILE.** `open(dir, O_TMPFILE | O_RDWR)` fails with EOPNOTSUPP
  (FUSE has no operation for the Linux-only anonymous file; the NFS client
  has no support either). Pinned by `test_open_o_tmpfile`.
* **No `renameat2` flags.** `FuseChannel` answers FUSE_RENAME2 with ENOSYS
  unless `experimental:fuse-rename-noreplace` is set, so the kernel fails
  `RENAME_NOREPLACE` and `RENAME_EXCHANGE` with EINVAL itself
  (`RENAME_NOREPLACE` onto an existing name gets EEXIST from the VFS
  first). `posix` has no `renameat2`; documented only.
* **`fallocate` modes other than 0.** Only mode 0, what `posix_fallocate`
  uses, is implemented; `FALLOC_FL_KEEP_SIZE` and `FALLOC_FL_PUNCH_HOLE`
  fail with EOPNOTSUPP (see bug 5 for the side effect). Pinned by
  `test_fallocate_keep_size`.
* **No holes.** `st_blocks` is `st_size` rounded up to 512-byte units
  (`FileInode::updateBlockCount`), so a sparse file made by `ftruncate` or
  `pwrite` past the end reports every block as allocated, and `SEEK_DATA` /
  `SEEK_HOLE` report no holes (EdenFS has no FUSE_LSEEK handler; the kernel
  answers SEEK_DATA with the position and SEEK_HOLE with the end of the
  file, which is what `test_fs_holes` accepts). POSIX leaves `st_blocks`
  to the implementation, and a lazy-loading file system has no allocation
  of its own to report. Pinned by `test_st_blocks`.
* **`rename` leaves the file's ctime alone.** POSIX requires only the
  parents' timestamps to change; Linux file systems also update the file's
  `st_ctime`, EdenFS does not. Documented only (rename/00 checks the
  parents).
* **atime moves only when a read reaches EdenFS.** A read served from the
  kernel page cache leaves `st_atime` as it is; a read that EdenFS performs
  (first read of a file, after `posix_fadvise(DONTNEED)`, with `O_DIRECT`)
  sets it to now, with no `relatime` rule. Documented only.
* **NFS specifics.** The EdenFS NFS server answers ACCESS with the union of
  the owner, group and other bits, so a mode like 0o007 lets the owner read
  and write while a mode with no bits at all is still refused; it performs
  no ownership check, so an unprivileged user can `chown` a file to root
  (`test_chown`, `test_fchown` and `test_lchown` pin this; `pathconf`
  nevertheless reports `_PC_CHOWN_RESTRICTED`); it ignores a `SETATTR` that
  changes only `atime` (utimensat/02 expects the atime unchanged over NFS);
  it stores 32-bit seconds, so 2^32 is clamped to 2^32 - 1 (utimensat/09,
  `test_large_time`), a negative time becomes 0, and a time within the first
  second of 1970 is stored as 0 (`test_utime_with_fd`). The Linux NFS
  client has no `fallocate` (glibc emulates `posix_fallocate`) and no
  xattrs (above).

### 3. Needs root

The tests run as one unprivileged user. Steps that need root are left out:
`chown` to another owner, block and character device nodes (`mknod b`/`c`),
and every step the script runs as another user (`pjdfstest -u 65534`). All EACCES and
sticky-directory scripts are built on the last one and are not ported
(`*/05.t`, `*/06.t`, open/06, open/07, open/08, rename/04, rename/05,
rename/09, rename/10, rename/21, rmdir/11, unlink/11, chmod/05, chmod/07,
chown/00, chown/05, chown/07, link/06, link/07, mknod/11, utimensat/06,
utimensat/07). In CPython, the `chown` to a large uid/gid in
`_test_all_chown_common` runs only as root and is left out.

Two cases are Linux differences, not EdenFS: unlink/08 expects `unlink` of a
directory to return EPERM or succeed, Linux returns EISDIR; utimensat/08
case 5 reads `st_birthtime`, which Linux does not have. In the CPython
ports, `utime` without times fills in the kernel's coarse clock, which can
be a few milliseconds behind `time.time()` (`test_utime_nofollow_symlinks`
allows 50 ms, as CPython's `test_utime_current` does), and
`mkdir(0o2755)` drops S_ISGID on every Linux file system (above).

Not run at all: scripts that exit immediately on Linux (`chflags/*` and
every script with `require chflags`, the FreeBSD `mdconfig` scripts,
`granular/*`, `posix_fallocate/00`, `utimensat/03`, `rename/22`), and the
`EFAULT` scripts (`*/1?.t` passing a NULL or invalid pointer), which pass on
EdenFS but cannot be written with the `posix` module. Of CPython, the
classes without a file system (uid/gid, spawn, exec, scheduler, signals,
pipes, pty, environ, `TestInvalidFD`, `URandom*`, `Termsize*`, `Eventfd*`,
`Memfd*`), the Windows and macOS classes, and `BytesWalkTests` /
`BytesFwalkTests` (the `WalkTests` with bytes paths; `test_listdir_bytes`,
`test_bytes` and `Pep383Tests.test_listdir` cover bytes names) are not
ported.

## Script by script

Each script links to its source at the pjdfstest commit the port is based
on. "bug 1/2/3" are the EdenFS bugs above; "dependent" are checks that only
observe the effect of another left-out step.

| script | test method | cases ported | cases left out |
|---|---|---|---|
| [chmod/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/00.t) | `test_chmod_00_changes_permission` | 74/119 | needs root: 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 74, 75, 76, 77, 78, 79, 80, 81, 87, 91, 95, 98, 99, 100, 101, 102, 103, 104, 105, 107, 111, 112, 114, 116; dependent: 113 |
| [chmod/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/01.t) | `test_chmod_01_enotdir_component_prefix_directory` | 11/17 | needs root: 8, 9, 10, 11, 12, 13 |
| [chmod/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/02.t) | `test_chmod_02_enametoolong_name_max` | 4/5 | bug 1: 5 |
| [chmod/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/03.t) | `test_chmod_03_enametoolong_path_max` | 5/5 |  |
| [chmod/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/04.t) | `test_chmod_04_enoent` | 7/7 |  |
| [chmod/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/05.t) | — | 0/14 | not ported: every check needs root |
| [chmod/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/06.t) | `test_chmod_06_eloop_too_many_symbolic` | 8/8 |  |
| [chmod/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/07.t) | — | 0/25 | not ported: every check needs root |
| [chmod/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/11.t) | — | 0/109 | not ported: every check needs root or sets a sticky bit |
| [chmod/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chmod/12.t) | `test_chmod_12_verify_suid_sgid_bit` | 11/14 | needs root: 3, 7, 11 |
| [chown/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/00.t) | — | 0/1280 | not ported: every check needs root |
| [chown/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/01.t) | `test_chown_01_enotdir_component_prefix_directory` | 14/22 | needs root: 10, 11, 12, 13, 14, 15, 16, 17 |
| [chown/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/02.t) | — | 0/10 | not ported: every check needs root or hits the too-long-name bug |
| [chown/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/03.t) | `test_chown_03_enametoolong_path_max` | 6/10 | needs root: 2, 7; dependent: 3, 8 |
| [chown/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/04.t) | `test_chown_04_enoent` | 9/9 |  |
| [chown/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/05.t) | — | 0/18 | not ported: every check needs root |
| [chown/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/06.t) | `test_chown_06_eloop_too_many_symbolic` | 10/10 |  |
| [chown/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/chown/07.t) | — | 0/132 | not ported: every check needs root |
| [ftruncate/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/00.t) | `test_ftruncate_00_decrease_increase_size` | 23/26 | needs root: 18, 24; dependent: 25 |
| [ftruncate/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/01.t) | `test_ftruncate_01_enotdir_component_prefix_directory` | 5/5 |  |
| [ftruncate/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/02.t) | `test_ftruncate_02_enametoolong_name_max` | 4/5 | bug 1: 5 |
| [ftruncate/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/03.t) | `test_ftruncate_03_enametoolong_path_max` | 5/5 |  |
| [ftruncate/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/04.t) | `test_ftruncate_04_enoent` | 4/4 |  |
| [ftruncate/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/05.t) | — | 0/15 | not ported: every check needs root |
| [ftruncate/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/06.t) | — | 0/8 | not ported: every check needs root |
| [ftruncate/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/07.t) | `test_ftruncate_07_eloop_too_many_symbolic` | 6/6 |  |
| [ftruncate/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/09.t) | `test_ftruncate_09_eisdir_directory` | 3/3 |  |
| [ftruncate/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/12.t) | `test_ftruncate_12_efbig_einval_maximum_file_size` | 3/3 |  |
| [ftruncate/13.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/13.t) | `test_ftruncate_13_einval_length_less` | 4/4 |  |
| [truncate/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/00.t) | `test_truncate_00_decrease_increase_size` | 20/21 | needs root: 18 |
| [truncate/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/01.t) | `test_truncate_01_enotdir_component_prefix_directory` | 5/5 |  |
| [truncate/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/02.t) | `test_truncate_02_enametoolong_name_max` | 4/5 | bug 1: 5 |
| [truncate/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/03.t) | `test_truncate_03_enametoolong_path_max` | 5/5 |  |
| [truncate/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/04.t) | `test_truncate_04_enoent` | 4/4 |  |
| [truncate/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/05.t) | — | 0/15 | not ported: every check needs root |
| [truncate/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/06.t) | — | 0/8 | not ported: every check needs root |
| [truncate/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/07.t) | `test_truncate_07_eloop_too_many_symbolic` | 6/6 |  |
| [truncate/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/09.t) | `test_truncate_09_eisdir_directory` | 3/3 |  |
| [truncate/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/12.t) | `test_truncate_12_efbig_einval_maximum_file_size` | 3/3 |  |
| [truncate/13.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/13.t) | `test_truncate_13_einval_length_less` | 4/4 |  |
| [link/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/00.t) | `test_link_00_creates_hardlinks` | 50/202 | no hard links: 4, 7, 30, 33, 108, 111, 133, 140, 161; dependent: 5, 6, 8, 9, 10, 11, 13, 14, 15, 18, 19, 20, 22, 24, 31, 32, 34, 35, 36, 37, 39, 40, 41, 44, 45, 46, 48, 50, 109, 110, 112, 113, 114, 115, 117, 118, 119, 122, 123, 124, 126, 128, 134, 135, 136, 138, 141, 142, 143, 145, 162, 163, 164, 166; needs root: 12, 38, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 116, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 168, 169, 175, 176, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 196, 197 |
| [link/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/01.t) | `test_link_01_enotdir_component_either_prefix` | 20/32 | needs root: 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25 |
| [link/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/02.t) | `test_link_02_enametoolong_name_max` | 5/10 | no hard links: 2, 4; dependent: 5, 6; bug 1: 10 |
| [link/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/03.t) | `test_link_03_enametoolong_path_max` | 3/13 | no hard links: 2, 6; dependent: 3, 4, 7, 8, 9, 11, 12; bug 1: 10 |
| [link/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/04.t) | `test_link_04_enoent_component_either_prefix` | 6/6 |  |
| [link/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/06.t) | — | 0/18 | not ported: every check needs root |
| [link/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/07.t) | — | 0/17 | not ported: every check needs root |
| [link/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/08.t) | `test_link_08_eloop_too_many_symbolic` | 10/10 |  |
| [link/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/09.t) | `test_link_09_enoent_source` | 3/5 | no hard links: 2; dependent: 4 |
| [link/10.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/10.t) | `test_link_10_eexist_destination` | 17/23 | needs root: 11, 12, 13, 14, 15, 16 |
| [link/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/link/11.t) | `test_link_11_eperm_source_directory` | 5/9 | needs root: 5, 6, 7, 8 |
| [mkdir/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/00.t) | `test_mkdir_00_creates_directories` | 25/36 | needs root: 17, 18, 21, 25, 28; dependent: 19, 20, 22, 23, 26, 27 |
| [mkdir/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/01.t) | `test_mkdir_01_enotdir_component_prefix_directory` | 11/17 | needs root: 8, 9, 10, 11, 12, 13 |
| [mkdir/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/02.t) | `test_mkdir_02_enametoolong_name_max` | 3/3 |  |
| [mkdir/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/03.t) | `test_mkdir_03_enametoolong_path_max` | 3/3 |  |
| [mkdir/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/04.t) | `test_mkdir_04_enoent_component_prefix` | 3/3 |  |
| [mkdir/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/05.t) | — | 0/12 | not ported: every check needs root |
| [mkdir/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/06.t) | — | 0/12 | not ported: every check needs root |
| [mkdir/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/07.t) | `test_mkdir_07_eloop_too_many_symbolic` | 6/6 |  |
| [mkdir/10.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkdir/10.t) | `test_mkdir_10_eexist` | 15/21 | needs root: 10, 11, 12, 13, 14, 15 |
| [mkfifo/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/00.t) | `test_mkfifo_00_creates_fifo` | 25/36 | needs root: 17, 18, 21, 25, 28; dependent: 19, 20, 22, 23, 26, 27 |
| [mkfifo/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/01.t) | `test_mkfifo_01_enotdir_component_prefix_directory` | 11/17 | needs root: 8, 9, 10, 11, 12, 13 |
| [mkfifo/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/02.t) | `test_mkfifo_02_enametoolong_name_max` | 4/4 |  |
| [mkfifo/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/03.t) | `test_mkfifo_03_enametoolong_path_max` | 4/4 |  |
| [mkfifo/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/04.t) | `test_mkfifo_04_enoent_component_prefix` | 3/3 |  |
| [mkfifo/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/05.t) | — | 0/12 | not ported: every check needs root |
| [mkfifo/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/06.t) | — | 0/12 | not ported: every check needs root |
| [mkfifo/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/07.t) | `test_mkfifo_07_eloop_too_many_symbolic` | 6/6 |  |
| [mkfifo/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mkfifo/09.t) | `test_mkfifo_09_eexist` | 15/21 | needs root: 10, 11, 12, 13, 14, 15 |
| [mknod/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/00.t) | `test_mknod_00_creates_fifo` | 25/36 | needs root: 17, 18, 21, 25, 28; dependent: 19, 20, 22, 23, 26, 27 |
| [mknod/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/01.t) | `test_mknod_01_enotdir_component_prefix_directory` | 17/27 | needs root: 12, 13, 14, 15, 16, 17, 18, 19, 20, 21 |
| [mknod/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/02.t) | `test_mknod_02_enametoolong_name_max` | 5/12 | needs root: 5, 8, 9; dependent: 6, 7, 10, 11 |
| [mknod/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/03.t) | `test_mknod_03_enametoolong_path_max` | 6/12 | needs root: 5, 9; dependent: 6, 7, 10, 11 |
| [mknod/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/04.t) | `test_mknod_04_enoent_component_prefix` | 3/3 |  |
| [mknod/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/05.t) | — | 0/12 | not ported: every check needs root |
| [mknod/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/06.t) | — | 0/12 | not ported: every check needs root |
| [mknod/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/07.t) | `test_mknod_07_eloop_too_many_symbolic` | 6/6 |  |
| [mknod/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/08.t) | `test_mknod_08_eexist` | 25/35 | needs root: 16, 17, 18, 19, 20, 21, 22, 23, 24, 25 |
| [mknod/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/mknod/11.t) | — | 0/28 | not ported: every check needs root |
| [open/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/00.t) | `test_open_00_opens_eventually_creates` | 35/47 | needs root: 17, 18, 21, 25, 28; dependent: 19, 20, 22, 23, 26, 27; bug 2: 43 |
| [open/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/01.t) | `test_open_01_enotdir_component_prefix_directory` | 14/22 | needs root: 10, 11, 12, 13, 14, 15, 16, 17 |
| [open/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/02.t) | `test_open_02_enametoolong_name_max` | 4/4 |  |
| [open/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/03.t) | `test_open_03_enametoolong_path_max` | 4/4 |  |
| [open/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/04.t) | `test_open_04_enoent_component_name_must` | 4/4 |  |
| [open/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/05.t) | — | 0/12 | not ported: every check needs root |
| [open/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/06.t) | — | 0/144 | not ported: every check needs root |
| [open/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/07.t) | — | 0/25 | not ported: every check needs root |
| [open/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/08.t) | — | 0/3 | not ported: every check needs root |
| [open/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/12.t) | `test_open_12_eloop_too_many_symbolic` | 6/6 |  |
| [open/16.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/16.t) | `test_open_16_eloop_o_nofollow_target_symbolic` | 6/6 |  |
| [open/17.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/17.t) | `test_open_17_enxio_o_nonblock_fifo_o_wrongly` | 3/3 |  |
| [open/22.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/22.t) | `test_open_22_eexist_o_creat_o_excl` | 15/21 | needs root: 10, 11, 12, 13, 14, 15 |
| [open/23.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/23.t) | `test_open_23_einval_attempt_made_open` | 5/5 |  |
| [open/24.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/24.t) | `test_open_24_enxio_trying_open_unix` | 5/5 |  |
| [open/25.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/25.t) | `test_open_25_files_larger_than_2gb` | 6/6 |  |
| [open/26.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/26.t) | `test_open_26_can_create_0000_mode` | 9/9 |  |
| [rename/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/00.t) | `test_rename_00_changes_name` | 61/122 | no hard links: 8, 23, 68; dependent: 9, 10, 12, 14, 15, 24, 25, 27, 29, 30, 69, 70, 72, 74, 75; needs root: 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 95, 99, 103, 106, 107, 108, 109, 110, 111, 112, 113, 115, 119 |
| [rename/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/01.t) | `test_rename_01_enametoolong_name_max` | 7/8 | bug 1: 8 |
| [rename/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/02.t) | `test_rename_02_enametoolong_path_max` | 6/6 |  |
| [rename/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/03.t) | `test_rename_03_enoent_component_from_prefix` | 6/6 |  |
| [rename/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/04.t) | — | 0/18 | not ported: every check needs root |
| [rename/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/05.t) | — | 0/17 | not ported: every check needs root |
| [rename/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/09.t) | — | 0/2353 | not ported: every check needs root |
| [rename/10.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/10.t) | — | 0/2099 | not ported: every check needs root |
| [rename/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/11.t) | `test_rename_11_eloop_too_many_symbolic` | 10/10 |  |
| [rename/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/12.t) | `test_rename_12_enotdir_component_either_prefix` | 20/32 | needs root: 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25 |
| [rename/13.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/13.t) | `test_rename_13_enotdir_from_directory_but` | 22/32 | needs root: 12, 13, 14, 15, 16, 17, 18, 19, 20, 21 |
| [rename/14.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/14.t) | `test_rename_14_eisdir_directory_but_from` | 22/32 | needs root: 12, 13, 14, 15, 16, 17, 18, 19, 20, 21 |
| [rename/18.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/18.t) | `test_rename_18_einval_from_parent_directory` | 6/6 |  |
| [rename/19.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/19.t) | `test_rename_19_einval_ebusy_attempt_made_rename` | 6/6 |  |
| [rename/20.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/20.t) | `test_rename_20_eexist_enotempty_directory_empty` | 19/25 | needs root: 12, 13, 14, 15, 16, 17 |
| [rename/21.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/21.t) | — | 0/16 | not ported: every check needs root |
| [rename/23.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/23.t) | `test_rename_23_succeeds_multiply_linked` | 14/42 | no hard links: 4, 12, 36; dependent: 6, 7, 9, 14, 15, 17, 38, 39, 41; needs root: 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33 |
| [rename/24.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rename/24.t) | `test_rename_24_directory_updates_link` | 13/13 |  |
| [rmdir/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/00.t) | `test_rmdir_00_removes_directories` | 10/10 |  |
| [rmdir/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/01.t) | `test_rmdir_01_enotdir_component_directory` | 14/14 |  |
| [rmdir/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/02.t) | `test_rmdir_02_enametoolong_name_max` | 3/4 | bug 1: 4 |
| [rmdir/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/03.t) | `test_rmdir_03_enametoolong_path_max` | 5/5 |  |
| [rmdir/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/04.t) | `test_rmdir_04_enoent_directory` | 4/4 |  |
| [rmdir/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/05.t) | `test_rmdir_05_eloop_too_many_symbolic` | 6/6 |  |
| [rmdir/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/06.t) | `test_rmdir_06_eexist_enotempty_directory_contains` | 17/23 | needs root: 11, 12, 13, 14, 15, 16 |
| [rmdir/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/07.t) | — | 0/10 | not ported: every check needs root |
| [rmdir/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/08.t) | — | 0/10 | not ported: every check needs root |
| [rmdir/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/11.t) | — | 0/47 | not ported: every check needs root |
| [rmdir/12.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/rmdir/12.t) | `test_rmdir_12_einval_last_component_eexist` | 6/6 |  |
| [symlink/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/00.t) | `test_symlink_00_creates_symbolic_links` | 14/14 |  |
| [symlink/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/01.t) | `test_symlink_01_enotdir_component_name_prefix` | 5/5 |  |
| [symlink/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/02.t) | `test_symlink_02_enametoolong_name_max` | 7/7 |  |
| [symlink/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/03.t) | `test_symlink_03_enametoolong_path_max` | 6/6 |  |
| [symlink/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/04.t) | `test_symlink_04_enoent_component_name_prefix` | 3/3 |  |
| [symlink/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/05.t) | — | 0/12 | not ported: every check needs root |
| [symlink/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/06.t) | — | 0/12 | not ported: every check needs root |
| [symlink/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/07.t) | `test_symlink_07_eloop_too_many_symbolic` | 6/6 |  |
| [symlink/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/symlink/08.t) | `test_symlink_08_eexist_name_already` | 15/21 | needs root: 10, 11, 12, 13, 14, 15 |
| [unlink/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/00.t) | `test_unlink_00_removes_regular_symbolic_links` | 65/112 | needs root: 14, 18, 36, 41, 52, 56, 59, 60, 63, 64, 68, 84, 90; dependent: 15, 16, 19, 20, 28, 29, 33, 34, 38, 39, 40, 43, 44, 45, 48, 49, 61, 62, 65, 66, 85, 86, 87, 91, 92, 93, 109, 110; no hard links: 27, 32, 37, 42, 47, 108 |
| [unlink/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/01.t) | `test_unlink_01_enotdir_component_prefix_directory` | 5/5 |  |
| [unlink/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/02.t) | `test_unlink_02_enametoolong_name_max` | 3/4 | bug 1: 4 |
| [unlink/03.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/03.t) | `test_unlink_03_enametoolong_path_max` | 4/4 |  |
| [unlink/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/04.t) | `test_unlink_04_enoent` | 4/4 |  |
| [unlink/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/05.t) | — | 0/10 | not ported: every check needs root |
| [unlink/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/06.t) | — | 0/10 | not ported: every check needs root |
| [unlink/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/07.t) | `test_unlink_07_eloop_too_many_symbolic` | 6/6 |  |
| [unlink/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/08.t) | — | 0/3 | not ported: Linux returns EISDIR (the script marks this TODO) |
| [unlink/11.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/11.t) | — | 0/270 | not ported: every check needs root |
| [unlink/14.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/unlink/14.t) | `test_unlink_14_open_file_not_freed` | 6/7 | bug 3: 4 |
| [utimensat/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/00.t) | `test_utimensat_00_changes_timestamps_any_type` | 22/32 | needs root: 17, 18, 19, 20, 21, 22, 23, 24, 25, 26 |
| [utimensat/01.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/01.t) | `test_utimensat_01_utime_now_sets_timestamps_to_now` | 7/7 |  |
| [utimensat/02.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/02.t) | `test_utimensat_02_utime_omit_leaves_time_unchanged` | 10/10 |  |
| [utimensat/04.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/04.t) | `test_utimensat_04_can_mtime_atime_vice` | 10/10 |  |
| [utimensat/05.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/05.t) | `test_utimensat_05_can_follow_symlinks` | 16/16 |  |
| [utimensat/06.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/06.t) | — | 0/13 | not ported: every check needs root |
| [utimensat/07.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/07.t) | — | 0/17 | not ported: every check needs root |
| [utimensat/08.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/08.t) | `test_utimensat_08_subsecond_precision` | 8/9 | Linux: 5 |
| [utimensat/09.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/utimensat/09.t) | `test_utimensat_09_y2038_compliant` | 7/7 |  |

## CPython test by test

Each CPython test links to its source at the Python 3.12.15 tag. "bug 4/5/6"
are the EdenFS bugs above; "by design" says which deviation the method
pins; "Python API" means the test checks the Python binding (argument
types, `pickle`, `repr`, `FakePath`), not the file system. Tests of the
same name in different classes have the same method name in different
classes, except where the table says so.

| CPython test | method | status |
|---|---|---|
| [test_posix.PosixTester.test_statvfs](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_statvfs` | ported (`space_test.py`) |
| [test_posix.PosixTester.test_fstatvfs](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_fstatvfs` | ported (`space_test.py`) |
| [test_posix.PosixTester.test_ftruncate](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_ftruncate` | ported (`space_test.py`) |
| [test_posix.PosixTester.test_truncate](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_truncate` | ported (`space_test.py`) |
| [test_posix.PosixTester.test_lockf](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_lockf` | ported (`lock_test.py`) |
| [test_posix.PosixTester.test_pread](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_pread` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_preadv](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_preadv` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_preadv_flags](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_preadv_flags` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_pwrite](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_pwrite` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_pwritev](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_pwritev` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_writev](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_writev` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_readv](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_readv` | ported (`io_test.py`) |
| [test_posix.PosixTester.test_pwritev_flags](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_pwritev_flags` | ported (`io_test.py`); CPython skips it everywhere (it looks for `os.RWF_SYNC` on `posix`) |
| [test_posix.PosixTester.test_preadv_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: 32-bit builds only |
| [test_posix.PosixTester.test_pwritev_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: 32-bit builds only |
| [test_posix.PosixTester.test_writev_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: 32-bit builds only |
| [test_posix.PosixTester.test_readv_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: 32-bit builds only |
| [test_posix.PosixTester.test_posix_fallocate](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_posix_fallocate` | ported (`space_test.py`); bug 4: the size check is left out over FUSE |
| [test_posix.PosixTester.test_posix_fadvise](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_posix_fadvise` | ported (`space_test.py`) |
| [test_posix.PosixTester.test_posix_fallocate_errno](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system (bad descriptor) |
| [test_posix.PosixTester.test_posix_fadvise_errno](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system (bad descriptor) |
| [test_posix.PosixTester.test_utime_with_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_utime_with_fd` | ported (`fd_test.py`); NFS: a time within the first second of 1970 is stored as 0 |
| [test_posix.PosixTester.test_utime_nofollow_symlinks](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_utime_nofollow_symlinks` | ported (`fd_test.py`, `FdTest`); the `TypeError` checks are left out |
| [test_posix.PosixTester.test_utime](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_utime` | ported (`fd_test.py`, `FdTest`); the `TypeError` checks are left out |
| [test_posix.PosixTester.test_dup](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_dup` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_dup2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_dup2` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_oscloexec](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_oscloexec` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_fstat](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_fstat` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_chdir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chdir` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_stat](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_stat` | ported (`fd_test.py`); the `TypeError` checks are left out |
| [test_posix.PosixTester.test_osexlock](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`O_EXLOCK`, `O_SHLOCK`) |
| [test_posix.PosixTester.test_osshlock](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`O_EXLOCK`, `O_SHLOCK`) |
| [test_posix.PosixTester.test_mkfifo](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: pjdfstest mkfifo/00 and mknod/00 cover it |
| [test_posix.PosixTester.test_mknod](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: pjdfstest mkfifo/00 and mknod/00 cover it |
| [test_posix.PosixTester.test_makedev](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: Python API |
| [test_posix.PosixTester.test_path_error2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: Python API |
| [test_posix.PosixTester.test_listdir_bytes_like](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: Python API |
| [test_posix.PosixTester.test_chown](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chown` | ported (`fd_test.py`); needs root: the large uid/gid part; NFS: by design, `chown` to root succeeds |
| [test_posix.PosixTester.test_fchown](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_fchown` | ported (`fd_test.py`); needs root: the large uid/gid part; NFS: by design, `chown` to root succeeds |
| [test_posix.PosixTester.test_lchown](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_lchown` | ported (`fd_test.py`); needs root: the large uid/gid part; NFS: by design, `chown` to root succeeds |
| [test_posix.PosixTester.test_listdir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_listdir` | ported (`scandir_test.py`, `ListdirTest`) |
| [test_posix.PosixTester.test_listdir_default](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_listdir_default` | ported (`scandir_test.py`, `ListdirTest`) |
| [test_posix.PosixTester.test_listdir_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_listdir_bytes` | ported (`scandir_test.py`, `ListdirTest`) |
| [test_posix.PosixTester.test_listdir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_listdir_fd` | ported (`scandir_test.py`, `ListdirTest`) |
| [test_posix.PosixTester.test_access](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_access_r_ok` | ported (`fd_test.py`); renamed because `FileTests.test_access` is in the same class |
| [test_posix.PosixTester.test_chmod_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chmod_file` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_chmod_dir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chmod_dir` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_chmod_file_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chmod_file_symlink` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_chmod_dir_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chmod_dir_symlink` | ported (`fd_test.py`) |
| [test_posix.PosixTester.test_lchmod_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_lchmod_dir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_lchmod_file_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_lchmod_dir_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_chflags](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_lchflags_regular_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_lchflags_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: BSD or macOS only (`lchmod`, `chflags`) |
| [test_posix.PosixTester.test_getcwd_long_pathnames](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_getcwd_long_pathnames` | ported (`names_test.py`); CPython's nested helper is unreachable, the port does what it describes |
| [test_posix.PosixTester.test_fs_holes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_fs_holes` | ported (`space_test.py`); by design: no holes are reported |
| [test_posix.PosixTester.test_path_with_null_character](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_path_with_null_character` | ported (`names_test.py`) |
| [test_posix.PosixTester.test_path_with_null_byte](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_path_with_null_byte` | ported (`names_test.py`) |
| [test_posix.PosixTester.testNoArgFunctions](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_getresuid](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_getresgid](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_setresuid](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_setresuid_exception](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_setresgid](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_setresgid_exception](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_initgroups](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_fexecve](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_waitid](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_register_at_fork](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_confstr](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_umask](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_strerror](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_pipe](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_pipe2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_pipe2_c_limits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_environ](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_putenv](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_getgrouplist](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_getgroups](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_cld_xxxx_constants](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_yield](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_priority](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_get_and_set_scheduler_and_param](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_param](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_rr_get_interval](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_getaffinity](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_sched_setaffinity](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_rtld_constants](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.PosixTester.test_pidfd_open](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | — | not ported: no file system |
| [test_posix.TestPosixDirFd.test_access_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_access_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_chmod_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chmod_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_chown_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_chown_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_utime_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_utime_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_mkdir_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_mkdir_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_mknod_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_mknod_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_open_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_open_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_readlink_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_readlink_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_rename_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_rename_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_symlink_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_symlink_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_unlink_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_unlink_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_mkfifo_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_mkfifo_dir_fd` | ported (`dirfd_test.py`) |
| [test_posix.TestPosixDirFd.test_stat_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_stat_dir_fd` | ported (`dirfd_test.py`); the `TypeError`/`OverflowError` checks are left out |
| [test_posix.TestPosixDirFd.test_link_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_posix.py) | `test_link_dir_fd` | ported (`dirfd_test.py`); by design: no hard links, the error is pinned |
| [test_os.MiscTests.test_getcwd_long_path](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_getcwd_long_path` | ported (`names_test.py`) |
| [test_os.MiscTests.test_getcwdb](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_getcwdb` | ported (`names_test.py`) |
| [test_os.MiscTests.test_getcwd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_access](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_access` | ported (`fd_test.py`) |
| [test_os.FileTests.test_closerange](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_closerange` | ported (`fd_test.py`) |
| [test_os.FileTests.test_replace](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_replace` | ported (`fd_test.py`) |
| [test_os.FileTests.test_read](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_read` | ported (`io_test.py`) |
| [test_os.FileTests.test_write](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_write` | ported (`io_test.py`) |
| [test_os.FileTests.test_copy_file_range](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_copy_file_range` | ported (`io_test.py`) |
| [test_os.FileTests.test_copy_file_range_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_copy_file_range_offset` | ported (`io_test.py`) |
| [test_os.FileTests.test_splice](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_splice` | ported (`io_test.py`) |
| [test_os.FileTests.test_splice_offset_in](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_splice_offset_in` | ported (`io_test.py`) |
| [test_os.FileTests.test_splice_offset_out](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_splice_offset_out` | ported (`io_test.py`) |
| [test_os.FileTests.test_rename](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_fdopen](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_open_keywords](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_symlink_keywords](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_copy_file_range_invalid_values](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_splice_invalid_values](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.FileTests.test_large_read](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: needs 2 GiB of memory |
| [test_os.FileTests.test_write_windows_console](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.StatAttributeTests.test_stat_attributes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_stat_attributes` | ported (`fd_test.py`); the `stat_result` type checks are left out |
| [test_os.StatAttributeTests.test_stat_attributes_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_stat_attributes_bytes` | ported (`fd_test.py`); the `stat_result` type checks are left out |
| [test_os.StatAttributeTests.test_statvfs_attributes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_statvfs_attributes` | ported (`space_test.py`); the `statvfs_result` type checks are left out |
| [test_os.StatAttributeTests.test_stat_result_pickle](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.StatAttributeTests.test_statvfs_result_pickle](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.StatAttributeTests.test_1686475](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.StatAttributeTests.test_15261](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.StatAttributeTests.test_file_attributes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.StatAttributeTests.test_access_denied](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.StatAttributeTests.test_stat_block_device](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.UtimeTests.test_utime](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_by_indexed](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_by_indexed` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_by_times](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_by_times` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_nofollow_symlinks](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_nofollow_symlinks` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_fd` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_dir_fd` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_directory](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_directory` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_current](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_current` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_utime_current_old](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_utime_current_old` | ported (`fd_test.py`, `UtimeTest`) |
| [test_os.UtimeTests.test_large_time](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_large_time` | ported (`fd_test.py`, `UtimeTest`); CPython runs it on NTFS only; NFS: by design, clamped to 2^32 - 1 |
| [test_os.UtimeTests.test_utime_invalid_arguments](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.UtimeTests.test_issue31577](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.WalkTests.test_walk_topdown](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_topdown` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_prune](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_prune` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_bottom_up](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bottom_up` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_symlink` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_bad_dir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bad_dir` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_bad_dir2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bad_dir2` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_named_pipe](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_named_pipe` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_named_pipe2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_named_pipe2` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_walk_many_open_files](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_many_open_files` | ported (`scandir_test.py`, `WalkTest`); NFS: by design, the unreadable directory is readable and drops out, as in CPython |
| [test_os.WalkTests.test_file_like_path](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API (`FakePath`, recursion limit) |
| [test_os.WalkTests.test_walk_above_recursion_limit](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API (`FakePath`, recursion limit) |
| [test_os.FwalkTests.test_walk_topdown](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_topdown` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_prune](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_prune` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_bottom_up](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bottom_up` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_symlink` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_bad_dir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bad_dir` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_bad_dir2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_bad_dir2` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_named_pipe](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_named_pipe` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_walk_named_pipe2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_walk_named_pipe2` | ported (`scandir_test.py`, `FwalkTest`; inherited from `WalkTests`) |
| [test_os.FwalkTests.test_compare_to_walk](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_compare_to_walk` | ported (`scandir_test.py`, `FwalkTest`) |
| [test_os.FwalkTests.test_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_dir_fd` | ported (`scandir_test.py`, `FwalkTest`) |
| [test_os.FwalkTests.test_yields_correct_dir_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_yields_correct_dir_fd` | ported (`scandir_test.py`, `FwalkTest`) |
| [test_os.FwalkTests.test_fd_leak](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_fd_leak` | ported (`scandir_test.py`, `FwalkTest`) |
| [test_os.FwalkTests.test_fd_finalization](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API (generator close) |
| [test_os.BytesWalkTests](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: `WalkTests` with bytes paths |
| [test_os.BytesFwalkTests](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: `FwalkTests` with bytes paths |
| [test_os.MakedirTests.test_makedir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_makedir` | ported (`scandir_test.py`, `MakedirTest`) |
| [test_os.MakedirTests.test_mode](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_mode` | ported (`scandir_test.py`, `MakedirTest`) |
| [test_os.MakedirTests.test_exist_ok_existing_directory](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_exist_ok_existing_directory` | ported (`scandir_test.py`, `MakedirTest`) |
| [test_os.MakedirTests.test_exist_ok_existing_regular_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_exist_ok_existing_regular_file` | ported (`scandir_test.py`, `MakedirTest`) |
| [test_os.MakedirTests.test_exist_ok_s_isgid_directory](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_exist_ok_s_isgid_directory` | ported (`scandir_test.py`, `MakedirTest`); by design: EPERM pinned over FUSE; NFS: CPython's flow (bug 6) |
| [test_os.MakedirTests.test_win32_mkdir_700](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.RemoveDirsTests.test_remove_all](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_remove_all` | ported (`scandir_test.py`, `RemoveDirsTest`) |
| [test_os.RemoveDirsTests.test_remove_partial](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_remove_partial` | ported (`scandir_test.py`, `RemoveDirsTest`) |
| [test_os.RemoveDirsTests.test_remove_nothing](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_remove_nothing` | ported (`scandir_test.py`, `RemoveDirsTest`) |
| [test_os.TestInvalidFD](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (bad descriptor) |
| [test_os.LinkTests.test_link](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_link` | ported (`names_test.py`); by design: no hard links, the error is pinned |
| [test_os.LinkTests.test_link_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_link_bytes` | ported (`names_test.py`); by design: no hard links, the error is pinned |
| [test_os.LinkTests.test_unicode_name](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_unicode_name` | ported (`names_test.py`); by design: no hard links, the error is pinned |
| [test_os.Pep383Tests.test_listdir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_listdir` | ported (`names_test.py`, `Pep383Test`); by design: the non-UTF-8 name is pinned, the UTF-8 names run |
| [test_os.Pep383Tests.test_open](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_open` | ported (`names_test.py`, `Pep383Test`); by design: the non-UTF-8 name is pinned, the UTF-8 names run |
| [test_os.Pep383Tests.test_statvfs](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_statvfs` | ported (`names_test.py`, `Pep383Test`); by design: the non-UTF-8 name is pinned, the UTF-8 names run |
| [test_os.Pep383Tests.test_stat](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_stat` | ported (`names_test.py`, `Pep383Test`); by design: the non-UTF-8 name is pinned, the UTF-8 names run |
| [test_os.ReadlinkTests.test_not_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_not_symlink` | ported (`names_test.py`) |
| [test_os.ReadlinkTests.test_missing_link](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_missing_link` | ported (`names_test.py`) |
| [test_os.ReadlinkTests.test_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_bytes` | ported (`names_test.py`) |
| [test_os.ReadlinkTests.test_pathlike](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.ReadlinkTests.test_pathlike_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.NonLocalSymlinkTests.test_directory_link_nonlocal](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_directory_link_nonlocal` | ported (`names_test.py`) |
| [test_os.TestSendfile.test_send_whole_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_send_whole_file` | ported (`io_test.py`, `SendfileTest`) |
| [test_os.TestSendfile.test_send_at_certain_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_send_at_certain_offset` | ported (`io_test.py`, `SendfileTest`) |
| [test_os.TestSendfile.test_offset_overflow](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_offset_overflow` | ported (`io_test.py`, `SendfileTest`) |
| [test_os.TestSendfile.test_invalid_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_invalid_offset` | ported (`io_test.py`, `SendfileTest`) |
| [test_os.TestSendfile.test_keywords](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestSendfile.test_headers](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: BSD or macOS only (headers and trailers) |
| [test_os.TestSendfile.test_trailers](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: BSD or macOS only (headers and trailers) |
| [test_os.TestSendfile.test_headers_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: BSD or macOS only (headers and trailers) |
| [test_os.TestSendfile.test_trailers_overflow_32bits](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: BSD or macOS only (headers and trailers) |
| [test_os.TestSendfile.test_flags](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: BSD or macOS only (headers and trailers) |
| [test_os.ExtendedAttributeTests.test_simple](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_simple` | ported (`xattr_test.py`); by design: no stored xattrs, the errors are pinned |
| [test_os.ExtendedAttributeTests.test_lpath](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_lpath` | ported (`xattr_test.py`); by design: no stored xattrs, the errors are pinned |
| [test_os.ExtendedAttributeTests.test_fds](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_fds` | ported (`xattr_test.py`); by design: no stored xattrs, the errors are pinned |
| [test_os.FDInheritanceTests.test_get_set_inheritable_o_path](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_get_set_inheritable_o_path` | ported (`fd_test.py`) |
| [test_os.FDInheritanceTests.test_get_set_inheritable](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_get_inheritable_cloexec](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_set_inheritable_cloexec](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_get_set_inheritable_badf](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_open](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_pipe](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_dup](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_dup_standard_stream](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_dup2](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_openpty](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: no file system (descriptor flags) |
| [test_os.FDInheritanceTests.test_dup_nul](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_os.TestScandir.test_attributes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_attributes` | ported (`scandir_test.py`, `ScandirTest`); by design: the hard link is left out |
| [test_os.TestScandir.test_current_directory](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_current_directory` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_removed_dir](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_removed_dir` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_removed_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_removed_file` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_broken_symlink](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_broken_symlink` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_bytes` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_fd](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | `test_fd` | ported (`scandir_test.py`, `ScandirTest`) |
| [test_os.TestScandir.test_uninstantiable](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_unpickable](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_repr](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_fspath_protocol](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_fspath_protocol_bytes](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_bytes_like](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_empty_path](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_consume_iterator_twice](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_bad_path_type](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_close](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_context_manager](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_context_manager_close](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_context_manager_exception](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_resource_warning](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Python API |
| [test_os.TestScandir.test_attributes_junctions](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_os.py) | — | not ported: Windows only |
| [test_fcntl.TestFcntl.test_fcntl_fileno](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | `test_fcntl_fileno` | ported (`lock_test.py`) |
| [test_fcntl.TestFcntl.test_fcntl_64_bit](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | `test_fcntl_64_bit` | ported (`lock_test.py`) |
| [test_fcntl.TestFcntl.test_flock](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | `test_flock` | ported (`lock_test.py`) |
| [test_fcntl.TestFcntl.test_lockf_exclusive](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | `test_lockf_exclusive` | ported (`lock_test.py`) |
| [test_fcntl.TestFcntl.test_lockf_share](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | `test_lockf_share` | ported (`lock_test.py`) |
| [test_fcntl.TestFcntl.test_fcntl_file_descriptor](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: Python API |
| [test_fcntl.TestFcntl.test_fcntl_bad_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: Python API |
| [test_fcntl.TestFcntl.test_fcntl_bad_file_overflow](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: Python API |
| [test_fcntl.TestFcntl.test_flock_overflow](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: Python API |
| [test_fcntl.TestFcntl.test_fcntl_f_pipesize](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: no file system (pipe) |
| [test_fcntl.TestFcntl.test_fcntl_f_getpath](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_fcntl.py) | — | not ported: BSD or macOS only |
| [test_mmap.MmapTests.test_basic](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_basic` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_access_parameter](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_access_parameter` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_tougher_find](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_tougher_find` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_double_close](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_double_close` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_entire_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_entire_file` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_length_0_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_length_0_offset` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_empty_file](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_empty_file` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_offset` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_prot_readonly](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_prot_readonly` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_io_methods](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_io_methods` | ported (`mmap_test.py`) |
| [test_mmap.MmapTests.test_move](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | `test_move` | ported (`mmap_test.py`); the anonymous-mapping part is left out |
| [test_mmap.MmapTests.test_find_end](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Python API (on a file-backed map) |
| [test_mmap.MmapTests.test_rfind](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Python API (on a file-backed map) |
| [test_mmap.MmapTests.test_length_0_large_offset](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Python API (on a file-backed map) |
| [test_mmap.MmapTests.test_repr](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Python API (on a file-backed map) |
| [test_mmap.MmapTests.test_mmap_closed_by_int_scenarios](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Python API (on a file-backed map) |
| [test_mmap.MmapTests.test_bad_file_desc](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_find_does_not_access_beyond_buffer](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_anonymous](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_read_all](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_read_invalid_arg](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_extended_getslice](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_extended_set_del_slice](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_subclass](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_error](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_non_ascii_byte](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_context_manager](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_context_manager_exception](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_weakref](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_write_returning_the_number_of_bytes_written](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_resize_past_pos](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_concat_repeat_exception](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_flush_return_value](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_madvise](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: no file system (anonymous mapping) |
| [test_mmap.MmapTests.test_tagname](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_sizeof](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_crasher_on_windows](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_invalid_descriptor](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_resize_up_when_mapped_to_pagefile](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_resize_down_when_mapped_to_pagefile](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_resize_fails_if_mapping_held_elsewhere](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |
| [test_mmap.MmapTests.test_resize_succeeds_with_error_for_second_named_mapping](https://github.com/python/cpython/blob/v3.12.15/Lib/test/test_mmap.py) | — | not ported: Windows only |

EdenFS additions, without a CPython original:

| module | method | checks |
|---|---|---|
| `io_test.py` | `test_o_append` | O_APPEND writes at the end after an lseek |
| `space_test.py` | `test_fallocate_keep_size` | by design: `fallocate` modes other than 0 are EOPNOTSUPP (bug 5 noted) |
| `space_test.py` | `test_st_blocks` | by design: `st_blocks` follows `st_size` |
| `dirfd_test.py` | `test_rmdir_dir_fd` | `unlinkat` with `AT_REMOVEDIR` |
| `dirfd_test.py` | `test_stat_dir_fd_nofollow` | `fstatat` with `AT_SYMLINK_NOFOLLOW` |
| `fd_test.py` | `test_fchmod` | `fchmod` and `fstat` through a descriptor |
| `fd_test.py` | `test_fd_after_rename` | `fchmod`, `futimens`, `write` and `fstat` through a descriptor after the path was renamed |
| `fd_test.py` | `test_open_o_directory` | `O_DIRECTORY` |
| `fd_test.py` | `test_open_o_nofollow` | `O_NOFOLLOW`, alone and with `O_PATH` |
| `fd_test.py` | `test_open_o_tmpfile` | by design: `O_TMPFILE` is EOPNOTSUPP |
| `fd_test.py` | `test_open_o_direct` | `O_DIRECT` with an aligned buffer |
| `lock_test.py` | `test_f_getlk` | `F_GETLK` reports the other process's lock |
| `lock_test.py` | `test_flock_second_descriptor` | `flock` is per open file description, `lockf` per process |
| `mmap_test.py` | `test_map_shared_write_back` | a `MAP_SHARED` write reaches other readers, and a `pwrite` reaches the map |
| `mmap_test.py` | `test_map_private` | a `MAP_PRIVATE` write stays private |
| `names_test.py` | `test_symlink_target_not_utf8` | by design: a symlink target need not be UTF-8 |
| `xattr_test.py` | `test_getxattr_user_sha1` | by design: `user.sha1` is the SHA-1 of the content, pinned as a contract |
| `xattr_test.py` | `test_getxattr_user_blake3` | by design: `user.blake3` is a 64-digit hex hash, pinned as a contract |
