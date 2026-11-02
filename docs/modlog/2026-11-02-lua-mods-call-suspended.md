## Lua mods: mods.call skips a suspended mod (ours, 2026-11-02)

- A mod suspended for its errors (`Mod X suspended: ...`) ran nothing for the host, but other mods could still run
  its provided functions through `mods.call`, with its grants. The call now fails with "mods.call: X is suspended"
  until `/lua reload X`.
- Not changed, still open (issue #70): a provided function runs with the provider's grants, not the caller's. The
  options (check the caller's grants too during the call, or treat providing as delegating and show it on the grant
  page) are upstream's permission model to decide; docs/lua-mods.md now says so where `mods.call` is listed.
