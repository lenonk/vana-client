## Recording: late frames keep time, frames go out in order (ours, 2026-11-03)

- `client.record` sent at most one frame per drawn frame, so drawing slower than the recording's rate gave a video
  shorter than the capture (30 fps asked, 15 drawn: half the length). A frame now fills every interval it covers (at
  most a second's worth after a stall), the same for the "no buffer free, send the last again" case.
- Readback buffers were sent as they mapped, sorted only among those mapped together, so a later frame mapping first
  went out before an earlier one. Each copy now has a number and they are sent strictly in that order (a failed map
  sends the last frame again in its place). Both from the Devin review of the upstream merge (PR #112).
- A late frame goes to the encoder thread as one item with its count, and that thread writes it that many times, so
  a burst of repeats can't overflow the 16-item queue and get dropped. Stopping sends the frames already mapped, in
  order (a copy still mapping counts as the last frame again), instead of dropping them (Devin review of #115).
