--[[
Copyright (c) Facebook, Inc. and its affiliates.

This software may be used and distributed according to the terms of the
GNU General Public License found in the LICENSE file in the root
directory of this source tree.
--]]

g__hook_start = function(info, arg)
  return g__hook_start_base(info, arg, function(arg, ctx)
    local file = g__set_common_file_functions(arg.path, arg.type)

    if not file.is_deleted() then
      file.contains_string = function(s)
        return coroutine.yield(g__contains_string(s))
      end
      file.len = function() return coroutine.yield(g__file_len()) end
      file.text = function() return coroutine.yield(g__file_text()) end
      file.is_symlink = function() return coroutine.yield(g__is_symlink()) end
      file.path_regex_match = function(p)
        return coroutine.yield(g__regex_match(p, file.path))
      end
    end

    ctx.file = file
  end)
end
