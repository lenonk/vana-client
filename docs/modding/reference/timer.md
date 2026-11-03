# `timer`

Calls a function after a delay, or again and again at an interval. Timers are checked once a frame during the mod
phase, so a timer fires at the first check after its time has come (a frame's precision). The handler gets no
arguments and runs as a coroutine, so it may sleep. Timers work in the lobby too; they stop when the mod unloads. No
permission is needed, but a timer is not the player's input: game actions from it need `automation`.

```lua
timer.after(3, function()
  print("Three seconds have passed.")
end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`timer.after(seconds, handler)`](#timerafter) | Calls `handler` once, after `seconds`. |
| [`timer.every(seconds, handler)`](#timerevery) | Calls `handler` every `seconds`, until cancelled. |
| [`timer.cancel(id)`](#timercancel) | Stops a timer. |

## `timer.after`

```lua
timer.after(seconds, handler) -> integer
```

Calls `handler` once, `seconds` from now.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `seconds` | `number` | yes | The delay. Below 0 counts as 0 (the next check). Fractions are fine (`0.5`). |
| `handler` | `function` | yes | Called with no arguments. |

**Returns** `integer`: the timer's id, for `timer.cancel`.

**Example**

```lua
events.on("zone_change", function(zone)
  timer.after(5, function()
    print("Five seconds in zone " .. zone)
  end)
end)
```

Inside a handler, `sleep(seconds)` waits too, without a new function.

## `timer.every`

```lua
timer.every(seconds, handler) -> integer
```

Calls `handler` every `seconds`, the first time `seconds` from now, until `timer.cancel` stops it or the mod unloads.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `seconds` | `number` | yes | The interval. Less than 0.01 counts as 0.01. |
| `handler` | `function` | yes | Called with no arguments. |

**Returns** `integer`: the timer's id, for `timer.cancel`.

**Example**

```lua
local ticks = 0
local id
id = timer.every(1, function()
  ticks = ticks + 1
  print("tick " .. ticks)
  if ticks == 10 then timer.cancel(id) end
end)
```

The next call is due one interval after the previous one fired; missed calls are not made up. Each call is a new
coroutine: a handler that sleeps longer than the interval is still running when the next call starts. An error in the
handler is reported, and the timer keeps running.

## `timer.cancel`

```lua
timer.cancel(id)
```

Stops a timer. An id that matches no timer (a `timer.after` that already fired, for example) does nothing. A handler
may cancel its own timer.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `integer` | yes | What `timer.after` or `timer.every` returned. |

**Returns** nothing.

**Example**

```lua
local reminder = timer.after(60, function() print("A minute has passed.") end)
commands.register("noremind", function()
  timer.cancel(reminder)
end)
```

A call that is already running (sleeping) is not stopped by `timer.cancel`.
