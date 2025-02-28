# Paths within EdenFS

## Path Types

There are three `Path` object types, each with a stored and non-stored (`Piece`)
variation. `PathComponent` and `RelativePath` were introduced to have the type
system prevent accidental bugs with using the wrong types in the wrong places.
Their purpose was originally to deal with names in our inode namespace.
`AbsolutePath` was introduced later with the intent to track names in the system
VFS namespace rather than in our mount point namespace.

Values of each of the following types are immutable. They are internally stored
as either a `std::string` or a `folly::StringPiece`, depending on if the `Path`
is stored or non-stored (`Piece`).

### `PathComponent`/`PathComponentPiece`

- Represents a name within a directory
- Illegal to
  - Contain directory separator ("/" or "\" on Windows)
  - Be empty
  - Be a relative component (".." or "..")

### `RelativePath`/`RelativePathPiece`

- Represents any number of `PathComponent(Piece)`s strung together
- Illegal to begin with or be composed with an `AbsolutePath(Piece)`
- Allowed to be empty

### `AbsolutePath`/`AbsolutePathPiece`

- Must begin with a "/" or "\\?\" on Windows
- On Windows, the path separator is always a "\\"
- May be composed with `PathComponent`s and `RelativePath`s
- May not be composed with other `AbsolutePath`s

## Construction

- Paths can be constructed from the following types:
  - `folly::StringPiece`
  - Stored path
  - Non-stored path
  - Default constructed to an empty value
- Paths can be move-constructed from `std::string` values and Stored values.

## Comparisons

- Comparisons can be made between Stored and Piece variations of the same type,
  meaning one can compare a `RelativePath` to a `RelativePathPiece`, but cannot
  compare a `RelativePath` to an `AbsolutePath`.

## Iterator

- `ComposedPathIterator` - Used for iteration of a `RelativePath`/`AbsolutePath`
  using various iteration methods (`paths()`, `allPaths()`, `suffixes()`,
  `findParents()`). An iterator over prefixes of a composed path. Iterating
  yields a series of composed path elements. For example, iterating the path
  "foo/bar/baz" will yield this series of Piece elements:
  1. "/" but only for `AbsolutePath` ("\\?\" on Windows)
  2. "foo"
  3. "foo/bar"
  4. "foo/bar/baz"
- Note: You may use the `dirname()` and `basename()` methods to focus on the
  portions of interest.
- `PathComponentIterator`- Used for iteration of a ComposedPath using the
  iteration method `components()`. An iterator over components of a composed
  path. Iterating yields a series of independent path elements. For example,
  iterating the relative path "foo/bar/baz" will yield this series of
  PathComponentPiece elements:
  1. "foo"
  2. "bar"
  3. "baz"
- Note: Iterating the absolute path "/foo/bar/baz" would also yield the same
  sequence.

## Lifetime

All the stored paths are merely a wrapper around an `std::string`, and the piece
version are also just a wrapper on top of a `folly::StringPiece` (which has
similar semantic as `std::string_view`), that is, a piece merely holds a view of
to the underlying `std::string` buffer. When a stored path is being moved, the
held `std::string` is also moved, which in most cases prevents copying and
re-allocating a string, this makes the move operation fairly cheap and since the
pieces were a view on that first string memory allocation, these are still
viewing valid and allocated memory.

However, `std::string` have an optimization where small strings aren't heap
allocated, but are stored in the `std::string` object itself, this is called SSO
for small string optimization. In this case, a `folly::StringPiece` is no longer
a view on the heap allocated memory, but on that SSO memory. What this means is
that moving a SSO `std::string` will make the `folly::StringPiece` invalid as it
would no longer point to valid memory!

What this means is that taking a path piece of a stored path and then moving
that stored path to extend its lifetime (say by moving it to an `ensure` blob),
will lead to a use after free when using the path piece in the case where the
stored path is small enough that the SSO kicks-in.

## Utility Functions

- `stringPiece()` - Returns the path as a `folly::StringPiece`
- `copy()` - Returns a stored (deep) copy of this path
- `piece()` - Returns a non-stored (shallow) copy of this path
- `value()` - Returns a reference to the underlying stored value
- `basename()` - Given a path like "a/b/c", returns "c"
- `dirname()` - Given a path like "a/b/c", returns "a/b"
- `getcwd()` - Gets the current working directory as an `AbsolutePath`
- `canonicalPath()` - Removes duplicate "/" characters, resolves "/./" and
  "/../" components. "//foo" is converted to "/foo". Does not resolve symlinks.
  If the path is relative, the current working directory is prepended to it.
  This succeeds even if the input path does not exist
- `joinAndNormalize()` - canonicalize a path string relative to a relative path
  base
- `relpath()` - Converts an arbitrary unsanitized input string to a normalized
  `AbsolutePath`. This resolves symlinks, as well as "." and "." components in
  the input path. If the input path is a relative path, it is converted to an
  absolute path. This throws if the input path does not exist or if a parent
  directory is inaccessible
- `expandUser()` - Returns a new path with `~` replaced by the path to the
  current user's home directory. This function does not support expanding the
  home dir of arbitrary users, and will throw an exception if the string starts
  with `~` but not `~/`. The resulting path will be passed through
  `canonicalPath()` and returned
- `normalizeBestEffort()` - Attempts to normalize a path by first attempting
  `relpath()` and falling back to `canonicalPath()` on failure.
- `splitFirst()` - Splits a path into the first component and the remainder of
  the path. If the path has only one component, the remainder will be empty. If
  the path is empty, an exception is thrown
- `ensureDirectoryExists()` - Ensures that the specified path exists as a
  directory. This creates the specified directory if necessary, creating any
  parent directories as required as well. Returns true if the directory was
  created, and false if it already existed. Throws an exception on error,
  including if the path or one of its parent directories is a file rather than a
  directory
- `removeRecursively()` - Recursively removes a directory tree. Returns false if
  the directory did not exist in the first place, and true if the directory was
  successfully removed. Throws an exception on error.

# Length Limitations

- Each PathComponent is limited to 255 characters. This restriction is
  self-imposed in an attempt to maintain compatibility with other filesystems.
- The total path length is not enforced by EdenFS, but operating systems on
  which EdenFS runs may impose their own limits. As of November 2024, the
  following limits are known.
  - Linux: Paths must be less than 4096 characters
    - Check `getconf NAME_MAX $PATH` to check the path component limits for a
      given filesystem
    - Check `getconf PATH_MAX $PATH` to check the total path length limits for a
      given filesystem
  - macOS: Paths must be under 1024 characters
    - Use the same commands as described above for Linux to check max path
      lengths on macOS
  - Windows:
    [There are 2 different limits](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation?tabs=registry)
    - Regular paths must be less than 260 characters total
    - UNC paths are allowed to be much longer: 32,767 total characters
- NOTE: Mononoke (the source control server component) enforces its own max path
  length restrictions. As of November 2024, the max path length that Mononoke
  allows is 980 characters.
