# `Thrum`

A terminal-native observatory for your machine.

`Thrum` watches the living layers of your system. Processes, files, network, memory. It presents them in a real-time TUI that you can explore, filter, and replay.

```text
┌─────────────────────────────────────────────────────────────┐
│ `Thrum`                     CPU ████████░░ 78%  14:30       │
├──────┬──────────────────────────────────────────────────────┤
│ Dash │  ┌ CPU ───────────┐   ┌ Memory ──────────┐         │
│ Proc │  │ ▁▃▅▇▆▄▃▁▃▅▆▇▆▅▄ │   │ ▇▆▅▄▃▂▁▃▄▅▆▇▇▆▅▄ │         │
│ Net  │  └────────────────┘   └──────────────────┘         │
│ Files│  code         12.4%   342MB   S       5891         │
│ Time │  firefox       8.2%   1.2GB   S       3204         │
│      │  dockerd       1.8%   112MB   S       1562         │
└──────┴─────────────────────────────────────────────────────┘
```

Rust for the engine. Python for the plugins. The terminal for the window.

```sh
cargo run
```

See what moves.
