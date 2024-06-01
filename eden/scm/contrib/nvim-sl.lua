--[[
Copyright (c) Meta Platforms, Inc. and affiliates.

This software may be used and distributed according to the terms of the
GNU General Public License version 2.
--]]

--[[

Neovim plugin for Sapling developers.

Features:
- Restoring test states.
  When editing `test-*.t`, press ENTER in normal mode to get a terminal with
  the test state set as if the test just executed to the current line.
  Under the hood, it uses `sl .t --record` and `sl debugrestoretest`.

Instillation:
- Copy this file to `~/.config/nvim/lua/nvim-sl.lua`.
- Append `require("nvim-sl").setup()` to `~/.config/nvim/init.lua`.

Configuration:
- `SL` environment variable: If set, use the specified `sl` binary instead.

Developers:
- See https://luals.github.io/wiki/annotations/ for type annotations.

--]]

--- @alias Cmd string[] | string

local reopen_terminal = (function()
  local close_last = nil

  --- Split a terminal and run `cmd` in it.
  --- Close the old terminal window if it exists.
  ---
  --- @param cmd Cmd Command to run.
  --- @param opts any Options.
  ---   `on_close` is a callback the window is closed.
  ---   `split` defines split position (default: below).
  return function(cmd, opts)
    -- Close the old terminal first.
    if close_last ~= nil then
      close_last()
    end
    -- The nvim APIs are a bit verbose. But this gets the job down.
    local buf = vim.api.nvim_create_buf(false, false)
    local win =
      vim.api.nvim_open_win(buf, true, { split = opts.split or "below" })
    local job_id = nil
    local close_this = nil
    close_this = function()
      if close_this == nil then
        return -- skip on re-entrant
      end
      if close_this == close_last then
        close_last = nil
      end
      close_this = nil
      if vim.api.nvim_win_is_valid(win) then
        vim.api.nvim_win_close(win, true) -- might trigger on_exit
      end
      if vim.api.nvim_buf_is_valid(buf) then
        vim.api.nvim_buf_delete(buf, { force = true })
      end
      if opts ~= nil and opts.on_close ~= nil then
        opts.on_close()
      end
      if job_id ~= nil then
        vim.fn.chanclose(job_id)
      end
    end
    job_id = vim.fn.termopen(cmd, { on_exit = close_this })
    close_last = close_this
    vim.api.nvim_command("startinsert")
  end
end)()

--- Spawn a process. Reports its (exit code, stdout, stderr).
---
--- @param cmd Cmd Command to run.
--- @param callback fun(code:number, stdout:string, stderr:string) Callback.
local function spawn(cmd, callback)
  local stdout = {}
  local stderr = {}
  vim.fn.jobstart(cmd, {
    on_stdout = function(_, data)
      vim.list_extend(stdout, data)
    end,
    on_stderr = function(_, data)
      vim.list_extend(stderr, data)
    end,
    on_exit = function(_, code)
      callback(code, table.concat(stdout), table.concat(stderr))
    end,
  })
end

--- In a test-*.t file, spawn a terminal as if test run up to the current line.
---
--- @param allow_retry boolean Whether to run `sl .t --record` on demand.
local function spawn_terminal_with_restored_test_state(allow_retry)
  local sign_id = 1000
  local file_name = vim.api.nvim_buf_get_name(0)
  local cursor_pos = vim.api.nvim_win_get_cursor(0)
  local line = cursor_pos[1]
  local buf = vim.api.nvim_get_current_buf()
  local sl_cmd = os.getenv("SL") or "sl"
  local cmd = {
    sl_cmd,
    "debugrestoretest",
    "--line",
    tostring(line),
    file_name,
    "--no-traceback",
  }
  spawn(cmd, function(code, stdout, stderr)
    if code == 0 then
      local script = stdout:gsub("\n$", "")
      -- Show ">>" sign to indicate terminal state.
      local function clear_highlight()
        vim.fn.sign_unplace("CurrentLineGroup", { buffer = buf })
      end
      clear_highlight()
      reopen_terminal(script, { on_close = clear_highlight })
      vim.fn.sign_place(
        sign_id,
        "CurrentLineGroup",
        "CurrentLineSign",
        buf,
        { lnum = line, priority = 10 }
      )
    else
      if stderr:find("no recording found") and allow_retry then
        spawn({ sl_cmd, ".t", "--record", file_name }, function(record_code)
          if record_code == 0 or record_code == 1 then
            spawn_terminal_with_restored_test_state(false)
          else
            vim.notify(stderr)
          end
        end)
      else
        vim.notify(stderr)
      end
    end
  end)
end

local function setup()
  vim.fn.sign_define("CurrentLineSign", { text = ">>" })
  vim.api.nvim_create_autocmd("BufEnter", {
    pattern = "test-*.t",
    callback = function()
      vim.keymap.set("n", "<Enter>", function()
        spawn_terminal_with_restored_test_state(true)
      end, { noremap = true, silent = true, buffer = true })
    end,
  })
end

return {
  setup = setup,
}
