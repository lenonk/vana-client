# Examples

Complete mods. To try one, copy its folder into your mods folder, switch it on in Config → Settings → Mods, and type
`/lua load <folder name>`. Each was run in the client as it is here.

| Mod | Shows | Permissions |
| --- | --- | --- |
| [hello](hello/) | The [getting started](../getting-started.md) mod: events, a command, an HP readout on the HUD, saved data, a setting, a window that targets what you pick. | `actions` |
| [target-ring](target-ring/) | Drawing in the world every frame: a ring around your target and its distance over its head. | none |
| [chat-tools](chat-tools/) | Chat filters (hide lines with a word), a typed-line shortcut (`/gg`), a repeating timer (`/countdown`), a confirmation prompt (`/forget`). | none |
| [waiting](waiting/) | Waiting inside a command: `/nearest` targets the closest thing and waits until the target has changed. | `actions` |

Commands:

| Mod | Command | Does |
| --- | --- | --- |
| hello | `/hp` | Your HP in the chat log. |
| hello | `/nearby` | Opens the Nearby window. |
| chat-tools | `/hideword <word>` | Hides chat lines containing the word; `/hideword` alone stops. |
| chat-tools | `/gg` | Says "Good game, everyone!" (to you, with /echo). |
| chat-tools | `/countdown <n>` | Counts down from n, one line a second. |
| chat-tools | `/forget` | Asks, then forgets the hidden word. |
| waiting | `/nearest` | Targets whatever is closest. |
