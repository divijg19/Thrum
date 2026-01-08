use std::cmp::Ordering;

use sysinfo::{ProcessesToUpdate, System};

pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub cpu: f32,
    pub memory: u64,
    pub status: String,
}

pub struct Samples {
    pub cpu_usage: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub processes: Vec<ProcessInfo>,
}

pub struct Samplers {
    sys: System,
}

impl Samplers {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
        }
    }

    pub fn sample(&mut self, refresh_proc: bool) -> Samples {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        if refresh_proc {
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
        }

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

        Samples {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            processes,
        }
    }
}
