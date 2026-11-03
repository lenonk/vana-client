# Modding vana-client

Mods are small Lua programs the client runs beside the game. A mod can show you things the game doesn't (timers,
distances, a list of who is nearby), change what the game shows, add commands, and act in the game for you when you
allow it. Mods use the client's windows and look, in both the classic and the modern interface.

## Where to start

1. **[Getting started](getting-started.md)**: write your first mod, step by step, from an empty folder to a mod with
   a command, an on-screen readout, a setting, saved data and a window.
2. **[The guide](guide.md)**: how mods work, topic by topic, with code you can copy.
3. **[The reference](reference/README.md)**: every function, its parameters, what it returns, and an example.
4. **[Examples](examples/README.md)**: complete mods to read, run and change.

## What you need

- The client, and a character to log in with.
- A text editor. One with a Lua language server (VS Code with the "Lua" extension, for example) gives you completion
  and checks for the whole API: the client writes the API's description into your mods folder when you log in.
- A little Lua. If you have never used it, [Programming in Lua](https://www.lua.org/pil/contents.html) covers the
  basics; mods use Lua 5.4.

## The shortest mod

`mods/hello/mod.toml`:

```toml
name = "Hello"
```

`mods/hello/init.lua`:

```lua
commands.register("hello", function()
  chat.add("Hello, " .. game.player().name .. "!")
end)
```

Switch it on in Config → Settings → Mods, type `/lua load hello`, then `/hello`.
