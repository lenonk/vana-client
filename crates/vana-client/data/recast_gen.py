"""Writes data/recasts.txt from LandSandBoat's sql/abilities.sql: each ability's recast timer id and base recast.

usage: python3 -I recast_gen.py LSB_DIR OUT

The ability DAT has no recast lengths (the server sends only what is left, in s2c 0x119). These are LSB's base
recasts, before gear, merits and job points. LSB's abilityId is the client's ability id less 0x200.
"""
import re
import sys
from pathlib import Path

ROW = re.compile(r"INSERT INTO `abilities` VALUES \((\d+),'([^']*)',\d+,\d+,\d+,(\d+),(\d+),")


def main(lsb, out):
    sql = (Path(lsb) / "sql" / "abilities.sql").read_text()
    rows = sorted((int(m[1]), m[2], int(m[3]), int(m[4])) for m in ROW.finditer(sql))
    lines = [
        "# vana-client data/recast_gen.py from LandSandBoat sql/abilities.sql (recastTime, recastId)",
        "# id (LSB abilityId) timer seconds name",
    ]
    lines += [f"{i} {timer} {secs} {name}" for i, name, secs, timer in rows if secs > 0]
    Path(out).write_text("\n".join(lines) + "\n")
    print(f"{len(lines) - 2} abilities")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
