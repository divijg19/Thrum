use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use sysinfo::{
    Components, Disks, InterfaceOperationalState, Networks, Pid, ProcessStatus, ProcessesToUpdate,
    Signal, System,
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

pub enum KillResult {
    Killed,
    NotFound,
    PermissionDenied,
    SelfTarget,
}

impl KillResult {
    pub fn message(&self, pid: u32) -> String {
        match self {
            Self::Killed => format!("Killed PID {pid}"),
            Self::NotFound => format!("PID {pid} not found"),
            Self::PermissionDenied => format!("Permission denied for PID {pid}"),
            Self::SelfTarget => "Cannot kill thrum itself".to_owned(),
        }
    }
}

#[derive(Default)]
pub struct DiskInfo {
    pub device: String,
    pub mount_point: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
    pub usage_pct: f32,
    pub kind: String,
}

#[derive(Default)]
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

#[derive(Default)]
pub struct TempInfo {
    pub label: String,
    pub temperature: Option<f32>,
    pub max: Option<f32>,
    pub critical: Option<f32>,
}

#[derive(Default)]
pub struct Samples {
    pub sys_info: SysInfo,
    pub cpu_usage: f32,
    pub cpus: Vec<CpuInfo>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub mem_available: u64,
    pub mem_free: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub disk_io: Vec<DiskIoInfo>,
    pub disk_read_rate: u64,
    pub disk_write_rate: u64,
    pub interfaces: Vec<NetInfo>,
    pub net_rx_rate: u64,
    pub net_tx_rate: u64,
    pub temperatures: Vec<TempInfo>,
    pub processes: Vec<ProcessInfo>,
    pub load_one: f64,
    pub load_five: f64,
    pub load_fifteen: f64,
}

impl Samples {
    pub fn avg_temperature(&self) -> f32 {
        let valid: Vec<f32> = self
            .temperatures
            .iter()
            .filter_map(|t| t.temperature.filter(|t| t.is_finite()))
            .collect();
        if valid.is_empty() {
            0.0
        } else {
            valid.iter().sum::<f32>() / valid.len() as f32
        }
    }

    pub fn avg_disk_usage(&self) -> f32 {
        if self.disks.is_empty() {
            0.0
        } else {
            self.disks.iter().map(|d| d.usage_pct).sum::<f32>() / self.disks.len() as f32
        }
    }
}

struct RatePairTracker<K: Eq + Hash> {
    prev: HashMap<K, (u64, u64)>,
}

impl<K: Eq + Hash> RatePairTracker<K> {
    #[cfg(test)]
    fn new() -> Self {
        Self {
            prev: HashMap::new(),
        }
    }

    fn from_iter(iter: impl Iterator<Item = (K, u64, u64)>) -> Self {
        let mut prev = HashMap::new();
        for (key, a, b) in iter {
            prev.insert(key, (a, b));
        }
        Self { prev }
    }

    fn rate_pair(&mut self, key: K, cum_a: u64, cum_b: u64) -> (u64, u64) {
        let (prev_a, prev_b) = self.prev.entry(key).or_insert((cum_a, cum_b));
        let rate_a = cum_a.saturating_sub(*prev_a);
        let rate_b = cum_b.saturating_sub(*prev_b);
        *prev_a = cum_a;
        *prev_b = cum_b;
        (rate_a, rate_b)
    }

    fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.prev.retain(|k, _| f(k));
    }
}

pub struct Samplers {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    network_rates: RatePairTracker<String>,
    disk_io_rates: RatePairTracker<String>,
    hostname: String,
    os: String,
    kernel: String,
    arch: String,
    physical_cores: usize,
}

impl Samplers {
    pub fn new() -> Self {
        let networks = Networks::new_with_refreshed_list();
        let network_rates = RatePairTracker::from_iter(networks.iter().map(|(name, data)| {
            (
                name.clone(),
                data.total_received(),
                data.total_transmitted(),
            )
        }));
        let disks = Disks::new_with_refreshed_list();
        let disk_io_rates = RatePairTracker::from_iter(disks.list().iter().map(|d| {
            let usage = d.usage();
            (
                d.mount_point().display().to_string(),
                usage.read_bytes,
                usage.written_bytes,
            )
        }));
        Self {
            sys: System::new(),
            networks,
            disks,
            components: Components::new_with_refreshed_list(),
            network_rates,
            disk_io_rates,
            hostname: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: System::cpu_arch(),
            physical_cores: System::physical_core_count().unwrap_or(1),
        }
    }

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

        let (interfaces, net_rx_rate, net_tx_rate) = self.collect_networks();
        let disks = self.collect_disk_info();
        let (disk_io, disk_read_rate, disk_write_rate) = self.collect_disk_io();
        let temperatures = self.collect_temperatures();
        let sys_info = self.collect_sys_info();
        let cpus = self.collect_cpus();
        let load = System::load_average();

        Samples {
            sys_info,
            cpu_usage: self.sys.global_cpu_usage(),
            cpus,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            mem_available: self.sys.available_memory(),
            mem_free: self.sys.free_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disks,
            disk_io,
            disk_read_rate,
            disk_write_rate,
            interfaces,
            net_rx_rate,
            net_tx_rate,
            temperatures,
            processes,
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
        }
    }

    fn collect_networks(&mut self) -> (Vec<NetInfo>, u64, u64) {
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
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            })
            .collect();
        raw.sort_by(|a, b| a.0.cmp(&b.0));

        let mut interfaces = Vec::with_capacity(raw.len());
        let mut net_rx_rate = 0u64;
        let mut net_tx_rate = 0u64;
        for (name, rx_cum, tx_cum, state, mac, ip) in raw {
            let (rx_rate, tx_rate) = self.network_rates.rate_pair(name.clone(), rx_cum, tx_cum);
            net_rx_rate = net_rx_rate.saturating_add(rx_rate);
            net_tx_rate = net_tx_rate.saturating_add(tx_rate);
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

        let iface_names: HashSet<&str> = interfaces.iter().map(|i| i.name.as_str()).collect();
        self.network_rates
            .retain(|k| iface_names.contains(k.as_str()));

        (interfaces, net_rx_rate, net_tx_rate)
    }

    fn collect_disk_info(&self) -> Vec<DiskInfo> {
        let mut disks: Vec<DiskInfo> = self
            .disks
            .iter()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used = total.saturating_sub(available);
                DiskInfo {
                    device: d.name().to_string_lossy().into_owned(),
                    mount_point: d.mount_point().display().to_string(),
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
        disks.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        disks
    }

    fn collect_disk_io(&mut self) -> (Vec<DiskIoInfo>, u64, u64) {
        let mut disk_io: Vec<DiskIoInfo> = Vec::new();
        let mut disk_read_rate = 0u64;
        let mut disk_write_rate = 0u64;
        for d in self.disks.list() {
            let usage = d.usage();
            let mount_point = d.mount_point().display().to_string();
            let (read_rate, write_rate) = self.disk_io_rates.rate_pair(
                mount_point.clone(),
                usage.read_bytes,
                usage.written_bytes,
            );
            disk_read_rate = disk_read_rate.saturating_add(read_rate);
            disk_write_rate = disk_write_rate.saturating_add(write_rate);

            disk_io.push(DiskIoInfo {
                mount_point,
                read_rate,
                write_rate,
            });
        }

        disk_io.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        let mount_points: HashSet<&str> = disk_io.iter().map(|d| d.mount_point.as_str()).collect();
        self.disk_io_rates
            .retain(|k| mount_points.contains(k.as_str()));

        (disk_io, disk_read_rate, disk_write_rate)
    }

    fn collect_temperatures(&self) -> Vec<TempInfo> {
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
        temperatures
    }

    fn collect_sys_info(&self) -> SysInfo {
        SysInfo {
            hostname: self.hostname.clone(),
            os: self.os.clone(),
            kernel: self.kernel.clone(),
            arch: self.arch.clone(),
            uptime: System::uptime(),
            cpu_count: self.sys.cpus().len(),
            boot_time: System::boot_time(),
            physical_cores: self.physical_cores,
            distro: System::distribution_id(),
        }
    }

    fn collect_cpus(&self) -> Vec<CpuInfo> {
        self.sys
            .cpus()
            .iter()
            .map(|c| CpuInfo {
                label: c.name().to_string(),
                usage: c.cpu_usage(),
                freq: c.frequency(),
            })
            .collect()
    }

    pub fn kill_process(&self, pid: u32, signal: Signal) -> KillResult {
        let sys_pid = Pid::from(pid as usize);
        if sys_pid == Pid::from(std::process::id() as usize) {
            return KillResult::SelfTarget;
        }
        self.sys
            .process(sys_pid)
            .map_or(KillResult::NotFound, |p| match p.kill_with(signal) {
                Some(true) => KillResult::Killed,
                Some(false) => KillResult::NotFound,
                None => KillResult::PermissionDenied,
            })
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
            mount_point: String::from("/"),
            fs: String::from("ext4"),
            total: 1_000_000_000_000,
            available: 500_000_000_000,
            usage_pct: 50.0,
            kind: String::from("ssd"),
        };
        assert_eq!(info.device, "/dev/sda1");
        assert_eq!(info.mount_point, "/");
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

    #[test]
    fn kill_result_messages() {
        assert_eq!(KillResult::Killed.message(42), "Killed PID 42");
        assert_eq!(KillResult::NotFound.message(42), "PID 42 not found");
        assert_eq!(
            KillResult::PermissionDenied.message(42),
            "Permission denied for PID 42"
        );
        assert_eq!(
            KillResult::SelfTarget.message(42),
            "Cannot kill thrum itself"
        );
    }

    #[test]
    fn rate_pair_tracker_tracks_rates() {
        let mut tracker: RatePairTracker<String> = RatePairTracker::new();
        assert_eq!(tracker.rate_pair("a".to_owned(), 100, 200), (0, 0));
        assert_eq!(tracker.rate_pair("a".to_owned(), 150, 250), (50, 50));
        assert_eq!(tracker.rate_pair("b".to_owned(), 300, 400), (0, 0));
        assert_eq!(tracker.rate_pair("b".to_owned(), 350, 500), (50, 100));
        tracker.retain(|k| k == "a");
        assert_eq!(tracker.rate_pair("a".to_owned(), 180, 300), (30, 50));
        assert_eq!(tracker.rate_pair("b".to_owned(), 350, 500), (0, 0));
    }

    #[test]
    fn avg_temperature_empty_returns_zero() {
        let samples = Samples::default();
        assert!((samples.avg_temperature() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn avg_temperature_with_values() {
        let samples = Samples {
            temperatures: vec![
                TempInfo {
                    temperature: Some(30.0),
                    ..TempInfo::default()
                },
                TempInfo {
                    temperature: Some(50.0),
                    ..TempInfo::default()
                },
            ],
            ..Samples::default()
        };
        assert!((samples.avg_temperature() - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn avg_temperature_ignores_none() {
        let samples = Samples {
            temperatures: vec![
                TempInfo {
                    temperature: Some(30.0),
                    ..TempInfo::default()
                },
                TempInfo {
                    temperature: None,
                    ..TempInfo::default()
                },
            ],
            ..Samples::default()
        };
        assert!((samples.avg_temperature() - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn avg_disk_usage_empty_returns_zero() {
        let samples = Samples::default();
        assert!((samples.avg_disk_usage() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn avg_disk_usage_with_values() {
        let samples = Samples {
            disks: vec![
                DiskInfo {
                    usage_pct: 20.0,
                    ..DiskInfo::default()
                },
                DiskInfo {
                    usage_pct: 60.0,
                    ..DiskInfo::default()
                },
            ],
            ..Samples::default()
        };
        assert!((samples.avg_disk_usage() - 40.0).abs() < f32::EPSILON);
    }
}
