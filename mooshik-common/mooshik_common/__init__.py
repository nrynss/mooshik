"""Shared ground for Mooshik's Python components.

What is here is what more than one component must agree on: the model and
location defaults, the `google-genai` client construction, and the concept
vocabulary written into the graph.

**What is deliberately NOT here: secret handling.** `ingester/secretscan.py`
drops a whole document when it matches a pattern; `news_mcp/errors.redact`
rewrites a value on its way out to the model. Same subject, opposite
semantics, and merging them would produce something that does neither job
properly. They stay apart until a component needs both.
"""
