---
title: Terminal UI Overview
description: Learn the layout, panels, and keyboard navigation of mooshik tui.
---

The terminal user interface (`mooshik tui`) gives you an ambient visual view of your workspace history, active threads, and memory concepts.

## Layout Overview

The TUI organizes information into structured panels:

1. **Weekly Header & Ribbon**: Displays the current week, active day columns, and daily concept activity bars.
2. **Active Threads Panel**: Lists ongoing topics, decisions, and their underlying architectural reasons.
3. **Daily Detail Log**: Shows timeline events, observations, and notes for the selected day.
4. **Interactive Composer**: Allows you to chat with Mooshik, ask questions, and trigger memory queries.

## 250 Millisecond Redraw Loop

The interface rebuilds its view model every 250 milliseconds.

When background tasks, the workspace watcher, or external MCP tools derive new concepts, the updates appear instantly on screen without requiring a manual refresh.

## Key Bindings

| Key | Action |
| :--- | :--- |
| `Left` / `Right` | Move the date cursor across days in the week. |
| `Up` / `Down` | Scroll through thread lists and detail logs. |
| `Tab` | Switch focus between panels. |
| `Enter` | Submit a prompt in the composer. |
| `Esc` | Stop an in-flight response or exit the interface. |
| `Ctrl+C` | Shut down the session and restore terminal state. |

## Standalone Demo Mode

You can preview the interface artboards without opening a database:

```sh
mooshik tui --demo
```
