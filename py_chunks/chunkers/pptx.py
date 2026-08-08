"""PPTX chunker wrapper over the Rust extension."""

from __future__ import annotations

# pylint: disable=c-extension-no-member,too-many-arguments,too-many-positional-arguments
import time
import warnings
from pathlib import Path

from py_chunks import _rust

_PPTX_SUFFIXES = (".pptx", ".potx", ".potm", ".ppsx", ".ppsm")
_PPTX_MODES = {
    "default", "structural", "semantic", "section",
    "sliding_window", "sentence", "page_aware",
}

# One wording per rejection, shared by every entry point. `slides_per_chunk` is
# the preferred public name for the page_aware parameter, but the message keeps
# the legacy `paragraphs_per_page` spelling it has always used — the two are
# aliases and renaming it here would break callers matching on the text.
_E_WINDOW = "window_size must be greater than 0"
_E_OVERLAP = "overlap must be less than window_size"
_E_SENTENCES = "sentences_per_chunk must be greater than 0"
_E_SLIDES = "paragraphs_per_page must be greater than 0"


def _validate_pptx_mode_args(
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    """Raise for argument values the requested mode cannot use.

    Shared by the batch, streaming and with-images entry points so all three
    reject the same argument with the same wording. They used to each have
    their own copy, and the streaming one skipped this entirely and let the
    Rust layer raise — in a different wording again.
    """
    if mode == "sliding_window":
        if window_size <= 0:
            raise ValueError(_E_WINDOW)
        if overlap >= window_size:
            raise ValueError(_E_OVERLAP)
    if mode == "sentence" and sentences_per_chunk <= 0:
        raise ValueError(_E_SENTENCES)
    if mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError(_E_SLIDES)


def _validate_pptx_options(
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
    if mode not in _PPTX_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_PPTX_MODES)} for PPTX, got: '{mode}'"
        )
    _validate_pptx_mode_args(
        mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def _validate_pptx_args(
    file_path: str,
    path: Path,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    """Raise descriptive errors for invalid chunk_pptx arguments."""
    if not path.is_file():
        raise FileNotFoundError(f"PPTX file not found: {file_path}")
    if path.suffix.lower() not in _PPTX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_PPTX_SUFFIXES)}, got: {file_path}")
    _validate_pptx_options(
        mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def _call_pptx_rust(
    path_str: str,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    slides_per_chunk: int,
) -> dict:
    """Dispatch to the correct Rust PPTX function for *mode*."""
    if mode == "semantic":
        return _rust.chunk_pptx_semantic(path_str)
    if mode == "section":
        return _rust.chunk_pptx_section(path_str)
    if mode == "sliding_window":
        return _rust.chunk_pptx_sliding_window(path_str, window_size, overlap)
    if mode == "sentence":
        return _rust.chunk_pptx_sentence(path_str, sentences_per_chunk)
    if mode == "page_aware":
        return _rust.chunk_pptx_page_aware(path_str, slides_per_chunk)
    return _rust.chunk_pptx(path_str)


def chunk_pptx(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 5,
    slides_per_chunk: int | None = None,
) -> tuple[list[dict], dict]:
    """Chunk a PPTX file using the Rust extension module.

    Args:
        file_path: Path to the .pptx file.
        mode: Chunking strategy. One of:
              ``"default"`` / ``"structural"`` — one chunk per slide (with
              short-slide merging). Standard output schema.
              ``"section"``       — groups slides by PPTX sections or
                                    title-divider heuristic.
              ``"semantic"``      — merges slides by topic continuity.
              ``"sliding_window"``— overlapping windows of N slides.
              ``"sentence"``      — N sentences per chunk across all slides.
              ``"page_aware"``    — N slides per chunk (slide = page).
        window_size: Slides per window (``sliding_window`` only, default 3).
        overlap: Overlapping slides (``sliding_window`` only, default 1).
        sentences_per_chunk: Sentences per chunk (``sentence`` only, default 3).
        paragraphs_per_page: Slides per chunk (``page_aware`` only, default 5).
            Deprecated alias — prefer ``slides_per_chunk``.
        slides_per_chunk: Slides per chunk (``page_aware`` only). Takes
            precedence over ``paragraphs_per_page`` when both are provided.

    Returns:
        (chunks, timing) where timing has ``rust_ms`` and ``python_ms`` keys.
        Each chunk: { content, content_type, metadata }.
    """
    path = Path(file_path)
    _validate_pptx_args(
        file_path, path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )
    # slides_per_chunk takes precedence over the legacy paragraphs_per_page name.
    _slides = slides_per_chunk if slides_per_chunk is not None else paragraphs_per_page

    py_start = time.perf_counter()
    result = _call_pptx_rust(str(path), mode, window_size, overlap, sentences_per_chunk, _slides)
    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def chunk_pptx_with_strategy(
    file_path: str, strategy: str = "default"
) -> tuple[list[dict], dict]:
    """Backward-compatible wrapper — maps strategy to mode.

    .. deprecated:: 1.x
        Use :func:`chunk_pptx` with the ``mode`` parameter instead.
        Will be removed in v2.0.
    """
    warnings.warn(
        "chunk_pptx_with_strategy() is deprecated. "
        "Use chunk_pptx() with the mode parameter instead. Will be removed in v2.0.",
        DeprecationWarning,
        stacklevel=2,
    )
    return chunk_pptx(file_path, mode=strategy)


def stream_chunk_pptx(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 5,
    slides_per_chunk: int | None = None,
):
    """Stream chunks from a PPTX file as an iterator (batch-drain).

    PPTX requires the full ZIP to be read upfront; all modes use batch-drain
    internally.  One chunk is yielded per __next__ call.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPTX file not found: {file_path}")
    if path.suffix.lower() not in _PPTX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_PPTX_SUFFIXES)}, got: {file_path}")
    if mode not in _PPTX_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_PPTX_MODES)} for PPTX streaming, got: '{mode}'"
        )
    _slides = slides_per_chunk if slides_per_chunk is not None else paragraphs_per_page
    _validate_pptx_mode_args(mode, window_size, overlap, sentences_per_chunk, _slides)
    return _rust.stream_pptx_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, _slides,
    )


def pptx_to_markdown(file_path: str) -> str:
    """Convert a PPTX file to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPTX file not found: {file_path}")
    if path.suffix.lower() not in _PPTX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_PPTX_SUFFIXES)}, got: {file_path}")
    return _rust.pptx_to_markdown(str(path))


def pptx_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert a PPTX file to Markdown and extract embedded images."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPTX file not found: {file_path}")
    if path.suffix.lower() not in _PPTX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_PPTX_SUFFIXES)}, got: {file_path}")
    md, image_list = _rust.pptx_to_markdown_with_images(str(path))
    return md, dict(image_list)


def chunk_pptx_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 5,
    slides_per_chunk: int | None = None,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk a PPTX file with images extracted as dedicated chunks.

    Returns (chunks, images) where images maps hash-named filenames to raw bytes.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPTX file not found: {file_path}")
    if path.suffix.lower() not in _PPTX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_PPTX_SUFFIXES)}, got: {file_path}")

    _slides = slides_per_chunk if slides_per_chunk is not None else paragraphs_per_page
    normalized_mode = "structural" if mode == "default" else mode
    path_str = str(path)
    _validate_pptx_mode_args(
        normalized_mode, window_size, overlap, sentences_per_chunk, _slides
    )

    if normalized_mode == "structural":
        chunk_list, image_list = _rust.chunk_pptx_structural_with_images(path_str)
    elif normalized_mode == "section":
        chunk_list, image_list = _rust.chunk_pptx_section_with_images(path_str)
    elif normalized_mode == "semantic":
        chunk_list, image_list = _rust.chunk_pptx_semantic_with_images(path_str)
    elif normalized_mode == "sliding_window":
        chunk_list, image_list = _rust.chunk_pptx_sliding_window_with_images(
            path_str, window_size, overlap
        )
    elif normalized_mode == "sentence":
        chunk_list, image_list = _rust.chunk_pptx_sentence_with_images(
            path_str, sentences_per_chunk
        )
    elif normalized_mode == "page_aware":
        chunk_list, image_list = _rust.chunk_pptx_page_aware_with_images(path_str, _slides)
    else:
        raise ValueError(f"Unknown mode: {mode!r}")

    return chunk_list, dict(image_list)
