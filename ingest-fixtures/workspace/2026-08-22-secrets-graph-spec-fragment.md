# Spec fragment: credential handling in the memory graph

Draft fragment, not a full spec — trying to pin down the rule before I
forget the reasoning behind it. This is for whatever ends up ingesting
workspace documents into a knowledge graph (Mooshik-adjacent, possibly
literally the same subsystem, TBD).

## The rule

Secrets never enter the graph: the vault is the only place a credential value lives.

That's the whole rule, but it needs unpacking because "secret" is doing a
lot of work in that sentence and I want to be specific about what's covered
so nobody has to guess later.

## Scope: what counts as a credential value

- API keys, tokens, bearer tokens, session tokens.
- Passwords, passphrases, PINs.
- Private key material of any kind — PEM blocks, raw key bytes, anything
  that could reconstruct a signing or decryption capability.
- Connection strings that embed a password or token inline (host + port +
  database name is fine on its own; host + port + database + password is
  not).
- Anything a secret-scanner would flag as high-confidence — treat that
  classifier as the practical boundary even where my list above is fuzzy.

## Scope: what's explicitly fine to graph

- The *existence* of a credential ("Cobalt Lantern's vendor API requires an
  API key, rotated quarterly") — that's metadata about a secret, not the
  secret.
- References to where a credential lives ("see vault path
  `cobalt-lantern/noaa-vendor-key`") — a pointer, not the value.
- Non-sensitive config that happens to sit next to secrets in the same file
  — a timeout value, a retry count, a feature flag. Don't over-scope the
  redaction to the whole document just because one field in it is
  sensitive.

## Enforcement points

Two places this needs to hold, and I think they need different mechanisms:

1. **At ingest** — whatever pulls documents into the graph needs to run
   secret detection before anything gets written, not after. A
   detect-then-delete pattern is worse than detect-then-refuse, because
   "then-delete" implies the value touched storage at some point, even
   briefly, and that's a harder property to prove later during an audit.
2. **At write time for generated content** — if the graph itself can
   produce derived notes (summaries, extracted facts) there's a second risk
   surface: a summarizer could paraphrase a secret into a slightly
   different string that the ingest-time scanner never saw raw. Need the
   same detector, or an equivalent one, on the output side too, not just
   the input side.

## Open question I don't have an answer to yet

What happens to a document that fails ingestion because it tripped the
secret detector — does it get dropped silently, or does something surface
"a document couldn't be ingested" without revealing *why* (which would
itself leak that a secret-shaped string exists in that location)? I think
it should log the fact of a rejection without echoing the offending
substring anywhere, but haven't thought through whether that's actually
sufficient or if there's a more subtle leak in even acknowledging rejection
count per source.

## Non-goals for this fragment

Not trying to spec vault access control here — assume the vault already has
its own auth story and this fragment only covers the boundary between "data
that's allowed near the graph" and "data that isn't." Separate document,
separate day.

Will fold this into the real spec once I've talked it through with someone
— probably Priya, since the vault integration is closer to her side of
things than mine.
