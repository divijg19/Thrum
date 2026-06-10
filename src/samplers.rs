use std::cmp::Ordering;

use sysinfo::{InterfaceOperationalState, Networks, ProcessesToUpdate, System};

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

pub struct Samples {
    pub cpu_usage: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub processes: Vec<ProcessInfo>,
    pub interfaces: Vec<NetInfo>,
}

pub struct Samplers {
    sys: System,
    networks: Networks,
}

impl Samplers {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new_with_refreshed_list(),
        }
    }

    pub fn sample(&mut self, refresh_proc: bool) -> Samples {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        if refresh_proc {
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
        }
        self.networks.refresh(true);

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
                name: name.to_string(),
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

        Samples {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            processes,
            interfaces,
        }
    }
}
