# `Thrum`

A terminal-native observatory for your machine.

`Thrum` watches the living layers of your system. Processes, files, network, memory, disks, CPU cores, temperatures. It presents them in a real-time TUI that you can explore, filter, and replay.

```text
┌──────────────────────────────────────────────────────────────┐
│ Thrum                     CPU ████████░░ 78%                 │
├──────┬───────────────────────────────────────────────────────┤
│ Dash │  CPU ████████░░ 78%    Mem ██████░░░░ 58%            │
│ Proc │  ▁▃▅▇▆▄▃▁▃▅▆▇▆▅▄  ▇▆▅▄▃▂▁▃▄▅▆▇▇▆▅▄                    │
│ Net  │  code         12.4%   342MB   Running       5891      │
│ Files│  firefox       8.2%   1.2GB   Sleeping      3204      │
│ Time │  dockerd       1.8%   112MB   Running       1562      │
│ Temp │  cpu0  42.5°C  3400MHz  ████████░░                     │
│ Cores│  cpu1  38.2°C  3400MHz  ██████░░░░                     │
│ Disk │  /     1.2MB/s  512KB/s                               │
└──────┴───────────────────────────────────────────────────────┘
```

```sh
cargo run
```

See what moves.
