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

* Represents a name within a directory
* Illegal to
    * Contain directory separator ("/")
    * Be empty
    * Be a relative component (".." or "..")

### `RelativePath`/`RelativePathPiece`

* Represents any number of `PathComponent(Piece)`s strung together
* Illegal to begin with or be composed with an `AbsolutePath(Piece)`
* Allowed to be empty

### `AbsolutePath`/`AbsolutePathPiece`

* Must begin with a "/"
* May be composed with `PathComponent`s and `RelativePath`s
* May not be composed with other `AbsolutePath`s

## Construction

* Paths can be constructed from the following types:
    * `folly::StringPiece`
    * Stored path
    * Non-stored path
    * Default constructed to an empty value
* Paths can be move-constructed from `std::string` values and Stored values.

## Comparisons

* Comparisons can be made between Stored and Piece variations of the same type,
  meaning one can compare a `RelativePath` to a `RelativePathPiece`, but cannot
  compare a `RelativePath` to an `AbsolutePath`.

## Iterator

* `ComposedPathIterator` - Used for iteration of a `RelativePath`/`AbsolutePath`
  using various iteration methods (`paths()`, `allPaths()`, `suffixes()`,
  `findParents()`). An iterator over prefixes of a composed path. Iterating
  yields a series of composed path elements. For example, iterating the path
  "foo/bar/baz" will yield this series of Piece elements:
    1. "/" but only for `AbsolutePath`
    2. "foo"
    3. "foo/bar"
    4. "foo/bar/baz"
* Note: You may use the `dirname()` and `basename()` methods to focus on the
  portions of interest.
* `PathComponentIterator`- Used for iteration of a ComposedPath using the
  iteration method `components()`. An iterator over components of a composed
  path. Iterating yields a series of independent path elements. For example,
  iterating the relative path "foo/bar/baz" will yield this series of
  PathComponentPiece elements:
    1. "foo"
    2. "bar"
    3. "baz"
* Note: Iterating the absolute path "/foo/bar/baz" would also yield the same
  sequence.

## Utility Functions

* `stringPiece()` - Returns the path as a `folly::StringPiece`
* `copy()` - Returns a stored (deep) copy of this path
* `piece()` - Returns a non-stored (shallow) copy of this path
* `value()` - Returns a reference to the underlying stored value
* `basename()` - Given a path like "a/b/c", returns "c"
* `dirname()` - Given a path like "a/b/c", returns "a/b"
* `getcwd()` - Gets the current working directory as an `AbsolutePath`
* `canonicalPath()` - Removes duplicate "/" characters, resolves "/./" and
  "/../" components. "//foo" is converted to "/foo". Does not resolve symlinks.
  If the path is relative, the current working directory is prepended to it.
  This succeeds even if the input path does not exist
* `joinAndNormalize()` - canonicalize a path string relative to a relative path
  base
* `relpath()` - Converts an arbitrary unsanitized input string to a normalized
  `AbsolutePath`. This resolves symlinks, as well as "." and "." components in
  the input path. If the input path is a relative path, it is converted to an
  absolute path. This throws if the input path does not exist or if a parent
  directory is inaccessible
* `expandUser()` - Returns a new path with `~` replaced by the path to the
  current user's home directory. This function does not support expanding the
  home dir of arbitrary users, and will throw an exception if the string starts
  with `~` but not `~/`. The resulting path will be passed through
  `canonicalPath()` and returned
* `normalizeBestEffort()` - Attempts to normalize a path by first attempting
  `relpath()` and falling back to `canonicalPath()` on failure.
* `splitFirst()` - Splits a path into the first component and the remainder of
  the path. If the path has only one component, the remainder will be empty. If
  the path is empty, an exception is thrown
* `ensureDirectoryExists()` - Ensures that the specified path exists as a
  directory. This creates the specified directory if necessary, creating any
  parent directories as required as well. Returns true if the directory was
  created, and false if it already existed. Throws an exception on error,
  including if the path or one of its parent directories is a file rather than a
  directory
* `removeRecursively()` - Recursively removes a directory tree. Returns false if
  the directory did not exist in the first place, and true if the directory was
  successfully removed. Throws an exception on error.
