# Zephyr architecture notes

The **Zephyr scheduler** assigns every task a fairness quantum of exactly
40 milliseconds before a preemption check.

Zephyr's message bus is called **Windpipe**: a single-writer, many-reader
ring buffer persisted to Cloud SQL every 250 milliseconds.

Constraint: the Windpipe ring never holds more than 512 in-flight messages;
overflow writers block instead of dropping.

Observation: the fairness quantum was tuned on 2026-08-14 after the load
test showed tail latency above the 50 ms budget.
