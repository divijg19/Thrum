use std::cmp::Ordering;

use sysinfo::{Components, Disks, InterfaceOperationalState, Networks, ProcessesToUpdate, System};

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
    pub load_one: f64,
    pub load_five: f64,
    pub load_fifteen: f64,
    pub processes: Vec<ProcessInfo>,
    pub interfaces: Vec<NetInfo>,
    pub disks: Vec<DiskInfo>,
    pub temperatures: Vec<TempInfo>,
    pub sys_info: SysInfo,
}

pub struct Samplers {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
}

impl Samplers {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
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

        let mut processes: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcessInfo {
                name: p.name().to_string_lossy().into_owned(),
                pid: pid.as_u32(),
                cpu: p.cpu_usage(),
                memory: p.memory(),
                status: format!("{:?}", p.status()),
            })
            .collect();

        processes.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(Ordering::Equal));

        let mut interfaces: Vec<NetInfo> = self
            .networks
            .iter()
            .map(|(name, data)| NetInfo {
                name: name.clone(),
                rx_bytes: data.received(),
                tx_bytes: data.transmitted(),
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
            hostname: System::host_name().unwrap_or_default(),
            os: System::long_os_version().unwrap_or_default(),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: System::cpu_arch(),
            uptime: System::uptime(),
        };

        let load = System::load_average();

        Samples {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
            processes,
            interfaces,
            disks,
            temperatures,
            sys_info,
        }
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
}
