-- In typed lines (the translate and chars add-ons): [[words]] -> the auto-translate phrase with that text; <tag> -> a
-- symbol of the game's font. The game's <t>-style tags are left alone.

local SYMBOLS = {
  star = "★", wstar = "☆", note = "♪", heart = "♥", wheart = "♡", circle = "○", bcircle = "●", square = "□",
  bsquare = "■", triangle = "△", btriangle = "▲", diamond = "◇", bdiamond = "◆", arrow = "→", left = "←",
  up = "↑", down = "↓", music = "♪", infinity = "∞", degree = "°", yen = "¥", times = "×", divide = "÷",
}

filters.typed(function(line)
  if not settings.get("on") then return nil end
  local out = line:gsub("%[%[(.-)%]%]", function(words)
    return text.autotranslate(words) or ("[[" .. words .. "]]")
  end)
  out = out:gsub("<(%a+)>", function(tag)
    return SYMBOLS[tag:lower()]
  end)
  return out ~= line and out or nil
end)
