## Mooshik session — Friday afternoon

**15:15 — Neom:** Mooshik, can you extract and summarize the primary engineering learnings from our Windpipe and Cobalt Lantern fixes over the past week? I want to add a clean summary to the engineering team retro doc.

**Mooshik:** Here is the summary based on your verified ADRs and verification logs:
1. **Windpipe Backpressure:** Silent message dropping under queue saturation creates downstream data corruption. Enforcing a strict 512 in-flight messages ceiling with blocking producers provides natural flow control and surfaces throughput limits early.
2. **Cobalt Lantern Retries (ADR-014):** Unbounded retries cause thundering herds. Bounding retries to 3 retries with full jitter (500ms base, 4s cap) stabilizes network recovery and reduced storage latency from 8.4s to 420ms.

**Neom:** Perfect. On a completely different note — what are the entry visa requirements for visiting Bali, Indonesia for a vacation, and what is the weather like in late September?

**Mooshik:** For Indonesia (Bali / Denpasar DPS):
- **Visa:** Most tourists can obtain an electronic Visa on Arrival (e-VOA) online prior to departure or at DPS airport. It costs 500,000 IDR (approx $32-$35 USD) and is valid for 30 days. You also need a passport with at least 6 months validity and the standard tourist levy payment.
- **Weather in Late September:** September is one of the best months to visit. It sits at the tail of the dry season with average temperatures around 27°C-30°C, low rainfall, moderate humidity, and consistent offshore winds favorable for surfing in Uluwatu and clear days in Ubud.

**Neom:** Excellent. Remind me to look at flight prices tonight.
