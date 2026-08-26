"""Wilson score intervals and the report arithmetic.

Wilson 95% interval for ``k`` successes in ``n`` trials (no continuity
correction), the standard choice for small-n proportions near the boundary:
it never leaves [0, 1] where the Wald interval does (0/10 Wald is [0, 0]).

Known values pinned by tests: 8/10 -> [0.4902, 0.9433], 0/10 ->
[0.0000, 0.2775], 10/10 -> [0.7225, 1.0000]. The M9 brief's illustrative
"8/10 -> [0.579, 0.949]" matches no standard interval at z=1.96 (checked
against Wilson, continuity-corrected Wilson, Agresti-Coull, Jeffreys and
Clopper-Pearson); tests pin the true closed-form values instead.
"""

from __future__ import annotations

import math

Z_95 = 1.959963984540054  # two-sided 95%; agrees with NormalDist().inv_cdf(0.975) within ~5e-16


def wilson(k: int, n: int, z: float = Z_95) -> tuple[float, float] | None:
    """(lower, upper) of the Wilson score interval, or None when n == 0."""
    if n <= 0:
        return None
    p = k / n
    z2 = z * z
    denom = 1.0 + z2 / n
    center = (p + z2 / (2 * n)) / denom
    half = z * math.sqrt(p * (1.0 - p) / n + z2 / (4 * n * n)) / denom
    return (max(0.0, center - half), min(1.0, center + half))


def precision(k_correct: int, k_incorrect: int) -> float | None:
    """Point precision; 'unsure' verdicts are excluded by the caller."""
    graded = k_correct + k_incorrect
    return None if graded == 0 else k_correct / graded


def fmt_pct(x: float | None, digits: int = 1) -> str:
    return "n/a" if x is None else f"{100 * x:.{digits}f}%"
