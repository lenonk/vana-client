## Lua mods: /lua only from the player (ours, 2026-11-02)

- `commands.run("//lua grant <own id> ...")` used to need no permission (a `//` line skips the actions check) and
  reached `/lua` like a typed line, so any mod could grant itself every permission its mod.toml lists, or
  `/lua run` code in another mod with that mod's grants (issue #68).
- Lines for the mods now carry where they came from: the player (the chat line, a macro), a mod's bind the player
  pressed, or a mod's own line (`commands.run`, an action). `/lua` runs only from the player; a mod's command still
  runs from a bind as the player's input and from a mod's line as the mod's own.
- `commands.run` refuses a line whose first word is `lua` with an error. Mod lines are trimmed before the check, so a
  leading space no longer sends one down the typed path as the player's.
