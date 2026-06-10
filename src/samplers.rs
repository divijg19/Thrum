use sysinfo::System;

pub struct Samples {
    pub cpu_usage: f32,
    pub mem_used: u64,
    pub mem_total: u64,
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

    pub fn sample(&mut self) -> Samples {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        Samples {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
        }
    }
}
