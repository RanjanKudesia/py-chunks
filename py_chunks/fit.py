"""Fit chunks to a token budget — the binding-level half of TECH_DEBT C1.

Why this lives in Python and not in the engine
----------------------------------------------
Token counting is tokenizer- and language-specific: tiktoken here, something
else in JS. Putting a token budget in ``rs-chunks`` would mean either vendoring
one tokenizer (wrong for everybody using a different embedding model) or calling
back into the host language (which breaks the byte-identical parity guarantee
that is the product's whole point). Characters are parity-safe; tokens are not.

So the engine keeps producing byte-identical, structure-aware chunks, and this
module re-fits them to *your* tokenizer. It is **deliberately parity-exempt**:
output depends on the counter you pass, so it is not part of the cross-SDK
byte-identical contract, and it says so here rather than pretending otherwise.

What it buys you
----------------
semchunk guarantees "0 chunks over budget" but only ever sees a ``str``. Running
this over ``get_chunks()`` output gives the same guarantee on all 36 formats,
while keeping ``content_type`` and ``metadata`` — including the repeated table
headers that make a retrieved row interpretable.

    >>> import py_chunks, tiktoken
    >>> enc = tiktoken.get_encoding("cl100k_base")
    >>> chunks = py_chunks.get_chunks("report.pdf")
    >>> fitted = py_chunks.fit_tokens(chunks, lambda s: len(enc.encode(s)), 512)

Every knob defaults to preserving what the engine gave you, so calling it with
just a counter and a budget only ever *splits* what is over budget.
"""

from __future__ import annotations

import re
from typing import Callable, Iterable, Literal

__all__ = ["fit_tokens"]

TokenCounter = Callable[[str], int]

_SENTENCE_END = re.compile(r"(?<=[.!?])\s+")
_PARAGRAPH = re.compile(r"\n\s*\n")

_SPLITS = ("sentence", "paragraph", "hard")
_MERGES = ("forward", "none")
_MERGE_METADATAS = ("first", "union")
_OVERSIZES = ("split", "keep", "error")


def _check_enum(name: str, value: str, allowed: tuple[str, ...]) -> None:
    """Reject an unrecognised option instead of silently falling through.

    A typo like ``split="sentances"`` would otherwise quietly select the hard
    whitespace split — a different chunking strategy, applied without complaint.
    ``js-chunks`` validates the same four options the same way.
    """
    if value not in allowed:
        options = ", ".join(repr(v) for v in allowed)
        raise ValueError(f"{name} must be one of {options}, got {value!r}")


def _split_text(text: str, how: str) -> list[str]:
    """Break `text` into candidate pieces, largest-grain first."""
    if how == "paragraph":
        parts = [p for p in _PARAGRAPH.split(text) if p.strip()]
    elif how == "sentence":
        parts = [p for p in _SENTENCE_END.split(text) if p.strip()]
    else:  # "hard" — last resort, split on whitespace
        parts = text.split()
    return parts or [text]


def _pack(pieces: list[str], counter: TokenCounter, budget: int, joiner: str) -> list[str]:
    """Greedily pack `pieces` into runs that fit `budget`."""
    out: list[str] = []
    cur: list[str] = []
    for piece in pieces:
        candidate = joiner.join(cur + [piece]) if cur else piece
        if cur and counter(candidate) > budget:
            out.append(joiner.join(cur))
            cur = [piece]
        else:
            cur.append(piece)
    if cur:
        out.append(joiner.join(cur))
    return out


def _merge_meta(a: dict, b: dict, how: str) -> dict:
    """Combine two chunks' metadata.

    ``"first"`` keeps the leading chunk's values — never invents one. ``"union"``
    collects differing scalars into a list so a chunk spanning two pages reports
    both rather than silently claiming one, which would be a fabricated value.
    """
    if how == "first":
        return dict(a)
    merged = dict(a)
    for k, v in b.items():
        if k not in merged:
            merged[k] = v
        elif merged[k] != v:
            cur = merged[k] if isinstance(merged[k], list) else [merged[k]]
            merged[k] = cur + (v if isinstance(v, list) else [v])
    return merged


def fit_tokens(
    chunks: Iterable[dict],
    counter: TokenCounter,
    budget: int,
    *,
    min_tokens: int = 0,
    merge: Literal["forward", "none"] = "forward",
    split: Literal["sentence", "paragraph", "hard"] = "sentence",
    merge_metadata: Literal["first", "union"] = "first",
    oversize: Literal["split", "keep", "error"] = "split",
    respect_boundaries: bool = True,
    boundary_keys: tuple[str, ...] = ("section_heading", "page_number", "sheet_name"),
) -> list[dict]:
    """Re-fit engine chunks to a token budget.

    Args:
        chunks: output of ``get_chunks`` (or any ``{content, content_type,
            metadata}`` sequence).
        counter: ``str -> int``. Any tokenizer: tiktoken, HuggingFace, custom.
        budget: maximum tokens per returned chunk.
        min_tokens: merge chunks below this into the next one. ``0`` disables.
        merge: ``"forward"`` joins an undersized chunk into the one after it;
            ``"none"`` leaves short chunks alone.
        split: where to break a chunk that exceeds `budget`.
        merge_metadata: ``"first"`` or ``"union"`` — see :func:`_merge_meta`.
        oversize: what to do when a single indivisible piece still exceeds
            `budget` — ``"split"`` cuts it hard, ``"keep"`` emits it whole,
            ``"error"`` raises naming the chunk.
        respect_boundaries: never merge across a change in `boundary_keys`.
            This is on by default because merging across a section or page makes
            the resulting chunk's metadata a lie, and because table-header
            repetition — the library's biggest measured retrieval advantage — is
            exactly what careless merging destroys.
        boundary_keys: metadata keys treated as structural boundaries.

    Returns:
        A new list. Inputs are not mutated.
    """
    _validate(counter, budget, min_tokens, merge, split, merge_metadata, oversize)

    items = [dict(c) for c in chunks]
    if not items:
        return []

    # ---- pass 1: merge undersized chunks forward -------------------------
    if merge == "forward" and min_tokens > 0:
        merged = _merge_forward(
            items,
            counter,
            budget,
            min_tokens=min_tokens,
            merge_metadata=merge_metadata,
            respect_boundaries=respect_boundaries,
            boundary_keys=boundary_keys,
        )
    else:
        merged = items

    # ---- pass 2: split whatever is over budget ---------------------------
    out: list[dict] = []
    for c in merged:
        out.extend(_refit_one(c, counter, budget, split, oversize))
    return out


def _validate(  # pylint: disable=too-many-arguments,too-many-positional-arguments
    counter: TokenCounter,
    budget: int,
    min_tokens: int,
    merge: str,
    split: str,
    merge_metadata: str,
    oversize: str,
) -> None:
    """Reject every argument the two passes would otherwise misinterpret."""
    if budget < 1:
        raise ValueError("budget must be greater than 0")
    if min_tokens < 0:
        raise ValueError("min_tokens must not be negative")
    if not callable(counter):
        raise ValueError(f"counter must be callable, got {type(counter).__name__}")
    _check_enum("merge", merge, _MERGES)
    _check_enum("split", split, _SPLITS)
    _check_enum("merge_metadata", merge_metadata, _MERGE_METADATAS)
    _check_enum("oversize", oversize, _OVERSIZES)


def _boundary(c: dict, boundary_keys: tuple[str, ...]) -> tuple:
    """The boundary signature of one chunk: its `boundary_keys` values as text."""
    meta = c.get("metadata") or {}
    return tuple(str(meta.get(k)) for k in boundary_keys)


def _merge_forward(  # pylint: disable=too-many-arguments,too-many-positional-arguments
    items: list[dict],
    counter: TokenCounter,
    budget: int,
    *,
    min_tokens: int,
    merge_metadata: str,
    respect_boundaries: bool,
    boundary_keys: tuple[str, ...],
) -> list[dict]:
    """Pass 1 — join each undersized chunk into the one that follows it."""
    merged: list[dict] = []
    pending: dict | None = None
    for c in items:
        if pending is None:
            pending = c
            continue
        same = (not respect_boundaries) or _boundary(pending, boundary_keys) == _boundary(
            c, boundary_keys
        )
        joined = f"{pending['content']}\n\n{c['content']}"
        if counter(pending["content"]) < min_tokens and same and counter(joined) <= budget:
            pending = {
                **pending,
                "content": joined,
                "metadata": _merge_meta(
                    pending.get("metadata") or {}, c.get("metadata") or {}, merge_metadata
                ),
            }
        else:
            merged.append(pending)
            pending = c
    if pending is not None:
        merged.append(pending)
    return merged


def _refit_one(
    c: dict,
    counter: TokenCounter,
    budget: int,
    split: str,
    oversize: str,
) -> list[dict]:
    """Pass 2 — `c` unchanged when it already fits, otherwise its split parts."""
    text = c.get("content") or ""
    if counter(text) <= budget or oversize == "keep":
        return [c]
    if oversize == "error":
        raise ValueError(
            f"chunk of {counter(text)} tokens exceeds budget {budget} and "
            f"oversize='error' (content_type={c.get('content_type')!r})"
        )

    pieces = _pack(_split_text(text, split), counter, budget, " ")
    # A single piece can still exceed the budget (one very long sentence, or a
    # table row that must not be cut mid-record). Fall back to a coarser split
    # for those rather than emitting something over budget.
    final: list[str] = []
    for piece in pieces:
        if counter(piece) <= budget:
            final.append(piece)
        else:
            final.extend(_pack(_split_text(piece, "hard"), counter, budget, " "))

    out: list[dict] = []
    for i, piece in enumerate(final):
        meta = dict(c.get("metadata") or {})
        meta["fit_part"] = i + 1
        meta["fit_total"] = len(final)
        out.append({**c, "content": piece, "metadata": meta})
    return out
