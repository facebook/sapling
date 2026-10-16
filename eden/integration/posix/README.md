# POSIX file system tests (ported from pjdfstest)

The tests in this directory are ports of the
[pjdfstest](https://github.com/pjd/pjdfstest) POSIX file system test suite
by Pawel Jakub Dawidek (BSD-2-Clause), at commit
`ededbeb2b44929972898afb87474b0937f78a877`, to the EdenFS integration test
framework.

Each `tests/<group>/<NN>.t` script of pjdfstest became one test method
`test_<group>_<NN>_<summary>` in `<group>_test.py` (`truncate_test.py` holds
`truncate/` and `ftruncate/`), with the script named in the docstring. The
table at the end of this file links every script to its method. Each method
runs in a fresh EdenFS mount in the `Default` (FUSE), `DefaultIoUring` and
`NFS` variants.

```
buck2 test '@fbcode//mode/opt' fbcode//eden/integration/posix/...
```

## Translation

The tests call the `posix` module, Python's direct binding of the POSIX
system calls, and use plain `unittest` assertions. (`os` re-exports `posix`
on POSIX systems, but also layers portable helpers on top; the tests stay
with the raw calls.) `base.py` provides the setup every script relies on: an
empty scratch directory inside the mount as the working directory, `umask 0`,
and a few small helpers (`assert_errno`; `bind_unix_socket` for
`create_file socket`; `mkdir_p` and `rm_rf` for the shell commands of the
same name; `name_max` and `path_max` for `namegen_max` and `dirgen_max`;
`utimensat` for `UTIME_OMIT`, which `posix.utime` cannot express). File
names are the script's variable names (`n0`, `n1`, ...).

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

## Which cases are ported

Every script was run with the real `pjdfstest` binary (`prove -v`) on a FUSE
mount of the EdenFS daemon, as an unprivileged user. The steps that cannot
run in that setting were removed, the trimmed scripts were replayed with
`pjdfstest` until every remaining case passed, and that set was translated.
Of the 154 scripts that do anything on Linux, 114 are ported, with 1,232 of
their 1,813 cases; 40 scripts have no check left and are not ported.

A left-out case is marked in place, for example `# cases 17-23: needs root`;
the numbers are the TAP case numbers the script prints on Linux. There are
three reasons, below. A check that only observes the effect of a left-out
step (`lstat uid,gid` after a `chown`, `nlink` after a `link`) is marked
`depends on a step above` and counts with that step's reason.

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

### 2. EdenFS by design

* **No hard links.** `link()` fails with EPERM over FUSE and with
  `NFS3ERR_NOTSUPP` over NFS (the Linux client turns that into errno 524,
  `ENOTSUPP`). `expect 0 link` steps and what follows from them are left
  out; link/00 keeps the `nlink == 1` checks and the error paths.
* **No setuid, setgid or sticky bits.** `chmod` with any of them fails with
  EPERM. chmod/11 is not ported for this reason.
* **NFS specifics.** The EdenFS NFS server does not enforce permission bits
  (it answers ACCESS with the most permissive rights of any class), ignores
  a `SETATTR` that changes only `atime` (utimensat/02 expects the atime
  unchanged over NFS), and stores 32-bit seconds (utimensat/09 expects 2^32
  clamped to 2^32 - 1 over NFS).

### 3. Needs root

The tests run as one unprivileged user. Steps that need root are left out:
`chown` to another owner, block and character device nodes (`mknod b`/`c`),
and every step the script runs as another user (`pjdfstest -u 65534`). All EACCES and
sticky-directory scripts are built on the last one and are not ported
(`*/05.t`, `*/06.t`, open/06, open/07, open/08, rename/04, rename/05,
rename/09, rename/10, rename/21, rmdir/11, unlink/11, chmod/05, chmod/07,
chown/00, chown/05, chown/07, link/06, link/07, mknod/11, utimensat/06,
utimensat/07).

Two cases are Linux differences, not EdenFS: unlink/08 expects `unlink` of a
directory to return EPERM or succeed, Linux returns EISDIR; utimensat/08
case 5 reads `st_birthtime`, which Linux does not have.

Not run at all: scripts that exit immediately on Linux (`chflags/*` and
every script with `require chflags`, the FreeBSD `mdconfig` scripts,
`granular/*`, `posix_fallocate/00`, `utimensat/03`, `rename/22`), and the
`EFAULT` scripts (`*/1?.t` passing a NULL or invalid pointer), which pass on
EdenFS but cannot be written with the `posix` module.

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
| [ftruncate/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/ftruncate/00.t) | `test_ftruncate_00_descrease_increase_size` | 23/26 | needs root: 18, 24; dependent: 25 |
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
| [truncate/00.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/truncate/00.t) | `test_truncate_00_descrease_increase_size` | 20/21 | needs root: 18 |
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
| [open/17.t](https://github.com/pjd/pjdfstest/blob/ededbeb2b44929972898afb87474b0937f78a877/tests/open/17.t) | `test_open_17_enxio_o_nonblock_fifo_o_wronly` | 3/3 |  |
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
