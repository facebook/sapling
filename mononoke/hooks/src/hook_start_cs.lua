--[[
Copyright (c) Facebook, Inc. and its affiliates.

This software may be used and distributed according to the terms of the
GNU General Public License found in the LICENSE file in the root
directory of this source tree.
--]]

g__hook_start = function(info, arg)
  return g__hook_start_base(info, arg, function(arg, ctx)
    local files = {}

    -- translation to lua from mercurial's util.shortuser()
    local get_author_unixname = function(author)
      local ind = author:find('@')
      if ind then
        author = author:sub(1, ind - 1)
      end

      ind = author:find('<')
      if ind then
        author = author:sub(ind + 1)
      end

      ind = author:find(' ')
      if ind then
        author = author:sub(0, ind)
      end

      ind = author:find('%.')
      if ind then
        author = author:sub(0, ind)
      end

      return author
    end

    for _, file_data in ipairs(arg) do
      local file = g__set_common_file_functions(file_data.path, file_data.type)

      if not file.is_deleted() then
        file.contains_string = function(s)
          return coroutine.yield(g__contains_string(file.path, s))
        end
        file.len = function()
          return coroutine.yield(g__file_len(file.path))
        end
        file.text = function()
          return coroutine.yield(g__file_text(file.path))
        end
        file.path_regex_match = function(p)
          return coroutine.yield(g__regex_match(p, file.path))
        end
      end
      files[#files+1] = file
    end

    ctx.files = files
    ctx.info.author_unixname = get_author_unixname(ctx.info.author)
    ctx.file_text = function(path)
      return coroutine.yield(g__file_text(path))
    end
    ctx.parse_commit_msg = function()
      return coroutine.yield(g__parse_commit_msg())
    end
    ctx.is_valid_reviewer = function(user)
      return coroutine.yield(g__is_valid_reviewer(user))
    end
  end)
end
