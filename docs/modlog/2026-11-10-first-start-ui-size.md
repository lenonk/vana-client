## First start: UI sizes from the screen's EDID (ours, 2026-11-10)

Issue lenonk/vana-client#97: a small screen started with menus too small to reach Settings, which only opens after
logging in.

- On a first start (no config file, not one that failed to read: `Config::first_start`, never saved), once the window exists, client/screen.rs
  reads the monitor's physical size and picks retail's Menu Size (tool 0037 / 0038) and the modern Interface Size
  (`modern_scale`), then saves the config. A config that exists is never touched.
- EDID base block: bytes 0..8 the header (00 FF FF FF FF FF FF 00); bytes 21 / 22 the maximum image size in cm (one of
  them 0: an aspect ratio, no size); the first detailed timing at 54..72 (pixel clock at 54..56, 0 for none) gives the
  active pixels (h = 56 | (58 >> 4) << 8, v = 59 | (61 >> 4) << 8) and the image size in mm (h = 66 | (68 >> 4) << 8,
  v = 67 | (68 & 0xf) << 8). Ours takes the timing's mm, else the cm bytes, and needs 40 mm or more each way; the size
  turns with the monitor (a portrait panel shown landscape), and one whose shape is far from its resolution's
  (outside 0.75..1.33 of it) is not trusted.
- Where the EDID comes from: Linux, /sys/class/drm/card*-<connector>/edid of the connected connectors, the monitor's
  by name (Wayland outputs are named as connectors, HDMI-A-1; X11's drop HDMI's "-A"), else the one whose preferred
  resolution is the monitor's, else the only one (a name match must be at the monitor's resolution when another
  connector is: NVIDIA's X11 driver numbers outputs from 0, the kernel from 1, and two cards can share names); Windows, the EDID value of the monitor's registry key
  (HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<id>\<instance>\Device Parameters, from EnumDisplayDevicesW's device
  interface name for the monitor's \\.\DISPLAYn); macOS, CGDisplayScreenSize.
- The rule *(ours)*: the reference is a 24-inch 1080-line screen (299 mm tall), where 1:1 menus and 60 % read well.
  A smaller screen is seen from nearer, so its UI grows by g = sqrt(299 / height mm). Retail's menus get UI pixels of
  that size: f = 1080 / (lines * g) of the window, the nearest of Settings' Menu Size steps (95 %, 90 % ... down to 512
  lines), the window's from 97.5 % up. The modern layer: 60 * g over the system's scale (it follows that scale
  already), in steps of 5, never below 60 or above 150. Without an EDID size retail's menus follow the system's scale
  (f = 1 / scale) and the modern layer keeps 60.
- Examples: 24-inch 1080p 296 mm, unchanged; 32-inch 1440p 393 mm, menus 85 %, 60 %; 7-inch 1280x800 handheld, 75 %,
  105 %; 13-inch 1080p laptop, 75 %, 80 % (65 % at the system's 125 %); 27-inch 4K at 150 % in a 4K window, 55 %, 60 % (a first
  start's 1280x800 window stops at 65 %, its last step of 512 lines or more).
- Seen live on Linux (Wayland, four monitors): a new config opened on DP-1, a 32-inch 2560x1440 (698 x 393 mm by its
  EDID, found by name), and took Menu Size 1088x680 (85 % of the 1280x800 window) and Interface Size 60 %. Windows
  builds (checked with the GNU target) and macOS are not seen live yet.
- Not covered: a screen streamed to a phone or tablet shows the host's monitor, whose size says nothing of the phone.
- Menu Size is pixels, so it stays a step of the window: a new Resolution in Settings (not borderless at the desktop's
  size, 0034 = 3, which keeps the window) moves a Menu Size that is a step of the old window to the same step of the
  new one (the last there is, else the window's), and the page's size lists follow the new window. Before, a first
  start's 1088x680 (85 % of 1280x800) stretched over 2x in a 2560x1440 window, and Settings showed it as the window's.
  The steps are one list, config::ui_steps, for Settings and the first start.
