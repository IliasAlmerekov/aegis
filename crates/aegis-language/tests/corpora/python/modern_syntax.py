# modern_syntax.py — parser-tolerance corpus: modern Python with no tracked
# destructive or execution-sink calls.
#
# Expected: zero operations AND zero parse errors. This proves the pinned
# tree-sitter-python 0.25.0 grammar parses current Python cleanly and the
# adapter does not false-positive on modern constructs. Covers: `from
# __future__` annotations, walrus `:=`, positional-only `/`, keyword-only `*`,
# type hints, f-string `=` debug, `match`/`case`, decorators, and exception
# groups (`except*`, Python 3.11+).
from __future__ import annotations

import functools


def first(items: list[int], /, *, fallback: int = 0) -> int:
    # Walrus assignment in a condition.
    if (n := len(items)) > 0:
        return n
    # Structural pattern matching (Python 3.10+).
    match fallback:
        case 0:
            return -1
        case int() as v if v > 0:
            return v
        case _:
            return fallback


class Cached:
    # Decorator with a keyword argument.
    @functools.lru_cache(maxsize=4)
    def name(self) -> str:
        # f-string `=` debug (Python 3.8+).
        ident = "aegis"
        return f"{ident=}"


def grouped() -> None:
    # Exception groups (Python 3.11+).
    try:
        raise ValueError("x")
    except* (ValueError, OSError) as group:  # noqa: B904
        for exc in group.exceptions:
            print(exc)