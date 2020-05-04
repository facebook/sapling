# EdenFS file globs

EdenFS supports glob patterns through the following interfaces:

* Ignore files (e.g. `.gitignore`)
* `globFiles` Thrift API

## Ignore files

EdenFS uses *ignore files* to exclude files in the `getScmStatus` Thrift API
(used by `hg status`, for example). The syntax for EdenFS' ignore files is
compatible with the syntax for [`gitignore` files][gitignore] used by the Git
version control system, even when an EdenFS checkout is backed by a Mercurial
repository.

## Glob pattern magic

EdenFS interprets the following tokens specially within glob patterns:

* `**`: Match zero, one, or more path components.
* `*`: Match zero, one, or more valid path component characters.
* `?`: Match exactly one valid path component characters.
* `[`: Match exactly one path component character in the given set of
  characters. The set is terminated by `]`.
* `[!`, `[^`: Match exactly one path component character *not* in the given set
  of characters. The set is terminated by `]`.

EdenFS glob patterns are compatible with [`gitignore` patterns][gitignore] used
by the Git version control system, even when an EdenFS checkout is backed by a
Mercurial repository.

## Globbing with symlinks

If a glob pattern matches a symlink exactly, `globFile` returns the symlink
itself (and not its target) as a match.

If a prefix of a glob pattern matches a symlink, `globFile` does not return the
symlink as a match, does not return the symlink's target as a match, and does
not resolve the symlink in order to continue matching the glob pattern.

[gitignore]: https://git-scm.com/docs/gitignore#_pattern_format
