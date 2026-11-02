## Message formatter: loops stop (ours, 2026-11-02)

- msgfmt::format (FUN_10153310) followed the jump codes 0x84..=0x8a to any signed-byte target, so a string whose
  jump went back to or before itself (`A FA 40 84 FB`) repeated forever and grew memory until the process was
  killed (issue #88, found by fuzzing). Where the formatter goes next depends only on the position, the compare
  flags and the choice end, so after each backward jump it now records that state and stops at the first repeat,
  returning the output so far. A string that ends never repeats a state, so retail strings format as before.
  Not checked against retail; what FUN_10153310 does with such a string is not known (ours).
