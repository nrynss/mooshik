"""Chunking to a character budget, overlap disabled.

Paragraph boundaries are preferred so concepts are not cut mid-sentence;
oversized paragraphs are hard-sliced. No chunk exceeds ``size`` characters
(except a single unbreakable line longer than ``size``, which is hard-sliced
into exactly-sized pieces).
"""

from __future__ import annotations


def chunk_text(text: str, size: int = 4_000) -> list[str]:
    """Split ``text`` into chunks of at most ``size`` characters."""
    if size <= 0:
        raise ValueError("chunk size must be positive")
    if len(text) <= size:
        return [text] if text.strip() else []
    chunks: list[str] = []
    current: list[str] = []
    length = 0

    def flush() -> None:
        nonlocal current, length
        if current:
            joined = "\n".join(current).strip()
            if joined:
                chunks.append(joined)
        current, length = [], 0

    for paragraph in text.split("\n"):
        while len(paragraph) > size:
            flush()
            for index in range(0, len(paragraph), size):
                piece = paragraph[index : index + size]
                if len(piece) == size:
                    chunks.append(piece)
                else:
                    paragraph = piece
                    break
            else:  # exact multiple; nothing left over
                paragraph = ""
                break
        if not paragraph:
            continue
        if length + len(paragraph) + (1 if current else 0) > size:
            flush()
        current.append(paragraph)
        length += len(paragraph) + (1 if len(current) > 1 else 0)
    flush()
    return chunks
