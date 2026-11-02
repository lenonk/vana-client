## Lua mods: wait_until budget, require paths, links in mod folders (ours, 2026-11-02)

- wait_until: the test runs on the game thread each frame. It now runs under the filter budget (FILTER_CAP,
  20 ms) with the previous deadline restored after; one that runs past it, or errors, is recorded as the mod's
  error and ends the wait (as an error did before). A test that looped used to freeze the client.
- require: a module name with a backslash or a colon is refused, as `..` and a leading `/` were (on Windows those
  made an absolute path that joined past the mod's folder).
- fs: a path whose existing part is a symlink, from mod-data down (the mod's <id>/files folder or .shared, and
  the path in it), is refused, so a link can't lead read, write, list, delete or move outside the folder. mod-data
  itself may be a link (where the user keeps it).
- Not changed: mods.call runs the provider's function with the provider's grants. Whether it should check the
  caller's grants instead is still open (upstream's permission model).
