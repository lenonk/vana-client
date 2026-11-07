# `ui`

The `ui` namespace makes windows for your mod. You describe a window as a list of elements (texts, buttons, sliders,
lists and so on) with `ui.window`, show it with `ui.open` or `ui.show`, and get what the player does in it through the
handler you give `ui.on`. The client draws the window in the look of the UI the player has chosen (the classic UI or
the modern UI), and the keyboard and the mouse work the same way in both. None of these functions needs a permission.
Changes to a window apply at once: `ui.get` right after `ui.update` sees the new value.

```lua
ui.window("hello", {
  title = "Hello",
  elements = {
    {type = "text", text = "Hello, adventurer!"},
    {type = "buttons", id = "ok", items = {"OK"}},
  },
})
ui.on("hello", function(win, e)
  if e.kind == "press" then ui.close(win) end
end)
commands.register("hello", function() ui.open("hello") end)
```

## Functions

| Function | What it does |
| --- | --- |
| [`ui.window(id, spec)`](#uiwindow) | Creates a window, or replaces the definition of one you made before. |
| [`ui.update(id, element, fields)`](#uiupdate) | Changes some fields of one element of a window. |
| [`ui.get(id, element)`](#uiget) | Reads an element's value as the player left it. |
| [`ui.open(id)`](#uiopen) | Shows a window and gives it the keys. |
| [`ui.show(id)`](#uishow) | Shows a window without giving it the keys. |
| [`ui.close(id)`](#uiclose) | Hides a window. |
| [`ui.is_open(id)`](#uiis_open) | Whether a window is showing. |
| [`ui.on(id, handler)`](#uion) | Sets the function that gets a window's events. |
| [`ui.prompt(text, buttons?, handler)`](#uiprompt) | Asks the player a question with buttons (Yes / No by default). |
| [`ui.menu(items, handler)`](#uimenu) | Opens a small pop-up list at the pointer, like a right-click menu. |

## Window elements

A window's `elements` is a list of tables, drawn top to bottom in the order given. Every element table has a `type`
(one of the types below). Most types also need an `id`, a string you choose, which is how `ui.update`, `ui.get` and
the events name the element. Give each element of a window a different id. The window's own options (`title`,
`width`, `help`, `hud`, `frame`, `x` / `y`, `modal`, `popup`) are listed under [`ui.window`](#uiwindow).

Every element, of any type, may also have these fields:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `type` | `string` | (required) | The element type: `text`, `buttons`, `slider`, `list`, `gauge`, `separator`, `tabs`, `input`, `choice`, `colour` (or `color`), `section` or `canvas`. |
| `tip` | `string` | `""` | Help text shown while the cursor or the pointer is on this element (see [Tips and help](#tips-and-help)). |
| `page` | `integer` | none | The page (from 1) of the tabs above it that this element belongs to (see [Pages](#pages-tabs)). Without it, the element shows on every page. |

The keyboard cursor stops on buttons, sliders, lists, tabs, inputs, choices, colours and sections. Texts, gauges,
separators and canvases are not cursor stops (a canvas still takes the mouse).

### text

A line of text.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | `""` | Optional. Needed only to change it with `ui.update` or read it with `ui.get`. |
| `text` | `string` | `""` | The text. |
| `dim` | `boolean` | `false` | Draws it greyed. |
| `colour` | `{r, g, b, a}` | none | Its colour (0..255 each, `a` defaults to 255). Overrides `dim`. |

### buttons

A row of buttons under a heading. Without `marked`, each button is a plain button and pressing it sends a `press`
event. With `marked`, the buttons are options: one of them is marked, and choosing another marks it and sends a `set`
event.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading above the buttons. |
| `items` | `string[]` | `{}` | The buttons' captions, left to right. |
| `marked` | `integer` | none | Makes the buttons options, with this one (from 1) marked. |

### slider

A slider with a number of steps. Enter starts moving it, Left / Right move it, Enter or Escape ends. Each move sends a
`set` event. The slider shows no number of its own: put the value you want shown in `shown`, and update it when the
slider moves.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading above the slider. |
| `steps` | `integer` | `2` | How many positions it has (at least 1). |
| `at` | `integer` | `1` | Its position, from 1 to `steps` (larger values are cut to `steps`). |
| `shown` | `string` | `""` | A text shown with the slider, usually its value. |

### list

Rows with a cursor. Up / Down move through the rows; Enter or a click on a row sends a `select` event. `height` rows
show at once; the list scrolls to keep the cursor in view.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `rows` | `table` | `{}` | The rows, top to bottom (see the row table below). |
| `columns` | `table` | `{}` | The columns (see the column table below). Without columns, a row's cells are shown together, two spaces apart. |
| `at` | `integer` | `1` | The row the cursor is on, from 1. |
| `height` | `integer` | `8` | How many rows show at once (at least 1). |

A row is either a string (a number or a boolean is shown as text too), or a table:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `cells` | `table` | the row table's own list part | The row's cells, one per column. You may instead put the cells straight in the row table: `{"Potion", "12"}`. |
| `icon` | `{kind, id}` | none | An icon before the first cell: `kind` is `"item"` (or none), `"status"`, `"spell"` or `"ability"`, as for [`hud.icon`](hud.md#hudicon); `id` is the item, status, spell or ability id (required in an `icon` table). |
| `colour` | `{r, g, b, a}` | none | The row's text colour. |

A column is a table:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `title` | `string` | `""` | The column heading. Headings show only when at least one column has a title; a click on a heading sends a `sort` event. |
| `width` | `number` | `0` | The column's width in the window's units (see `width` under [`ui.window`](#uiwindow)). `0` shares out what the fixed columns leave. |
| `align` | `string` | left | `"right"` aligns the column's cells to the right (for numbers). |

### gauge

A bar filled from 0 to 1, with a label and a text. It is not a cursor stop.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | `""` | Optional. Needed only for `ui.update` and `ui.get`. |
| `label` | `string` | `""` | The text before the bar. |
| `value` | `number` | `0` | How full it is, 0 to 1. |
| `text` | `string` | `""` | A text shown with the bar, for example `"75%"`. |
| `kind` | `string` | `"hp"` | The bar's colour: `"hp"`, `"mp"`, `"tp"`, `"cast"`, `"experience"` or `"recast"`. |

### separator

A gap between elements (the modern UI draws a line). It has no fields besides `type`, and no id.

### tabs

Page tabs: the elements after it that have a `page` field show only on their page (see [Pages](#pages-tabs)). Left /
Right (or a click) turn the page and send a `set` event.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `items` | `string[]` | `{}` | The page names. |
| `at` | `integer` | `1` | The page shown, from 1. |

### input

A line the player types in. Enter (or a click on it) starts typing; Enter ends it and sends a `change` event; Escape
puts the text back as it was. While the player types, every change sends an `edit` event. Backspace, Delete, Left,
Right, Home and End work while typing.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading above the line. |
| `text` | `string` | `""` | The text in the line. |
| `max` | `integer` | `64` | The most characters it takes (1 to 1024). |

### choice

One value out of several, turned with Left / Right. Enter (or a click on the value) turns it forward. Each turn sends a
`set` event. Turning wraps round from the last value to the first.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading above it. |
| `items` | `string[]` | `{}` | The values. |
| `at` | `integer` | `1` | The value shown, from 1. |

### colour

A colour the player can edit. Enter (or a click on the colour box) starts editing. With the keyboard, Up / Down pick
the channel (R, G, B, and A when `alpha` is on) and Left / Right change it in 32 steps from 0 to 255; Enter keeps the
colour and Escape puts the old one back. The modern UI also offers a colour picker. Every change sends a `change`
event with the new value. The type may be spelt `"colour"` or `"color"`.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading above it. |
| `value` | `{r, g, b, a}` | white `{255, 255, 255, 255}` | The colour. |
| `alpha` | `boolean` | `false` | Lets the player edit the alpha (A) channel too. |

### section

A heading that folds what is under it (see [Folding sections](#folding-sections)). Enter or a click folds or unfolds
it and sends a `toggle` event.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `label` | `string` | `""` | The heading. |
| `open` | `boolean` | `true` | Whether it starts unfolded. |

### canvas

A region your mod draws in with a list of drawing commands. Coordinates are in the units of the window's `width`,
from the canvas's top left corner, with y going down; drawing outside it is cut off. A click sends a `click` event and
a drag sends `drag_start`, `drag` and `drag_end` events (see [Window events](#window-events)). To redraw it, give it a
new `draw` list with `ui.update(window, canvas, {draw = ...})`. It is not a cursor stop.

A press and release without moving is a click. Moving more than a few units with the button down starts a drag
instead, and no `click` follows it.

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `id` | `string` | (required) | The element's id. |
| `w` | `number` | `200` | Its width (at least 1). |
| `h` | `number` | `100` | Its height (at least 1). |
| `draw` | `table` | `{}` | The drawing commands, drawn in order (later ones over earlier ones). |

Every drawing command is a table with a `type` (required) and a `colour` (`{r, g, b, a}`, default white). The other
fields, all numbers defaulting to `0` unless shown:

| `type` | Fields | What it draws |
| --- | --- | --- |
| `"line"` | `x1`, `y1`, `x2`, `y2`, `width` (default `1`) | A line from (x1, y1) to (x2, y2), `width` thick. |
| `"rect"` | `x`, `y`, `w`, `h`, `to`, `line` | A filled rectangle with its top left at (x, y). With `to` (a colour), it fades from `colour` at the top to `to` at the bottom. With `line` (a width), only its outline is drawn, that thick (`to` is then not used). |
| `"circle"` | `x`, `y`, `r`, `line` | A filled circle centred on (x, y) with radius `r`; with `line`, only its outline, that thick. |
| `"tri"` | `x1`, `y1`, `x2`, `y2`, `x3`, `y3` | A filled triangle. |
| `"text"` | `x`, `y`, `text` (default `""`) | Text with its top left at (x, y), in the UI's text font. |
| `"icon"` | `x`, `y`, `size` (default `16`), `kind`, `id` | A square icon with its top left at (x, y): `kind` is `"item"` (or none), `"status"`, `"spell"` or `"ability"`, as for [`hud.icon`](hud.md#hudicon). `id` is required. |

### Pages (tabs)

A `tabs` element starts a set of pages. Each element after it with a `page = n` field shows only while the tabs are on
page n (counting from 1). Elements without `page` show on every page. The next `tabs` element starts a new set, and
the elements after it follow those tabs instead. An element with a `page` that comes before any tabs always shows.

When the page turns, the cursor moves to the first control that can be seen if it was on one that is now hidden.

### Folding sections

A `section` folds the elements under it, down to the next `section` or `tabs` element. While it is folded those
elements are hidden. On a window with pages, a section that has a `page` folds only the elements of its own page and
the elements without a page; elements of other pages are not under it.

### Tips and help

The window's `help` text and each element's `tip` tell the player what things do. While the window has the keys, the
classic UI shows them in its help bar (the tip of the element under the cursor, else the window's help). The modern UI
shows the help under the window's elements: the tip of the element under the pointer; else, while the window has the
keys, the tip of the element under the cursor; else the window's help.

### Keys and the mouse

While a window has the keys:

- **Up / Down** move the cursor from control to control, wrapping round at the ends. On a list they first move
  through its rows; past the first or last row they go on to the next control. Moving onto a list puts its cursor on
  its first row (coming from above) or its last row (coming from below).
- **Left / Right** move between the buttons of a buttons element, and turn tabs and choices. While a slider or a colour
  is being edited, they change it.
- **Enter** presses a button, chooses an option or a list row, starts moving a slider, starts typing in an input,
  starts editing a colour, turns a choice forward, and folds or unfolds a section.
- **Escape** closes the window and sends `close`. While something is being edited it ends the edit instead: a slider
  keeps its value, a colour and a typed line go back to what they were.

The mouse works in both UIs. A click on a window gives it the keys (except a window made with `hud = true`). A click
on a control does what Enter does there, and also: a click on a slider moves it to that point, a click on a list row
chooses that row, a click on a list's column heading sends `sort`, and a click on the arrows of tabs or a choice turns
them back or forward (the modern UI shows tabs as a strip: a click on a tab goes straight to it). In the classic UI the
cursor also follows the pointer over the controls.

## Window events

The handler you give [`ui.on`](#uion) is called as `handler(window_id, event)`. `event` is a table with a `kind` and,
for every kind but `close`, the `id` of the element it comes from. All indexes count from 1. When the event arrives,
the window has already changed: a list's cursor is on the chosen row, a closed window is already closed.

| `kind` | Fields | Sent when |
| --- | --- | --- |
| `"press"` | `id`, `index` | A plain button is pressed (Enter or a click). `index` is which button, from the left. |
| `"set"` | `id`, `index` | An option button is chosen (only when the choice changes), a slider moves (`index` is its new step), tabs turn (`index` is the new page) or a choice turns (`index` is the new value). |
| `"select"` | `id`, `index` | A list row is chosen with Enter or a click. `index` is the row. |
| `"edit"` | `id`, `text` | The text of an input changes while the player types. Also sent with the old text when Escape puts it back. |
| `"change"` | `id`, `text` | Typing in an input ends with Enter, or because the player clicked another element or another window. |
| `"change"` | `id`, `value` | A colour changes: each step while it is edited, and again when Escape puts the old colour back. `value` is `{r, g, b, a}`. |
| `"toggle"` | `id`, `open` | A section is folded (`open = false`) or unfolded (`open = true`). |
| `"sort"` | `id`, `column` | A list's column heading is clicked. `column` is which column. |
| `"click"` | `id`, `x`, `y` | A canvas is clicked. `x` and `y` (integers) are where, in the canvas's units from its top left. |
| `"drag_start"` | `id`, `x`, `y`, `dx`, `dy` | The pointer moves with the left button held down on a canvas: a drag starts. `x`, `y` are where the button went down. |
| `"drag"` | `id`, `x`, `y`, `dx`, `dy` | The pointer moves during that drag. `dx`, `dy` are the move since the last report. The drag stays with the canvas, even when the pointer leaves it, until the button comes up. |
| `"drag_end"` | `id`, `x`, `y`, `dx`, `dy` | The button comes up, ending the drag. |
| `"close"` | (none) | Escape closed the window, or a click outside a `popup` window closed it. `ui.close` does not send it. |

Note that `change` comes from both inputs and colours: look at `e.id`, or at whether `e.text` or `e.value` is set.

The handler runs as the player's input, so with the `actions` permission it may cast, use items and the like. It may
also sleep. Events come in the mod phase after the key press or click.

```lua
ui.on("settings", function(win, e)
  if e.kind == "set" and e.id == "volume" then
    ui.update(win, "volume", {shown = tostring(e.index - 1)})
  elseif e.kind == "change" and e.id == "name" then
    print("name is now " .. e.text)
  elseif e.kind == "close" then
    print("settings closed")
  end
end)
```

### A complete example

A window with a text, a slider, a list and two buttons, opened with the command `//fruit`.

```lua
local fruit = {"Apple", "Pear", "Plum", "Cherry"}
ui.window("fruit", {
  title = "Fruit",
  help = "Pick a fruit and how many.",
  elements = {
    {type = "text", id = "status", text = "Nothing picked yet."},
    {type = "slider", id = "count", label = "How many", steps = 10, at = 1, shown = "1"},
    {type = "list", id = "kind", rows = fruit, height = 4, tip = "Enter picks a fruit."},
    {type = "buttons", id = "act", label = "Done?", items = {"Reset", "Close"}},
  },
})
ui.on("fruit", function(win, e)
  if e.kind == "set" and e.id == "count" then
    ui.update(win, "count", {shown = tostring(e.index)})
  elseif e.kind == "select" and e.id == "kind" then
    local n = ui.get(win, "count").at
    ui.update(win, "status", {text = n .. " x " .. fruit[e.index]})
  elseif e.kind == "press" and e.id == "act" then
    if e.index == 1 then
      ui.update(win, "count", {at = 1, shown = "1"})
      ui.update(win, "status", {text = "Nothing picked yet."})
    else
      ui.close(win)
    end
  end
end)
commands.register("fruit", function() ui.open("fruit") end)
```

## `ui.window`

```lua
ui.window(id, spec)
```

Creates a window from a description, or replaces the description of a window your mod already made with this id. The
window does not show until you call `ui.open` or `ui.show`. Calling it again on a window that is open keeps it open and
redraws it with the new elements; the values you give (`at`, `text`, `open` and so on) replace what the player had set.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's name. It only has to be unique among your mod's windows. Avoid `"__prompt"` and `"__menu"`, which `ui.prompt` and `ui.menu` use. |
| `spec` | [`WindowSpec`](types.md#windowspec) | yes | The window's description. |

**`spec` fields**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `title` | `string` | `""` | The window's title. |
| `width` | `number` | `366` in the classic UI, `360` in the modern UI | The window's width. In the classic UI it is in the classic UI's units (those of `x` and `y`); in the modern UI it is in the modern UI's points. `0` gives the default. |
| `help` | `string` | `""` | Help text for the window (see [Tips and help](#tips-and-help)). |
| `elements` | `table` | `{}` | The elements, top to bottom (see [Window elements](#window-elements)). |
| `hud` | `boolean` | `false` | A HUD window: it never takes the keys, not from `ui.open` and not from a click. Clicks on its controls still act. |
| `frame` | `boolean` | `true` | `false` draws the window without its frame. |
| `x`, `y` | `number` | none | The window's top left corner in the classic UI, in its own units. Give both, or neither. Without them, windows are placed one below and right of the other from the top left. The modern UI places its windows itself and does not use these. |
| `modal` | `boolean` | `false` | While a modal window is open it keeps the keys and is the only mod window the pointer reaches; other mod windows cannot take the keys from it. It is centred on the screen (in the classic UI, at `x`, `y` when given). A click outside it does not close it. |
| `popup` | `boolean` | `false` | A pop-up: a modal window placed with its top left at the pointer, which a click outside it closes (sending `close`). It opens where the pointer is when `ui.open` opens it. |

**Returns** nothing.

**Errors**
- An element with a type not in the list: `unknown element type "..."` (one without a `type` raises a conversion
  error).
- An element of a type that needs an id but has none: `a list element needs an id` (with its type).
- A canvas drawing command with a type not in the list: `unknown draw command "..."`.
- A colour field that is neither a table nor `false`: `a colour is {r, g, b, a} (0..255), or false for none`.
- A field of the wrong kind (for example a string where a number belongs) raises a conversion error.

**Example**

```lua
ui.window("timer", {
  title = "Egg timer",
  width = 300,
  elements = {
    {type = "choice", id = "mins", label = "Minutes", items = {"1", "3", "5"}, at = 2},
    {type = "gauge", id = "left", label = "Left", value = 1, kind = "recast"},
    {type = "buttons", id = "go", items = {"Start", "Stop"}},
  },
})
```

The player's `/lua hide` command hides every mod's windows (the mods keep running).

## `ui.update`

```lua
ui.update(id, element, fields)
```

Changes one element of a window: only the fields you give change, the others stay. Use it to show new values (a text,
a list's rows, a gauge) without rebuilding the whole window. It sends no event.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |
| `element` | `string` | yes | The element's id. |
| `fields` | `table` | yes | The fields to change. |

**`fields` fields**

Which fields an element takes depends on its type. Any other field is ignored. Every element also takes `tip` and
`page`. An element's `type` and `id` cannot change.

| Element type | Fields it takes |
| --- | --- |
| text | `text`, `dim`, `colour` (`false` takes it away) |
| buttons | `label`, `items`, `marked` (`false` takes the mark away) |
| slider | `label`, `steps`, `at`, `shown` |
| list | `rows`, `columns`, `at`, `height` |
| gauge | `label`, `value`, `text`, `kind` |
| tabs | `items`, `at` |
| input | `label`, `text`, `max` |
| choice | `label`, `items`, `at` |
| colour | `label`, `value`, `alpha` |
| section | `label`, `open` |
| canvas | `w`, `h`, `draw` |

The fields mean what they mean in [Window elements](#window-elements); indexes count from 1.

**Returns** nothing.

**Errors**
- No window with that id: `no window "..."`.
- No element with that id in the window: `no element "..." in "..."`. Texts and gauges without an id, and
  separators, cannot be found.
- Fields of the wrong kind raise the same errors as in `ui.window`.

**Example**

```lua
ui.window("stats", {title = "Stats", elements = {{type = "text", id = "uptime", text = ""}}})
ui.show("stats")
local n = 0
timer.every(1, function()
  n = n + 1
  ui.update("stats", "uptime", {text = "Up for " .. n .. " s"})
end)
```

## `ui.get`

```lua
ui.get(id, element) -> table
```

Reads an element's value as the player left it: the text typed, the row or option chosen, the colour, whether a section
is open. Use it when you need a value at some later moment instead of tracking every event.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |
| `element` | `string` | yes | The element's id. |

**Returns** `table`: one field, depending on the element type:

| Element type | Field | Type | Description |
| --- | --- | --- | --- |
| text, input | `text` | `string` | Its text. |
| buttons | `at` | `integer?` | The marked option, from 1; `nil` for plain buttons (no `marked`). |
| slider | `at` | `integer` | Its step, from 1. |
| list | `at` | `integer` | The row the cursor is on, from 1. |
| tabs | `at` | `integer` | The page shown, from 1. |
| choice | `at` | `integer` | The value shown, from 1. |
| gauge | `value` | `number` | Its value, 0 to 1. |
| colour | `value` | `{r, g, b, a}` | The colour, a table with fields `r`, `g`, `b`, `a` (0..255). |
| section | `open` | `boolean` | Whether it is unfolded. |
| canvas | (none) | | An empty table. |

**Errors**
- No window with that id: `no window "..."`.
- No element with that id in the window: `no element "..." in "..."`.

**Example**

```lua
ui.window("setup", {title = "Setup", elements = {
  {type = "input", id = "name", label = "Name", text = "Kupo"},
  {type = "colour", id = "tint", label = "Tint", value = {200, 160, 80}},
}})
commands.register("save", function()
  local name = ui.get("setup", "name").text
  local c = ui.get("setup", "tint").value
  print(("name %s, colour %d %d %d"):format(name, c.r, c.g, c.b))
end)
```

## `ui.open`

```lua
ui.open(id)
```

Shows a window and gives it the keys (arrows, Enter, Escape), so the player can use it with the keyboard at once. It
takes the keys from any other window of a mod, at the end of the mod phase. A `hud = true` window shows but does not
take the keys. While a modal window is open, only another modal window can take the keys from it.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |

**Returns** nothing.

**Errors** No window with that id: `no window "..."`.

**Example**

```lua
commands.register("fruit", function()
  ui.open("fruit")
end)
```

## `ui.show`

```lua
ui.show(id)
```

Shows a window without giving it the keys, so the game keeps the keyboard. Use it for windows that display information.
A click on it still works, and (unless it is a `hud = true` window) gives it the keys.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |

**Returns** nothing.

**Errors** No window with that id: `no window "..."`.

**Example**

```lua
ui.window("dps", {title = "DPS", hud = true, elements = {{type = "text", id = "v", text = "0"}}})
ui.show("dps")
```

## `ui.close`

```lua
ui.close(id)
```

Hides a window. The window keeps its elements and values, so `ui.open` or `ui.show` brings it back as it was. It sends
no `close` event. If the player was typing in one of its inputs, the typing ends without a `change` event (the text
typed so far stays in the element).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. A window that does not exist is ignored. |

**Returns** nothing.

**Example**

```lua
ui.on("fruit", function(win, e)
  if e.kind == "press" and e.id == "act" and e.index == 2 then
    ui.close(win)
  end
end)
```

## `ui.is_open`

```lua
ui.is_open(id) -> boolean
```

Tells whether one of your mod's windows is showing (opened with `ui.open` or `ui.show` and not closed since).

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |

**Returns** `boolean`: `true` while the window shows; `false` when it is hidden or does not exist.

**Example**

```lua
commands.register("fruit", function()
  if ui.is_open("fruit") then ui.close("fruit") else ui.open("fruit") end
end)
```

## `ui.on`

```lua
ui.on(id, handler)
```

Sets the function that gets a window's events (see [Window events](#window-events)). A window has one handler: a
second call replaces the first. You may set it before or after `ui.window`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `id` | `string` | yes | The window's id. |
| `handler` | `function` | yes | Called as `handler(window_id, event)` for every event of the window. |

**Returns** nothing.

**Example**

```lua
ui.on("fruit", function(win, e)
  if e.kind == "select" then
    print(win .. ": row " .. e.index .. " of " .. e.id)
  end
end)
```

The handler runs as the player's input: with the `actions` permission it may do game actions (see
[Window events](#window-events)).

## `ui.prompt`

```lua
ui.prompt(text, buttons?, handler)
```

Asks the player a question in a small modal window in the centre of the screen, with a row of buttons under the text.
When the player presses a button the window closes and `handler` gets that button's number. If the player presses
Escape instead, `handler` gets `nil`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `text` | `string` | yes | The question. |
| `buttons` | `string[]` | no, default `{"Yes", "No"}` | The buttons' captions. Leave it out (`ui.prompt(text, handler)`) or pass `nil` for Yes / No. |
| `handler` | `function` | yes | Called as `handler(index)`: `index` is the button pressed, from 1, or `nil` when Escape closed the prompt. |

**Returns** nothing.

**Example**

```lua
local kills = 0
commands.register("reset", function()
  ui.prompt("Reset the kill counter?", function(i)
    if i == 1 then kills = 0 print("Counter reset.") end
  end)
end)
```

Notes:
- A click outside the prompt does not close it.
- Your mod has one prompt at a time. Calling `ui.prompt` while one is open replaces it, and the first handler is never
  called.
- The handler runs as the player's input, like a window handler.

## `ui.menu`

```lua
ui.menu(items, handler)
```

Opens a small pop-up list with its top left at the pointer, like a right-click menu. When the player picks a row (Enter
or a click), the list closes and `handler` gets that row's number. Escape or a click outside the list closes it and
gives `handler` `nil`.

**Parameters**

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `items` | `string[]` | yes | The rows, top to bottom. Up to 12 show at once; more scroll. |
| `handler` | `function` | yes | Called as `handler(index)`: `index` is the row picked, from 1, or `nil` when the list was closed. |

**Returns** nothing.

**Example**

```lua
hud.on("button", function(id, e)
  if e.button == "right" then
    ui.menu({"Reset", "Hide"}, function(i)
      if i == 1 then print("reset") elseif i == 2 then hud.remove("button") end
    end)
  end
end)
```

Notes:
- Your mod has one such menu at a time: a new call replaces an open one, whose handler is then never called.
- The handler runs as the player's input, like a window handler.
