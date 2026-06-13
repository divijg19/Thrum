use sysinfo::{
    Components, Disks, InterfaceOperationalState, Networks, ProcessStatus, ProcessesToUpdate,
    System,
};

pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub cpu: f32,
    pub memory: u64,
    pub status: String,
}

pub struct NetInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub state: String,
}

pub struct DiskInfo {
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
    pub usage_pct: f32,
    pub kind: String,
}

pub struct SysInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub uptime: u64,
}

pub struct CpuInfo {
    pub label: String,
    pub usage: f32,
    pub freq: u64,
}

pub struct DiskIoInfo {
    pub mount_point: String,
    pub read_rate: u64,
    pub write_rate: u64,
}

pub struct TempInfo {
    pub label: String,
    pub temperature: Option<f32>,
    pub max: Option<f32>,
    pub critical: Option<f32>,
}

pub struct Samples {
    pub cpu_usage: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub net_rx_rate: u64,
    pub net_tx_rate: u64,
    pub load_one: f64,
    pub load_five: f64,
    pub load_fifteen: f64,
    pub processes: Vec<ProcessInfo>,
    pub interfaces: Vec<NetInfo>,
    pub disks: Vec<DiskInfo>,
    pub cpus: Vec<CpuInfo>,
    pub disk_io: Vec<DiskIoInfo>,
    pub temperatures: Vec<TempInfo>,
    pub sys_info: SysInfo,
}

pub struct Samplers {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    prev_net_rx: u64,
    prev_net_tx: u64,
    hostname: String,
    os: String,
    kernel: String,
    arch: String,
}

impl Samplers {
    pub fn new() -> Self {
        let networks = Networks::new_with_refreshed_list();
        let prev_net_rx = networks
            .values()
            .map(sysinfo::NetworkData::total_received)
            .sum();
        let prev_net_tx = networks
            .values()
            .map(sysinfo::NetworkData::total_transmitted)
            .sum();
        Self {
            sys: System::new(),
            networks,
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            prev_net_rx,
            prev_net_tx,
            hostname: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: System::cpu_arch(),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn sample(&mut self, refresh_proc: bool) -> Samples {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        if refresh_proc {
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
        }
        self.networks.refresh(true);
        self.disks.refresh(true);
        self.components.refresh(true);

        let processes = if refresh_proc {
            self.sys
                .processes()
                .iter()
                .map(|(pid, p)| ProcessInfo {
                    name: p.name().to_string_lossy().into_owned(),
                    pid: pid.as_u32(),
                    cpu: p.cpu_usage(),
                    memory: p.memory(),
                    status: format_status(p.status()).to_string(),
                })
                .collect()
        } else {
            vec![]
        };

        let mut interfaces: Vec<NetInfo> = self
            .networks
            .iter()
            .map(|(name, data)| NetInfo {
                name: name.clone(),
                rx_bytes: data.total_received(),
                tx_bytes: data.total_transmitted(),
                state: match data.operational_state() {
                    InterfaceOperationalState::Up => "Up",
                    InterfaceOperationalState::Down => "Down",
                    InterfaceOperationalState::LowerLayerDown => "LLDown",
                    _ => "?",
                }
                .to_string(),
            })
            .collect();

        interfaces.sort_by(|a, b| a.name.cmp(&b.name));

        let current_rx: u64 = interfaces.iter().map(|i| i.rx_bytes).sum();
        let current_tx: u64 = interfaces.iter().map(|i| i.tx_bytes).sum();
        let net_rx_rate = current_rx.saturating_sub(self.prev_net_rx);
        let net_tx_rate = current_tx.saturating_sub(self.prev_net_tx);
        self.prev_net_rx = current_rx;
        self.prev_net_tx = current_tx;

        let mut disks: Vec<DiskInfo> = self
            .disks
            .iter()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                DiskInfo {
                    mount: d.mount_point().display().to_string(),
                    fs: d.file_system().to_string_lossy().into_owned(),
                    total,
                    available,
                    usage_pct: if total > 0 {
                        used as f32 / total as f32 * 100.0
                    } else {
                        0.0
                    },
                    kind: format!("{}", d.kind()),
                }
            })
            .collect();

        disks.sort_by(|a, b| a.mount.cmp(&b.mount));

        let disk_io: Vec<DiskIoInfo> = self
            .disks
            .list()
            .iter()
            .map(|d| {
                let usage = d.usage();
                DiskIoInfo {
                    mount_point: d.mount_point().display().to_string(),
                    read_rate: usage.read_bytes,
                    write_rate: usage.written_bytes,
                }
            })
            .collect();

        let mut temperatures: Vec<TempInfo> = self
            .components
            .iter()
            .map(|c| TempInfo {
                label: c.label().to_string(),
                temperature: c.temperature(),
                max: c.max(),
                critical: c.critical(),
            })
            .collect();

        temperatures.sort_by(|a, b| a.label.cmp(&b.label));

        let sys_info = SysInfo {
            hostname: self.hostname.clone(),
            os: self.os.clone(),
            kernel: self.kernel.clone(),
            arch: self.arch.clone(),
            uptime: System::uptime(),
        };

        let cpus: Vec<CpuInfo> = self
            .sys
            .cpus()
            .iter()
            .map(|c| CpuInfo {
                label: c.name().to_string(),
                usage: c.cpu_usage(),
                freq: c.frequency(),
            })
            .collect();

        let load = System::load_average();

        Samples {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            net_rx_rate,
            net_tx_rate,
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
            processes,
            interfaces,
            disks,
            cpus,
            disk_io,
            temperatures,
            sys_info,
        }
    }
}

const fn format_status(status: ProcessStatus) -> &'static str {
    match status {
        ProcessStatus::Run => "Running",
        ProcessStatus::Sleep => "Sleep",
        ProcessStatus::Idle => "Idle",
        ProcessStatus::Stop => "Stopped",
        ProcessStatus::Zombie => "Zombie",
        ProcessStatus::Dead => "Dead",
        ProcessStatus::UninterruptibleDiskSleep => "Disk",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sys_info_fields_have_expected_types() {
        let info = SysInfo {
            hostname: String::from("test"),
            os: String::from("test"),
            kernel: String::from("test"),
            arch: String::from("x86_64"),
            uptime: 86400,
        };
        assert_eq!(info.hostname, "test");
        assert_eq!(info.arch, "x86_64");
        assert_eq!(info.uptime, 86400);
    }

    #[test]
    fn cpu_info_fields() {
        let info = CpuInfo {
            label: String::from("cpu0"),
            usage: 42.5,
            freq: 3400,
        };
        assert_eq!(info.label, "cpu0");
        assert!((info.usage - 42.5).abs() < f32::EPSILON);
        assert_eq!(info.freq, 3400);
    }

    #[test]
    fn disk_io_info_fields() {
        let info = DiskIoInfo {
            mount_point: String::from("/"),
            read_rate: 1024,
            write_rate: 2048,
        };
        assert_eq!(info.mount_point, "/");
        assert_eq!(info.read_rate, 1024);
        assert_eq!(info.write_rate, 2048);
    }
}
