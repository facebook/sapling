hook = function (ctx)
  if string.match(ctx.file.path, "^fbcode/third%-party2/")
    and not ctx.file.is_symlink() then
      return false,
        "All projects committed to fbcode/third-party2/ must be symlinks",
        "All files committed to fbcode/third-party2/ must be symlinks \z
         If you're sure you know what you are doing, then you can bypass \z
         this lint with @allow-non-symlink-tp2 (on its own line)."
  end
  return true
end
