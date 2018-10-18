-- If a file's path matches any of these patterns, it cannot be committed
local deny_files = {
  -- buck-out directories are not normally committed
  -- Note that - is special in Lua patterns - % escapes it.
  "^buck%-out/",
  "/buck%-out/",
  -- Easy marker for any file that must never be committed
  "DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED",
  -- Old fbmake output directories in fbsource
  "^fbcode/_bin/",
  -- Accidental nesting of project names in fbsource
  "^fbandroid/fbandroid/",
  "^fbcode/fbcode/",
  "^fbobjc/fbobjc/",
  "^xplat/xplat/",
  -- fbsource xplat should not contain per-project subfolders.
  "^xplat/fbandroid/",
  "^xplat/fbcode/",
  "^xplat/fbobjc/",
}

hook = function (ctx)
  for _, deny_pattern in ipairs(deny_files) do
    if ctx.file.path:match(deny_pattern) then
      return false, (
          "Denied filename '%s' matched name pattern '%s'. " ..
          "Rename or remove this file and try again."
        ):format(ctx.file.path, deny_pattern)
    end
  end
  return true
end
