hook = function (ctx)
  -- TODO(stash): would be nice to have it configurable
  local filesizelimit = 10
  local path = ctx.file.path;

  -- TODO(stash): would be nice to have it configurable
  if path:match("^fbobjc/.*%.mm") or path:match("%.graphql$") or
     path:match("^xplat/third%-party/yarn/offline%-mirror/flow%-bin%-.*%.tgz$") then
    return true
  end

  if ctx.file.len() > filesizelimit then
    local error_msg = ("File size limit is %s bytes. You tried to push file %s that is over the limit"):format(filesizelimit, path)
    return false, error_msg
  end

  return true
end
