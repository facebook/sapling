--[[
Copyright (c) Facebook, Inc. and its affiliates.

This software may be used and distributed according to the terms of the
GNU General Public License found in the LICENSE file in the root
directory of this source tree.
--]]

g__set_common_file_functions = function(path, type)
  local file = {}
  file.path = path
  file.is_added = function() return type == "added" end
  file.is_deleted = function() return type == "deleted" end
  file.is_modified = function() return type == "modified" end
  return file
end

g__hook_start_base = function(info, arg, setup)
  if hook == nil then
    error("no hook function")
  end

  local ctx = {}
  ctx.config_strings = g__config_strings
  ctx.config_ints = g__config_ints
  ctx.regex_match = function(pattern, s)
    return coroutine.yield(g__regex_match(pattern, s))
  end
  ctx.info=info
  setup(arg, ctx)

  io = nil
  os = nil
  local acc, desc, long_desc = hook(ctx)
  if type(acc) ~= "boolean" then
    error("invalid hook return type")
  end
  if acc and desc ~= nil then
    error("failure description must only be set if hook fails")
  end
  if acc and long_desc ~= nil then
    error("failure long description must only be set if hook fails")
  end
  if desc ~= nil and type(desc) ~= "string" then
    error("invalid hook failure short description type")
  end
  if long_desc ~= nil and type(long_desc) ~= "string" then
    error("invalid hook failure long description type")
  end
  local res = {acc, desc, long_desc}
  return res
end
