## Auto-translate in a non-English client; the client language (2026-11-19)

<!-- dll-build: 30261102_0 -->

Modlog-gaps item 162 ("Auto-translate in a non-English client: French second words, the second title word, the
per-language word compares, how the resolver finds phrase id N when French doubles the words; German FUN_10191710").
Addresses are 30261102_0. Decompiled with Ghidra; the word builders' `this` pointers, the title words and the quote
gate checked in Rizin.

The client language:
- DAT_10457bd8 +8 (0 ja, 1 en, 2 de, 3 fr). PlayOnline fills it: FUN_100157d0 hands the host &DAT_10457b58 and
  FUN_100157e0 copies the block's +0x28 (DAT_10457b80) to +8, and the block's +0x4c bit 0 (DAT_10457ba4) to +0x18
  bit 7, the flag that makes German and French read the English files and compare as English (FUN_1025cb90,
  FUN_101b67e0, FUN_101bde00). A debug switch (FUN_101d3fe0, flag 4) can name it ("japanese" .. "french") or step it.
- Ours: config.json `language` ("ja" / "en" / "de" / "fr", default "en"), set process-wide at Config::load
  (dat::lang). The flag stays clear (ours has no install without the European files). No Settings row.
- The language tables: FUN_1025cb20 picks DAT_1038bbd0 (ja), DAT_1038b610 (en), DAT_1038b450 (de), DAT_1038b7d0 (fr);
  slot 0x18 of each is 55545 / 55665 / 55785 / 56205. tables.rs had the German and French tables' names swapped (the
  index was right); fixed.

The dictionary in the client's language:
- FUN_101b67e0 sets the dictionary's language byte to the files' language + 1 (FUN_101bc590) and FUN_101bcd70 reads
  slot 0x18 of that language, keeping the groups of that byte or of header kind 1 (no file has one). The built Items
  and Key Items groups and their blocks carry the same byte (FUN_101bc880 / FUN_101bcc50 / FUN_101bc770 /
  FUN_101bca80); ours wrote 2 (English) everywhere.
- A category's words (FUN_101bc090): title words first, then the entries'. Entry words: FUN_101bbfa0 (English, German):
  one word keyed by its text. FUN_101bbe00 (Japanese, French): a second string follows each entry outside the item
  kinds 7..0x12; longer than its NUL it is the key (FUN_101bddc0), and French adds a second word keyed by the text
  (FUN_101bdd80), so a French entry with a second string is two words (550 entries). Words go in order (the `this`
  of each call steps by one word, 0x101bbf04 / 0x101bbf4e). An item kind's word keeps its NUL in its length.
- Title words: the title keyed by the completion; in French (FUN_101bbd10, header language 4) a second one keyed by the
  title inside its brackets (text + 2, length - 4, 0x101bc265) when the title from its third byte differs from the
  completion over the completion's length under FUN_101c9e70 with DAT_1037e358 (an ordering table that keeps accented
  letters apart): nine groups ("【Réponses】" / "Reponses" ...). The Items group can have one too; Key Items cannot (the
  pairs path, FUN_101bbcb0).
- `@` texts (FUN_101bbba0): `@A` group 0x14 in Japanese and English, 0x15 in German and French; `@M` is group 0x13
  (ours lacked it; no file uses it); all names in the files' language.
- Compares (FUN_101bde00 for keys, FUN_101bded0 for texts): Japanese _strncmp, English _strnicmp (FUN_10327340),
  German and French FUN_101c9e70 with DAT_1037e3a0 (accents off; FUN_101c9d70 folds A..Z, a..z and the 0x85 letters
  through the table, a signed difference). The word order (vtable slot 0) compares the other key with the word's over
  the word's key length.
- Sorting: each category's entry words (not its titles; not Key Items, which stay in table order) and the merged array
  of every category's words (FUN_101bdaf0: titles in, empty texts and a lone 6-byte block out) are selection-sorted by
  that order. Ours sorted a merged list by Unicode lower case.
- Look-up (FUN_101b84b0 -> FUN_101bd060 / FUN_101bd230 / FUN_101bd430): the typed word has its hiragana turned to
  katakana (FUN_10191360), then a binary search of the merged array (or of a category's sorted words) with the
  compare over the typed length, walked out to the whole run; the run is listed (at most 0x3fb looked at) without
  texts starting with a space and without a text already listed (same length, the text compare). A category the
  finder (FUN_101bc640: file groups by group byte, then built groups by kind) does not know lists "エラー"
  (DAT_1037da7c). An empty category or no match lists the categories' titles.
- The resolver (FUN_101bc410): words[id] in every language but French; in French the scan from words[id] to the first
  word whose id byte is the id (the doubled words push entry N past index N). Every file's entry k has id k, so
  ours' look-up by id gives the same text.

German's quotation marks (FUN_10191710; language 2 and the flag clear): each lone `"` (a one-byte character by
FUN_10191050 / FUN_10190ff0, called with second byte 0) becomes 85 44, the next 85 53, by turns (*inf: German's „ and “; the font's glyphs not checked*); a line of
half the caller's buffer or more is left alone. Callers: the log (FUN_10129090, lines whose mode word's second byte has
no low five bits, 0x800), the chat line (FUN_101b9c30, 0x968), the macro editor's lines not being edited
(win_macro_v10, 0x98), the text field (FUN_101c76b0, 0x41a, 15 windows), the search comment printer (FUN_101ded50,
0x52) and the comment window (win_comment_v10, 0x200).

Ours now:
- dat::lang (client / files), Config.language.
- autotrans: Word (text, key, lengths, id), title_words / entry_words, the compares (strncmp / strnicmp / fold_cmp with
  FOLD_ORDER / FOLD_ACCENTS), select_sort, search, listed, kana; Dictionary::load_lang; the built groups and the merged
  array made again together; blocks with the dictionary's language byte (id_block, phrase_block, the Lua blocks).
- font::german_quotes (char_len = FUN_10191050): chat lines, the chat line being typed, the comment window (its lines
  and live box), the macro editor (lines, live title and line).
- Tests: `dat::autotrans::tests::{fold_tables, compares, search_finds_the_run, french_words, japanese_words,
  german_places}`, `dat::font::tests::german_quote_marks`.

Not built (new queue row 174): every other language-dependent part of the client (133 reads of DAT_10457bd8 +8: the
message files of each language slot, item / key item / spell names, the "a" / "an" articles, number and date formats,
the faq / help areas, /itemsearch's language byte, ...); while only the dictionary follows `language`, the built Items
and Key Items words take their names from the English name tables. German quotes on the other text fields
(FUN_101c76b0's other callers), on log lines outside push_chat_line, and in the search comment printer (ours has no
equivalent of FUN_101ded50 yet). Not checked live (no German or French account to test with).

Found on the way (new queue row 175): the macro editor draws a line that is not being edited with the auto-translate
brackets (win_macro_v10: FUN_101b8850(.., 0x98, 1, 0), the same call the comment window makes); ours draws it plain
(macroedit::shown). Search with a typed word inside a category (FUN_101bd060 with both) is built in the dictionary,
but the candidate list only asks a category with no word, as before.

Review notes: typed accented letters are not found because ours encodes é as `?` (encode_sjis has no 0x85 row; the
European text encoding is part of row 174); typing the word without accents finds it, as the folded compare does. A
typed word inside the Key Items category is searched over an unsorted list, as retail does (FUN_101bd230 on
FUN_101bc480, which FUN_101bbcb0 leaves in table order). The modern layer draws German's marks as „ and “.
