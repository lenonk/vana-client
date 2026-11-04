## Training regime: kept per server and character (2026-11-03)

Ours (a fix to the Extra, not retail). The regime state was kept by character name alone: in the data dir for a plain
run, beside the `FFXI_CONFIG` copy for the wrapper's runs. A regime begun in one kind of run was missing from the
other, so after a restart the Training window stayed away until the next kill credit, and then came back with only
the monster's name. It is now kept in the data dir for every run, a folder per server (its search address,
"127.0.0.1-54002") and a file per character (`config::regime_path`): it mirrors what that server keeps, so both kinds
of run agree and a same-named character on another server has its own. With no file there yet, a zone-in reads the
newest by-name file of an earlier build that parses (a kept `null`, a cancel, counts as read).
