# Hotbars: macro bars by job

Hotbars is a bundled mod (from Arusia's VCHotbar, PR #88): up to three bars of up to twelve slots over the chat
window. Each slot holds a macro of as many lines as you like. Its action's icon shows on the slot (the first spell,
ability, weapon skill or item a line names), with a shade and the time left while it recasts; a slot with no action
shows its label. Turn it on in Config → Extras, on its page (Hotbars), where its size and place are set too.

## Sets

Macros load by job. A **main set** (`WAR`) loads whenever that job is your main job; a **sub set** (`WAR` with sub
`NIN`) loads on top of it when you play WAR/NIN, slot by slot. Slots the main set fills have a gold edge, those the
sub set fills a blue one. Clearing a slot of a sub set the main set fills blanks it for that sub job only.

## On the bars

- **Keys:** bar 1 takes `1`..`0`, `-`, `=`; bar 2 the same with Ctrl, bar 3 with Alt. The manager's Config page picks
  each bar's modifier and the key row (number row, function keys or the numpad), and any slot's key. A slot shows
  its key in its corner.
- **Click** a slot to run it; **right-click** to clear it or open the manager; **drag** a slot onto another to swap
  them (in the sub set when either comes from it).
- **Hover** a slot for its name and lines.
- The jobs over the top bar open the manager.

The game's macro palette hides while the bars show (Hide Stock Macro Bar); its Ctrl and Alt keys still run its
macros unless a bar uses them.

## Macros

Each line runs as typed. `/wait 2` alone on a line waits two seconds; `<wait 2>` at the end of a line waits after it.
A macro still running ignores a second press.

## The library, the Builder and the palette

Each main job has a **library** of macros (`//hb`, Library: New, Edit, Copy, Delete). The Library page shows them as
tiles: click one to choose it, right-click to edit it, drag it onto a slot of the bars to put it there. A slot can
name a library macro instead of holding its copy, so editing the macro changes every slot (main or sub set) that
names it.

The **Builder** makes one. New or Edit opens it in the window's place; Save keeps the macro in the library and
Cancel goes back to it. Pick a type (spell, job ability, weapon skill, pet command, item, equipment, a system
command such as `/attack`, `/heal`, `/assist` or `/lockon`, or Macro for typed lines only), search, pick, choose
the target, and add lines to run after it. System commands show a game icon standing in for them (a sword for
`/attack`, Regen for `/heal`). Spells are the ones the magic list offers: learned, and castable by your main or
support job at its level (blue magic only while set). The help line describes the spell, ability or item under the
cursor or the pointer.

Abilities that open a list in the game's menu (Rune Enchantment, Ward, Effusion, Phantom Roll, Quick Draw, Sambas,
Waltzes, Jigs, Steps, the Flourishes, Stratagems, Blood Pact: Rage and Ward, Ready) open the same list here, marked
`›`: pick Rune Enchantment, then Ignis. Back returns to the list. A search finds the abilities in those lists too.

**Group slots:** one slot for several actions. Open a list such as Rune Enchantment, or search (`cure` finds Cure to
Cure VI), and press **Make a Group of These**; Save it and put it on a slot as any macro. The slot shows the member
used last, with a small arrow in its corner. Pressing it (its key or a click) opens the members in a list over the
slot, the one used last under the cursor: Up / Down and Enter, or a click, runs one; the number keys pick one too;
Escape or a click elsewhere closes it. While you edit a group, the list shows its members (click one to take it
out); search to add more, of any type. The Target menu sets every member's target. Groups are optional: one slot per
rune still works as before.

**Line it builds** shows the macro's first line (`/ja "Ignis" <me>`). Type your line there to run it instead,
for anything the Builder gets wrong; the slot keeps the picked action's icon and recast when your line names none.
Clear the field to go back to the built line.

The **palette** (`//hb palette`, or Palette on the Library page) shows the main job's library as tiles on the screen;
drag its background to move it. Drag a tile onto a slot of the bars to put that macro there; click one to edit it.

The manager's **Config** page sets each bar's modifier and key row, and a slot's key.

The macros are kept in `mod-data/hotbar/files/macros.json`:

```json
{
  "WAR": {
    "library": { "1": { "name": "Provoke", "action": { "kind": "ja", "name": "Provoke", "target": "t" },
                        "lines": ["/p Provoked <t>"] },
                 "2": { "name": "Ignis", "action": { "kind": "ja", "name": "Ignis", "target": "me",
                                                     "line": "/ja \"Ignis\" <me> <wait 1>" } } },
    "main": { "1": { "1": { "ref": "1" }, "2": { "name": "Berserk", "lines": ["/ja \"Berserk\" <me>"] } } },
    "sub": { "NIN": { "1": { "6": { "name": "Utsusemi", "lines": ["/ma \"Utsusemi: Ichi\" <me>"] } } } }
  }
}
```

## Commands

- `//hb` opens or closes the manager.
- `//hb show`, `//hb hide`: the bars for this session.
- `//hb palette` shows or hides the palette.
- `//hb run <bar> <slot>`: runs a slot (for a key bind).
- `//hb reload`: reads macros.json again.
- `//hb wipestock` empties the game's macro sets after keeping a copy; `//hb restorestock` puts it back.
