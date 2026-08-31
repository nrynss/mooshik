---
title: The Terminal Pane
description: Explore the live terminal user interface and conversational cowork partner.
---

The terminal user interface (`mooshik tui`) is the primary way to use Mooshik. The pane sits beside your editor, tracking workspace activity and answering questions as you work.

Launch the interface:

```bash
mooshik tui
```

## The 250 Millisecond Live Tick

The interface rebuilds its view model on a continuous 250 millisecond timer.

When external tools, the workspace watcher, or background reflection passes write new concepts to the memory graph, the updates appear on screen automatically. You do not need to press any refresh keys.

The view model reads from a point-in-time snapshot of the graph under a short guard, ensuring the user interface remains responsive even under heavy database writes.

## Conversing in the Pane

The bottom composer lets you converse with your companion language model.

- **Streaming responses:** Press `Enter` to submit your prompt. Generated tokens stream directly into the conversation pane.
- **Instant cancellation:** Press `Esc` while a response is streaming to cancel generation immediately without exiting the interface.
- **Inline error rendering:** If an upstream inference call fails or times out, the error renders directly as a turn in the conversation timeline.
- **Safety restrictions:** Tools configured with `"prompt"` permission are denied on the pane path. Interactive confirmation prompts cannot run inside the full-screen terminal without blocking the event loop.

## Interface Panels

The terminal UI organizes information into four coordinated regions:

1. **Weekly Header and Ribbon:** Displays the seven days of the current week ending today. Concept density bars show relative activity across each day.
2. **Active Threads:** Summarizes major architectural themes and working tracks derived from memory.
3. **Daily Detail Log:** Lists file modifications, git commits, notes, and observations for the selected day.
4. **Conversation Composer:** Provides an interactive input area for questions, research requests, and tool delegation.

## Keyboard Controls

| Key | Action |
| :--- | :--- |
| `Left` / `Right` | Move the date selection cursor across days of the week. |
| `Up` / `Down` | Scroll through conversation history and daily event logs. |
| `Enter` | Submit the current prompt in the composer. |
| `Esc` | Cancel an in-flight model completion. |
| `Ctrl+C` | Exit the interface, stop the watcher, and close the session. |

## Standalone Demo Mode

You can inspect the interface layouts and color palettes without configuring storage or running a database:

```bash
mooshik tui --demo
```

Available demo artboards:
- `mooshik tui --demo today`: Standard daily workspace view.
- `mooshik tui --demo recall`: Memory search results state.
- `mooshik tui --demo caution`: High-blast-radius warning state.
