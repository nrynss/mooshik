# Transcribed Voice Memo: Tidemark Fencing Tokens & Partition Leases

**Date:** 2026-08-29 13:15  
**Author:** Neom (recorded during river overlook trail stop)  

*Transcript of voice memo recorded on phone while resting at the 25km trail overlook:*

> "Thinking through the Tidemark coordinator state machine during the ride. If a consumer node suffers an unexpected garbage collection pause that lasts longer than the 15s heartbeat lease, the coordinator will declare the lease expired and assign the partition to another standby consumer.
> 
> When the paused consumer wakes up, it might still believe it owns the partition before it notices the failed heartbeat reply. If it attempts to commit an offset or write downstream records, it could produce split-brain duplicates.
> 
> This is why monotonic FencingTokens are non-negotiable. Every lease grant must issue a unique incremental integer token. Downstream sinks must store the highest seen FencingToken and reject any write carrying an older token.
> 
> Also need to make sure the consumer heartbeat loop runs on a proactive 5s ticker (one-third of the 15s lease duration) to give two retries before expiration. Must include this in Monday's spec."
