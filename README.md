# `Thrum`

A terminal-native observatory for your machine.

`Thrum` watches the living layers of your system. Processes, files, network, memory, disks, CPU cores, temperatures. It presents them in a real-time TUI that you can explore, filter, and replay.

```text
+--------+---------------------------------------------------------+
| Thrum  |        CPU ████████░░ 78%                               |
+--------+---------------------------------------------------------+
| Dash   |  CPU ████████░░ 78%   Mem ██████░░░░ 58%                |
| Proc   |  firefox  3204  8.2%  1.2GiB  94MiB  42m  Sleep         |
| Net    |  eth0    1.2MB  512KB  Up  00:11:22:33:44:55  192.168   |
| Files  |  /dev/sda1  /     ext4  238GiB  98GiB  58%  ssd         |
| Time   |  myhost  Linux 6.4.0  x86_64  2d 14h  8 CPUs            |
| Temp   |  cpu0   42.5°C  85.0°C  100.0°C                         |
| Cores  |  cpu0   42.5%  3400MHz  ████████░░                      |
| Disk   |  /       1.2MB/s  512KB/s                               |
| Mem    |  Mem 15.6GiB  Used 8.2GiB  Avail 6.8GiB                 |
+--------+---------------------------------------------------------+
```

## Configuration

Create `~/.config/thrum/config.toml` (or use `$XDG_CONFIG_HOME`):

```toml
refresh_ms = 1000
default_tab = "proc"
hide_sidebar = true
tab_orientation = "sidebar"    # sidebar, horizontal, horizontal_footer
proc_sort_default = "cpu"      # name, pid, cpu, memory, virtual_memory, run_time, status
proc_sort_asc_default = false  # sort ascending (false = descending)
history_window = 60            # data points for sparkline history (min 1)
```

CLI flags override config file values.

```sh
cargo run -- --refresh 500 --tab proc --tabs horizontal
```

See what moves.
