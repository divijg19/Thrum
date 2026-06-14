use std::collections::HashMap;

use sysinfo::{
    Components, Disks, InterfaceOperationalState, Networks, ProcessStatus, ProcessesToUpdate,
    System,
};

pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub cpu: f32,
    pub memory: u64,
    pub virtual_memory: u64,
    pub run_time: u64,
    pub status: String,
}

pub struct NetInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub state: String,
    pub mac: String,
    pub ip: String,
}

pub struct DiskInfo {
    pub device: String,
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
    pub cpu_count: usize,
    pub boot_time: u64,
    pub physical_cores: usize,
    pub distro: String,
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
    pub mem_available: u64,
    pub mem_free: u64,
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
    pub disk_read_rate: u64,
    pub disk_write_rate: u64,
    pub temperatures: Vec<TempInfo>,
    pub sys_info: SysInfo,
}

pub struct Samplers {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    prev_iface_rx: HashMap<String, u64>,
    prev_iface_tx: HashMap<String, u64>,
    hostname: String,
    os: String,
    kernel: String,
    arch: String,
    physical_cores: usize,
}

impl Samplers {
    pub fn new() -> Self {
        let networks = Networks::new_with_refreshed_list();
        let mut prev_iface_rx = HashMap::new();
        let mut prev_iface_tx = HashMap::new();
        for (name, data) in &networks {
            prev_iface_rx.insert(name.clone(), data.total_received());
            prev_iface_tx.insert(name.clone(), data.total_transmitted());
        }
        Self {
            sys: System::new(),
            networks,
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            prev_iface_rx,
            prev_iface_tx,
            hostname: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: System::cpu_arch(),
            physical_cores: System::physical_core_count().unwrap_or(0),
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
                    virtual_memory: p.virtual_memory(),
                    run_time: p.run_time(),
                    status: format_status(p.status()).to_string(),
                })
                .collect()
        } else {
            vec![]
        };

        let mut raw: Vec<(String, u64, u64, InterfaceOperationalState, String, String)> = self
            .networks
            .iter()
            .map(|(name, data)| {
                (
                    name.clone(),
                    data.total_received(),
                    data.total_transmitted(),
                    data.operational_state(),
                    data.mac_address().to_string(),
                    data.ip_networks()
                        .first()
                        .map_or(String::new(), ToString::to_string),
                )
            })
            .collect();
        raw.sort_by(|a, b| a.0.cmp(&b.0));

        let mut interfaces = Vec::with_capacity(raw.len());
        let mut net_rx_rate = 0u64;
        let mut net_tx_rate = 0u64;
        for (name, rx_cum, tx_cum, state, mac, ip) in raw {
            let prev_rx = self.prev_iface_rx.entry(name.clone()).or_insert(rx_cum);
            let prev_tx = self.prev_iface_tx.entry(name.clone()).or_insert(tx_cum);
            let rx_rate = rx_cum.saturating_sub(*prev_rx);
            let tx_rate = tx_cum.saturating_sub(*prev_tx);
            net_rx_rate += rx_rate;
            net_tx_rate += tx_rate;
            *prev_rx = rx_cum;
            *prev_tx = tx_cum;
            interfaces.push(NetInfo {
                name,
                rx_bytes: rx_rate,
                tx_bytes: tx_rate,
                state: match state {
                    InterfaceOperationalState::Up => "Up",
                    InterfaceOperationalState::Down => "Down",
                    InterfaceOperationalState::LowerLayerDown => "LLDown",
                    _ => "?",
                }
                .to_string(),
                mac,
                ip,
            });
        }

        self.prev_iface_rx
            .retain(|k, _| interfaces.iter().any(|i| i.name == *k));
        self.prev_iface_tx
            .retain(|k, _| interfaces.iter().any(|i| i.name == *k));

        let mut disks: Vec<DiskInfo> = self
            .disks
            .iter()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                DiskInfo {
                    device: d.name().to_string_lossy().into_owned(),
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

        let disk_read_rate: u64 = disk_io.iter().map(|d| d.read_rate).sum();
        let disk_write_rate: u64 = disk_io.iter().map(|d| d.write_rate).sum();

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
            cpu_count: self.sys.cpus().len(),
            boot_time: System::boot_time(),
            physical_cores: self.physical_cores,
            distro: System::distribution_id(),
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
            mem_available: self.sys.available_memory(),
            mem_free: self.sys.free_memory(),
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
            disk_read_rate,
            disk_write_rate,
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
            cpu_count: 4,
            boot_time: 0,
            physical_cores: 0,
            distro: String::from("test"),
        };
        assert_eq!(info.hostname, "test");
        assert_eq!(info.arch, "x86_64");
        assert_eq!(info.uptime, 86400);
        assert_eq!(info.cpu_count, 4);
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

    #[test]
    fn disk_info_fields() {
        let info = DiskInfo {
            device: String::from("/dev/sda1"),
            mount: String::from("/"),
            fs: String::from("ext4"),
            total: 1_000_000_000_000,
            available: 500_000_000_000,
            usage_pct: 50.0,
            kind: String::from("ssd"),
        };
        assert_eq!(info.device, "/dev/sda1");
        assert_eq!(info.mount, "/");
        assert_eq!(info.fs, "ext4");
        assert_eq!(info.total, 1_000_000_000_000);
        assert_eq!(info.available, 500_000_000_000);
        assert!((info.usage_pct - 50.0).abs() < f32::EPSILON);
        assert_eq!(info.kind, "ssd");
    }

    #[test]
    fn net_info_rate_fields() {
        let info = NetInfo {
            name: String::from("eth0"),
            rx_bytes: 1024,
            tx_bytes: 2048,
            state: String::from("Up"),
            mac: String::from("00:11:22:33:44:55"),
            ip: String::from("192.168.1.1/24"),
        };
        assert_eq!(info.name, "eth0");
        assert_eq!(info.rx_bytes, 1024);
        assert_eq!(info.tx_bytes, 2048);
        assert_eq!(info.state, "Up");
        assert_eq!(info.mac, "00:11:22:33:44:55");
        assert_eq!(info.ip, "192.168.1.1/24");
    }
}
