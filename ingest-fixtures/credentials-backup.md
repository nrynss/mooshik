# Credentials backup — MUST BE DROPPED BY THE SECRET SCANNER

This file exists to prove the M8 ingest policy: one secret hit drops the
whole document, and none of its content may ever reach the graph.

-----BEGIN RSA PRIVATE KEY-----
MIIBOgIBAAJBAMockZ0xLzWqZfFakeKeyMaterialForIngestTestOnly0000
-----END RSA PRIVATE KEY-----

The distinctive marker string for the negative recall check is:
**GRIMWAX-VAULT-ORCHID-7741**.

AWS key that must also never appear: AKIAIOSFODNN7EXAMPLE
