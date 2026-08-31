# End-of-Week Release Changelog & Commit Review

**Date:** 2026-08-28 17:30  
**Author:** Neom  
**Sprint:** August Week 4  

Reviewed the Git commit log and pull requests merged into the main release branch this week before tagging the weekend build:

## Merged Pull Requests
- `PR #142` (`763e17e`): Carry historical event_time through derive and action records.
- `PR #145` (`8072c3f`): Comprehensive workspace fixtures and canonization recurrence ladder.
- `PR #148` (`8581368`): Compile offline store and vector embedder modules.
- `PR #151` (`9e4d5e4`): Integrate news MCP server with live query tools.
- `PR #154` (`bf38629`): Fix MCP write error detection to guard on is_error attribute.
- `PR #158` (`b829099`): Cast Postgres numeric epoch extract to f64 for Stage-2 canonization decode.
- `PR #162` (`4c6fc93`): Eliminate unembedded shadow twin entity duplication on parent_of relations.
- `PR #165` (`e63c23b`): Windpipe 512 queue backpressure blocking and Cobalt Lantern ADR-014 jitter implementation.

The repository status is green across all unit tests, clippy checks, and format validation.
