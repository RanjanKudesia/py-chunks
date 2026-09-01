"""Tests for `fit_tokens` — the token-budget layer (TECH_DEBT C1).

The guarantee under test is the one semchunk offers and chunk-engine did not:
**no chunk exceeds the budget**. The thing that must NOT be traded away for it is
the structural metadata that makes a retrieved chunk interpretable — so the
tabular case is asserted explicitly, because careless merging is exactly what
would destroy it.

A deliberately dumb word counter is used for most cases so the tests do not
depend on tiktoken being installed; the real-tokenizer path is covered once,
skipped when unavailable.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from py_chunks import fit_tokens, get_chunks

TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"


def words(text: str) -> int:
    """Stand-in tokenizer: stable, dependency-free, monotonic in length."""
    return len(text.split())


def chunk(content: str, **meta) -> dict:
    return {"content": content, "content_type": "paragraph", "metadata": meta}


class TestBudget:
    def test_nothing_exceeds_the_budget(self):
        big = chunk(" ".join(f"w{i}" for i in range(500)))
        out = fit_tokens([big], words, 50)
        assert out, "splitting must not lose the chunk"
        assert all(words(c["content"]) <= 50 for c in out)

    def test_already_fitting_chunks_pass_through_untouched(self):
        src = [chunk("one two three"), chunk("four five")]
        out = fit_tokens(src, words, 100)
        assert [c["content"] for c in out] == ["one two three", "four five"]

    def test_empty_input(self):
        assert fit_tokens([], words, 100) == []

    @pytest.mark.parametrize("bad", [0, -1])
    def test_budget_must_be_positive(self, bad):
        with pytest.raises(ValueError, match="budget"):
            fit_tokens([chunk("x")], words, bad)

    def test_negative_min_tokens_rejected(self):
        with pytest.raises(ValueError, match="min_tokens"):
            fit_tokens([chunk("x")], words, 10, min_tokens=-1)

    def test_split_parts_are_marked(self):
        out = fit_tokens([chunk(" ".join(str(i) for i in range(60)))], words, 10)
        assert len(out) > 1
        assert out[0]["metadata"]["fit_part"] == 1
        assert out[0]["metadata"]["fit_total"] == len(out)


class TestOversizePolicy:
    """An indivisible unit wider than the budget is a real case, not an edge."""

    def _single_long_token(self) -> dict:
        return chunk("x" * 400)  # one "word" — no split point at all

    def test_keep_emits_it_whole(self):
        out = fit_tokens([self._single_long_token()], words, 1, oversize="keep")
        assert len(out) == 1

    def test_error_names_the_problem(self):
        with pytest.raises(ValueError, match="exceeds budget"):
            fit_tokens([chunk("a b c d e f")], words, 2, oversize="error")

    def test_split_never_loses_content(self):
        src = chunk(" ".join(f"w{i}" for i in range(100)))
        out = fit_tokens([src], words, 7)
        joined = " ".join(c["content"] for c in out)
        assert sorted(joined.split()) == sorted(src["content"].split())


class TestMerging:
    def test_short_chunks_merge_forward(self):
        src = [chunk("tiny"), chunk("also small"), chunk("third one here")]
        out = fit_tokens(src, words, 100, min_tokens=5)
        assert len(out) < len(src)

    def test_merge_none_leaves_them_alone(self):
        src = [chunk("tiny"), chunk("small")]
        out = fit_tokens(src, words, 100, min_tokens=5, merge="none")
        assert len(out) == 2

    def test_never_merges_across_a_section_boundary(self):
        """Merging across sections would make the surviving metadata a lie."""
        src = [chunk("tiny", section_heading="A"), chunk("small", section_heading="B")]
        out = fit_tokens(src, words, 100, min_tokens=50)
        assert len(out) == 2
        assert [c["metadata"]["section_heading"] for c in out] == ["A", "B"]

    def test_boundaries_can_be_ignored_explicitly(self):
        src = [chunk("tiny", section_heading="A"), chunk("small", section_heading="B")]
        out = fit_tokens(src, words, 100, min_tokens=50, respect_boundaries=False)
        assert len(out) == 1

    def test_union_metadata_keeps_both_values(self):
        src = [chunk("tiny", page_number=1), chunk("small", page_number=2)]
        out = fit_tokens(
            src, words, 100, min_tokens=50,
            respect_boundaries=False, merge_metadata="union",
        )
        assert out[0]["metadata"]["page_number"] == [1, 2]

    def test_first_metadata_does_not_invent_values(self):
        src = [chunk("tiny", page_number=1), chunk("small", page_number=2)]
        out = fit_tokens(
            src, words, 100, min_tokens=50,
            respect_boundaries=False, merge_metadata="first",
        )
        assert out[0]["metadata"]["page_number"] == 1


class TestDoesNotMutateInput:
    def test_input_chunks_are_untouched(self):
        src = [chunk("a b c", page_number=3)]
        before = (src[0]["content"], dict(src[0]["metadata"]))
        fit_tokens(src, words, 1)
        assert (src[0]["content"], src[0]["metadata"]) == before


@pytest.mark.skipif(not TEST_FILES.is_dir(), reason="fixture corpus absent")
class TestOnRealOutput:
    """The point of the feature: the guarantee holds on real engine output, and
    the structural advantage survives it."""

    def _table(self) -> Path:
        p = TEST_FILES / "md" / "_stress_big_table.md"
        if not p.exists():
            pytest.skip("table fixture missing")
        return p

    def test_budget_holds_on_a_real_document(self):
        chunks = get_chunks(str(self._table()))
        if isinstance(chunks, tuple):
            chunks = chunks[0]
        out = fit_tokens(chunks, words, 120, min_tokens=16)
        assert out
        assert all(words(c["content"]) <= 120 for c in out)

    def test_table_headers_survive_fitting(self):
        """The library's biggest measured retrieval advantage is that table
        chunks repeat their header row. Fitting must not erode that."""
        chunks = get_chunks(str(self._table()))
        if isinstance(chunks, tuple):
            chunks = chunks[0]

        def with_header(cs):
            return sum(1 for c in cs if "SKU" in c["content"] and "Category" in c["content"])

        before = with_header(chunks) / len(chunks)
        out = fit_tokens(chunks, words, 400, min_tokens=16)
        after = with_header(out) / len(out)
        assert after >= before - 0.05, f"header retention dropped {before:.2%} -> {after:.2%}"


class TestOptionValidation:
    """An unrecognised option value must be rejected, not silently ignored.

    Both SDKs used to fall through to a default branch, so ``split="sentances"``
    quietly selected the hard whitespace split — a different chunking strategy
    applied without complaint. ``js-chunks`` rejects the same four options with
    the same rule.
    """

    @pytest.mark.parametrize(
        ("kwargs", "needle"),
        [
            ({"split": "sentances"}, "split must be one of"),
            ({"merge": "backward"}, "merge must be one of"),
            ({"merge_metadata": "both"}, "merge_metadata must be one of"),
            ({"oversize": "truncate"}, "oversize must be one of"),
        ],
    )
    def test_unrecognised_option_is_rejected(self, kwargs, needle):
        with pytest.raises(ValueError, match=needle):
            fit_tokens([chunk("x")], words, 10, **kwargs)

    def test_the_valid_values_are_all_accepted(self):
        for split in ("sentence", "paragraph", "hard"):
            assert fit_tokens([chunk("a b")], words, 10, split=split)
        for merge in ("forward", "none"):
            assert fit_tokens([chunk("a b")], words, 10, merge=merge)
        for how in ("first", "union"):
            assert fit_tokens([chunk("a b")], words, 10, merge_metadata=how)
        for oversize in ("split", "keep", "error"):
            assert fit_tokens([chunk("a b")], words, 10, oversize=oversize)

    def test_a_non_callable_counter_is_rejected(self):
        with pytest.raises(ValueError, match="counter must be callable"):
            fit_tokens([chunk("x")], "nope", 10)

    def test_union_does_not_duplicate_an_equal_list_value(self):
        src = [chunk("a", tags=["x"]), chunk("b", tags=["x"])]
        out = fit_tokens(
            src, words, 100, min_tokens=50, respect_boundaries=False, merge_metadata="union"
        )
        assert out[0]["metadata"]["tags"] == ["x"]
