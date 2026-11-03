-- Hide every chat line that contains a word (/hideword <word>, /hideword alone to stop).
-- (/hide is taken: it is the game's command for hiding from the friend list.)
local hidden_word = nil

filters.chat(function(text)
  if hidden_word and text:lower():find(hidden_word, 1, true) then
    return false -- drop the line
  end
  return nil -- keep it as it is
end)

commands.register("hideword", function(word)
  hidden_word = word and word:lower() or nil
  chat.add(hidden_word and ("Hiding lines with " .. hidden_word .. ".") or "Hiding nothing.")
end)

-- A shortcut: typing /gg is replaced by a line before the game reads it.
filters.typed(function(line)
  if line == "/gg" then
    return "/echo Good game, everyone!"
  end
end)

-- /countdown 5: one line a second, then "Go!".
commands.register("countdown", function(n)
  local left = tonumber(n) or 3
  local id
  id = timer.every(1, function()
    if left > 0 then
      chat.add(tostring(left))
      left = left - 1
    else
      chat.add("Go!")
      timer.cancel(id)
    end
  end)
end)

-- /forget asks first, in the game's look.
commands.register("forget", function()
  ui.prompt("Forget the hidden word?", { "Forget it", "Keep it" }, function(choice)
    if choice == 1 then
      hidden_word = nil
      chat.add("Forgotten.")
    end
  end)
end)
