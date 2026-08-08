# Changelog

All notable changes to `py-chunks` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Seeded 2026-08 from the git history for 0.4.5 → 0.6.0; earlier releases are
summarised only by their tags.

## [0.6.2] - 2026-08-08

### Fixed
- **`stream_chunk_doc`, `stream_chunk_ppt` and `stream_chunk_docx` skipped
  argument validation.** `stream_chunk_doc` and `stream_chunk_ppt` ran no
  mode or numeric check at all, so a typo'd `mode` silently streamed
  *default-mode* chunks instead of raising — the quietest possible failure.
  `stream_chunk_docx` raised `NotImplementedError: Streaming for <mode> mode
  coming soon` (every mode streams; nothing was unimplemented) and checked only
  `window_size`. All three now run the same `_validate_*_options` their batch
  and bytes routes run, so an invalid mode or argument is a `ValueError`
  wherever you enter. **`stream_chunk_docx` no longer raises
  `NotImplementedError`** — catch `ValueError`. The identical bug was fixed for
  `stream_chunk_pptx` earlier and never propagated to its three siblings.
- **EPUB accepted invalid mode arguments and returned no chunks.**
  `get_chunks("book.epub", mode="sliding_window", window_size=100,
  overlap=100)` returned **0 chunks** where a valid call returns 1,440. The
  engine's EPUB facade never ran the shared argument check, and its per-chapter
  builder failures are deliberately swallowed (an image-only cover page must not
  abort a whole book), so a caller mistake became "this book is empty". Fixed in
  the engine: `ValueError`, with the same message every other format raises.
- **Spreadsheets silently accepted `paragraphs_per_page=0`.** The spreadsheet
  family paginates by `rows_per_chunk`, so `paragraphs_per_page` was dropped at
  the dispatch mapping site and never validated — while three docs pages promise
  it is rejected. `get_chunks("book.xlsx", mode="page_aware",
  paragraphs_per_page=0)` (and the bytes and streaming routes) now raise
  `ValueError`.

### Changed
- **One wording per rejection.** `chunkers/doc.py`, `chunkers/ppt.py` and
  `chunkers/docx.py` said `window_size must be > 0` (and doc/ppt also
  `sentences_per_chunk must be > 0`, `paragraphs_per_page must be > 0`) where
  the engine says `... must be greater than 0`; `chunkers/xlsx.py` said
  `window_size must be >= 1`. All now use the engine's verbatim strings, held in
  module-level constants shared by the batch, streaming and with-images entry
  points (the pattern `chunkers/pptx.py` already used). The exception **type** is
  unchanged (`ValueError`) — code matching on message *text* for these four
  formats must update. Branch on the exception type, not the message.
- **Python 3.9 compatibility (the declared floor was broken).** `import
  py_chunks` raised `TypeError: unsupported operand type(s) for |: 'type' and
  'NoneType'` on CPython 3.9 — 41 runtime-evaluated PEP 604 unions (`str |
  None` and friends) in function signatures across `_sources.py`,
  `chunkers/csv.py`, `chunkers/tsv.py`, `chunkers/pptx.py` and
  `chunkers/xlsx.py`. `from __future__ import annotations` is now present in
  all 22 package modules (and in the two test modules that had the same
  problem), so annotations are never evaluated at definition time. Verified by
  importing and running the cp39-abi3 wheel on CPython 3.9.6. `requires-python
  = ">=3.9"` and the 3.9 classifier are now true statements.

### Added
- `py_chunks.__version__`, read from the installed distribution metadata via
  `importlib.metadata`, so it cannot drift from what pip installed. Exported
  through `__all__`.
- A **Python 3.9 wheel-test job** in the release workflow (`test-linux-py39`),
  gating `publish` alongside the existing 3.11 job. The 3.11 job proves the
  stable ABI spans versions; the 3.9 job proves the Python sources contain no
  post-3.9 syntax — a class of breakage no other job in the matrix could see.

## [0.6.1] - 2026-08-08

### Added
- No-filesystem bytes API bound from the engine: `get_chunks_from_bytes`,
  `get_chunks_with_images_from_bytes`, `get_markdown_from_bytes`,
  `get_markdown_with_images_from_bytes`, `chunk_csv_from_bytes` and
  `csv_to_markdown_from_bytes` in `py_chunks._rust`. The Python
  bytes/file-object/upload/URL paths now call these directly instead of
  round-tripping through a temp file (output verified byte-identical).
- Type information: `py.typed` marker, complete `_rust.pyi` stub covering the
  whole compiled surface (222 names), and the `Typing :: Typed` classifier.
- Linux aarch64 wheels in the release workflow, plus a wheel-install test job
  (installs the built cp39-abi3 wheel on Python 3.11 and runs the
  corpus-independent test subset).
- `__all__` now exports every format module's `chunk_*`/`stream_chunk_*` pair
  (18 modules); README module list updated to match.
- This changelog.

### Changed
- Wheels are built against pyo3's `abi3-py39` stable ABI: one wheel per
  platform now covers CPython 3.9+ (previously cp313-only, so 3.9–3.12 users
  fell back to a source build).
- The GIL is released while the Rust engine parses and chunks
  (`Python::allow_threads` across all entry points, streaming constructors and
  iterator pulls) — a long parse no longer blocks other Python threads.
- `py_chunks/__init__.py` refactored: format routing tables and helpers moved
  to `py_chunks/_dispatch.py`, source-agnostic entry points to
  `py_chunks/_sources.py`; the twin 17-branch streaming ladders are now one
  table-driven router. Public API unchanged.
- Stale docstrings that listed 8–9 formats now describe the full 36-format
  surface.
- PyPI metadata: project URLs (homepage/docs/repository), full classifier set,
  and an accurate description.

### Fixed
- Two image test suites resolved fixtures relative to the working directory
  and silently skipped unless pytest ran from the repo root; they now use the
  same absolute fixture resolution as every other suite.
- 17 stale skip-reason strings referencing a nonexistent `tests/fixtures/`
  directory.
- Streaming-from-bytes no longer leaks its temp file when iterator
  construction fails (e.g. an invalid mode).
- Stale 0.4.x wheels removed from git tracking; `dist/` ignored and excluded
  from the crate archive.

## [0.6.0] - 2026-08-07

The consolidation release: the per-format Rust fork inside this package was
retired and every format now binds the single vendored `rs-chunks` engine
shared with `js-chunks` and the Rust crate.

### Changed
- All 17 format binding groups migrated onto the vendored engine
  (`crates/rs-chunks`); the PyO3 layer is generated by binding macros and
  contains no parsing logic of its own.
- The engine now owns PDF parsing (#57, #74); `structural` mode on PDF routes
  to the real structural pipeline instead of `default` (#54).
- Streaming yields chunks lazily instead of pre-building every dict (#55).
- Image names are identical across the Python/JS/Rust SDKs (#76).
- Dropped the unused `pypdfium2` dependency — PDFium ships inside the wheel
  via `liteparse` (B3).

### Added
- Dozens of ported format fixes ahead of the migration, including: .doc
  tables/list depth/breadcrumbs (#12), .doc FIB/piece-table offsets (#10),
  JSON `record_range` metadata (#46), xlsx workbook repair and skipped-sheet
  reporting (#8, #21, #66), email threading headers and mbox per-message
  metadata (#36, #37), eml charset-mismatch retry (#72), msg legacyDN and
  image attachments (#47, #48), epub TOC/metadata (#38–#40), pptx SmartArt
  and embedded charts (#4, #15), docx multi-image and table-cell image
  extraction (#13, #71), txt encoding detection (#30–#32), markdown/html
  entity and structure fixes (#22, #27, #29, #41, #42, #45, #63, #64).

### Fixed
- Post-migration engine re-syncs: PDF subset-font decoding (#84, #97), column
  split (#91, #94), inline-image strip hardening (#86), base-14 font metrics
  (#92, #93), whitespace measurement (#96); ipynb ANSI stripping (#44); msg
  RTF codepages (#49); ODS named ranges (#20); bounded over-long lines (#68);
  normalised line endings (#89, #90).
- The pylint gate is real and the built `.so` is no longer tracked (#67, #69).
- Release packaging: the vendored engine ships the README its manifest names.

## [0.5.0] - 2026-07-28

### Added
- Seven new formats: `.eml`/`.mbox` (MIME email), `.odt`/`.odp`
  (OpenDocument), `.json`/`.jsonl`/`.ndjson` (record-per-chunk).
- Image extraction for the spreadsheet family and HTML.

### Changed
- PDF parsing rewritten on `liteparse` (vendored PDFium) — the previous PDF
  stack and its external binary resolution are gone.

## [0.4.7] - 2026-07-02

### Added
- Image extraction for legacy `.doc` and `.ppt` via MS-ODRAW BLIP records
  (JPEG/PNG).

## [0.4.6] - 2026-06-19

### Fixed
- README: PPT in the description, simplified XLSX/CSV quick start, `.pdf`
  listed under `list_images` support.

## [0.4.5] - 2026-06-17

Baseline for this changelog. Earlier history (0.1.x–0.4.4) predates it; see
git tags.

[Unreleased]: https://github.com/RanjanKudesia/py-chunks/compare/v0.6.2...HEAD
[0.6.2]: https://github.com/RanjanKudesia/py-chunks/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/RanjanKudesia/py-chunks/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/RanjanKudesia/py-chunks/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/RanjanKudesia/py-chunks/compare/v0.4.7...v0.5.0
[0.4.7]: https://github.com/RanjanKudesia/py-chunks/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/RanjanKudesia/py-chunks/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/RanjanKudesia/py-chunks/releases/tag/v0.4.5
