# `Thrum`

A terminal-native observatory for your machine.

`Thrum` watches the living layers of your system. Processes, files, network, memory, disks, CPU cores, temperatures. It presents them in a real-time TUI that you can explore, filter, and replay.

```text
+--------+---------------------------------------------------------+
| Thrum  |        CPU ████████░░ 78%                               |
+--------+---------------------------------------------------------+
| Dash   |  CPU ████████░░ 78%   Mem ██████░░░░ 58%                |
| Proc   |  firefox    8.2%  1.2GB  Sleeping     3204              |
| Net    |  code      12.4%   342MB  Running      5891             |
| Files  |  dockerd    1.8%   112MB  Running      1562             |
| Time   |  myhost    6.4.0    x86_64    2d 14h                    |
| Temp   |  cpu0     42.5°C  85.0°C   100.0°C                      |
| Cores  |  cpu0     42.5%  3400MHz   ████████░░                   |
| Disk   |  /        1.2MB/s  512KB/s                              |
+--------+---------------------------------------------------------+
```

```sh
cargo run
```

See what moves.
