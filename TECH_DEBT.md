# Known defects — tracked for the next release

Found during hands-on quality review against real documents (see
`QUALITATIVE_all_round.md` for the full repro). Listed here so the public benchmark
report can disclose them honestly instead of hiding or silently working around them.
Benchmark numbers in this round were captured against the **current** code; re-run the
affected experiments once these land.

## 1. `section` mode: document intro lands out of reading order

- **Repro:** `all_round.docx`, `mode="section"`.
- **Symptom:** the document's intro/title paragraph ("Sample Document / This document
  was created…") is emitted as **chunk[7]** instead of chunk[0]. All other chunks are in
  correct reading order.
- **Where:** section-boundary bookkeeping in the section chunker — the intro block
  (before the first heading) appears to be flushed after later sections rather than
  first.
- **Affects:** `benchmarks/competitive` reading-order check; any RAG use of `section`
  mode where document order matters for citation/provenance.

## 2. `get_markdown` drops body text adjacent to inline images

- **Repro:** `all_round.docx`, Images section — an inline image is immediately followed
  by a body paragraph ("Documents may contain images. For example…"), confirmed present
  in `word/document.xml`.
- **Symptom:** `get_markdown()` renders only the `[Image: Web Access Symbol]` placeholder
  and **drops the following paragraph**. `get_chunks()` on the same input keeps the
  paragraph text but drops the image marker instead — the two export paths are
  **inconsistent** on inline-image paragraphs.
- **Impact today:** chunk-based RAG is unaffected (text is retained via `get_chunks`);
  markdown export specifically loses content. Still a correctness bug in `get_markdown`.

## 3. `semantic` mode fragments numbered/bulleted lists

- **Repro:** `all_round.docx`, `mode="semantic"`.
- **Symptom:** the 6-item numbered list (with a nested bullet sub-list under item 5) is
  broken into tiny 27–35 character bullet-piece chunks instead of being kept as one
  `bullet_list` unit, unlike `default`/`structural` and `section` which isolate it
  correctly.
- **Affects:** structure-integrity scoring for `semantic` mode specifically; other modes
  are not affected.

## 4. PPTX: some files yield zero chunks in `default` mode

- **Repro:** `poi_SmartArt.pptx`, `poi_tika-2605.pptx` (Apache POI test corpus,
  `test_files/pptx/`), `mode="default"`.
- **Symptom:** `get_chunks()` returns an empty list — no text extracted at all. Cascades
  into every downstream assertion (schema, metadata, streaming) for these two files in
  the pytest suite (54 failures total from this one root cause).
- **Where:** likely PPTX text extraction not covering SmartArt diagram text (stored in
  `diagrams/data*.xml`, not regular shape text) and/or whatever `tika-2605` stresses
  (named for an Apache Tika bug ticket — check what structure that file exercises).
- **Affects:** any deck using SmartArt diagrams; low real-world frequency but a hard
  content-loss failure (0 chunks, not degraded chunks) when it hits.

## 5. DOCX: heading level not clamped to the documented 1–6 range

- **Repro:** `poi_bug59058.docx`, `mode="sentence"` (also surfaces in
  `structural`/`semantic`/`section`).
- **Symptom:** `metadata["source_paragraph_heading_level"]` (or the mode's equivalent
  heading-level field) comes back as `7` for a paragraph styled "Heading 7" in Word.
  Schema/tests assume levels are always `1..=6` (matching HTML/Markdown H1–H6
  semantics), so callers relying on that contract can get an out-of-range value.
- **Where:** wherever the docx heading-level style (`w:pStyle` → `HeadingN`) is parsed
  into an integer — needs clamping (or the schema needs to officially allow 7–9, which
  Word supports as outline levels).
- **Affects:** `test_docx.py` heading-level-range assertions (4 failures); any consumer
  that trusts the 1–6 contract.

## 6. DOCX: `semantic` mode yields zero chunks on two fixtures

- **Repro:** `poi_saut_page.docx`, `poi_chartex.docx`, `mode="semantic"` only (other
  modes not checked in detail — verify before fixing).
- **Symptom:** `get_chunks(..., mode="semantic")` returns an empty list.
- **Where:** not yet diagnosed — both are single-mode, isolated failures (1 each), likely
  two unrelated small edge cases in the semantic chunker's input handling for these
  specific document structures. Investigate independently.

## 7. Test suite gap (not an engine bug): encrypted DOCX fixture breaks the generic parametrized tests

- **Repro:** `poi_bug53475-password-is-pass.docx` — a password-protected DOCX.
- **Symptom:** `get_chunks()` correctly raises a clean `RuntimeError: Failed to parse
  DOCX: ... Could not find EOCD` (encrypted OOXML isn't a valid zip to the reader without
  the password) — this is the **desired** behavior, not a defect. But `test_docx.py`'s
  fixture discovery globs every `.docx` in `test_files/docx/` and assumes each one parses
  successfully, so this one file cascades into 66 failures across nearly every
  parametrized test class.
- **Fix:** exclude known-adversarial/encrypted fixtures from the "expect success" glob
  (or add a dedicated `pytest.raises(RuntimeError)` test for them), not an engine change.
- **Why it matters now:** `test_files/` became a shared corpus across py-chunks/
  chunks-rs/js-chunks this cycle (Apache POI test-corpus fixtures added for adversarial
  Rust/WASM testing landed in the same shared directory) — py-chunks' pytest suite was
  written against a curated 8-document set and had never run against the expanded corpus
  until this release cycle surfaced it.

---
Note: items 4–7 above were found running the full pytest suite against the expanded
shared `test_files/` corpus ahead of the v0.5.0 release (2026-07-28) — not caused by
this release's changes (liteparse 2.9.0 bump, docx `max_chars` char-boundary panic fix,
`semantic` `primary_merge_reason` tie-break determinism fix), which are unrelated and
verified clean (4214/4340 pytest pass; only the fixtures above fail, plus the
pre-existing `pptx section_divider_slide_emits_heading_chunk` failure already known).
Deferred to the next release per owner's call — not blocking v0.5.0.

Status: open, scheduled for the upcoming release (~2 days out as of 2026-07-28).
Owner: fixing directly in `src/extensions/md/`.
