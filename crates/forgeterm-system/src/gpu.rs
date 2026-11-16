//! GPU usage, read from documented vendor interfaces only:
//!
//! - **NVIDIA** (Windows and Linux): NVML, the library that ships with the driver
//!   (`nvml.dll` / `libnvidia-ml.so`), loaded at runtime. No driver: not available.
//! - **AMD on Linux:** the amdgpu driver's sysfs files (`gpu_busy_percent`,
//!   `mem_info_vram_used`, `mem_info_vram_total`).
//! - **Anything else** (AMD/Intel on Windows, Intel on Linux): not measured; the
//!   reason says so. Nothing is estimated.
//!
//! Opening NVML can wake a laptop's sleeping discrete GPU, so the app keeps a
//! [`GpuProbe`] only while the monitor panel is open and drops it when it closes.

use std::fs;
use std::path::{Path, PathBuf};

/// One GPU's measured values; a value the interface does not give is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuUsage {
    pub name: String,
    /// Busy share over the driver's last interval, 0–100.
    pub utilization_percent: Option<u32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature_c: Option<u32>,
    /// Where the numbers come from ("NVML", "amdgpu sysfs").
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuStatus {
    Measured(Vec<GpuUsage>),
    /// Why there are no numbers, for the user.
    Unavailable(String),
}

enum Backend {
    // Boxed: the loaded library table is about 12 KB.
    Nvml(Box<nvml_wrapper::Nvml>),
    Sysfs(Vec<PathBuf>),
    None(String),
}

/// An open connection to whatever GPU interface this machine has.
pub struct GpuProbe {
    backend: Backend,
}

impl GpuProbe {
    /// Tries NVML, then (Linux) amdgpu sysfs.
    pub fn open() -> Self {
        let nvml_error = match nvml_wrapper::Nvml::init() {
            Ok(nvml) => match nvml.device_count() {
                Ok(n) if n > 0 => {
                    return Self {
                        backend: Backend::Nvml(Box::new(nvml)),
                    };
                }
                Ok(_) => "the NVIDIA driver reports no GPU".to_owned(),
                Err(e) => format!("NVIDIA driver: {e}"),
            },
            Err(e) => format!("no NVIDIA driver (NVML: {e})"),
        };
        if cfg!(target_os = "linux") {
            let cards = amdgpu_cards(Path::new("/sys/class/drm"));
            if !cards.is_empty() {
                return Self {
                    backend: Backend::Sysfs(cards),
                };
            }
        }
        let other = if cfg!(target_os = "linux") {
            "no AMD GPU with amdgpu sysfs counters; Intel GPUs are not measured"
        } else {
            "AMD and Intel GPUs are not measured on this system yet"
        };
        tracing::info!("GPU not measured: {nvml_error}; {other}");
        Self {
            backend: Backend::None(format!("{nvml_error}; {other}")),
        }
    }

    pub fn sample(&self) -> GpuStatus {
        match &self.backend {
            Backend::Nvml(nvml) => nvml_sample(nvml),
            Backend::Sysfs(cards) => {
                let gpus: Vec<GpuUsage> = cards.iter().filter_map(|c| sysfs_sample(c)).collect();
                if gpus.is_empty() {
                    GpuStatus::Unavailable("the amdgpu counters could not be read".to_owned())
                } else {
                    GpuStatus::Measured(gpus)
                }
            }
            Backend::None(reason) => GpuStatus::Unavailable(reason.clone()),
        }
    }
}

fn nvml_sample(nvml: &nvml_wrapper::Nvml) -> GpuStatus {
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
    let count = match nvml.device_count() {
        Ok(n) => n,
        Err(e) => return GpuStatus::Unavailable(format!("NVIDIA driver: {e}")),
    };
    let mut gpus = Vec::new();
    for i in 0..count {
        let Ok(d) = nvml.device_by_index(i) else {
            continue;
        };
        let memory = d.memory_info().ok();
        gpus.push(GpuUsage {
            name: d.name().unwrap_or_else(|_| format!("NVIDIA GPU {i}")),
            utilization_percent: d.utilization_rates().ok().map(|u| u.gpu.min(100)),
            memory_used: memory.as_ref().map(|m| m.used),
            memory_total: memory.as_ref().map(|m| m.total),
            temperature_c: d.temperature(TemperatureSensor::Gpu).ok(),
            source: "NVML",
        });
    }
    if gpus.is_empty() {
        GpuStatus::Unavailable("the NVIDIA driver gave no readable GPU".to_owned())
    } else {
        GpuStatus::Measured(gpus)
    }
}

/// `device` folders of AMD cards (`vendor` 0x1002) that have the busy counter.
fn amdgpu_cards(drm: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // card0, card1 — not the connectors (card0-HDMI-A-1) or render nodes.
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|e| e.path().join("device"))
        .filter(|d| {
            read_trim(&d.join("vendor")).is_some_and(|v| v.eq_ignore_ascii_case("0x1002"))
                && d.join("gpu_busy_percent").is_file()
        })
        .collect();
    cards.sort();
    cards
}

fn sysfs_sample(device: &Path) -> Option<GpuUsage> {
    let number = |file: &str| read_trim(&device.join(file))?.parse::<u64>().ok();
    let utilization = number("gpu_busy_percent")?;
    // hwmon temperature is in millidegrees.
    let temperature = fs::read_dir(device.join("hwmon"))
        .ok()
        .and_then(|mut d| d.next())
        .and_then(|e| e.ok())
        .and_then(|e| read_trim(&e.path().join("temp1_input")))
        .and_then(|t| t.parse::<u64>().ok())
        .map(|m| (m / 1000) as u32);
    let card = device
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(GpuUsage {
        name: format!("AMD GPU ({card})"),
        utilization_percent: Some(utilization.min(100) as u32),
        memory_used: number("mem_info_vram_used"),
        memory_total: number("mem_info_vram_total"),
        temperature_c: temperature,
        source: "amdgpu sysfs",
    })
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn amdgpu_sysfs_is_read_and_other_cards_are_skipped() {
        let root = std::env::temp_dir().join(format!("forgeterm-drm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        // An AMD card with counters and a temperature sensor.
        let amd = root.join("card1/device");
        write(&amd.join("vendor"), "0x1002\n");
        write(&amd.join("gpu_busy_percent"), "37\n");
        write(&amd.join("mem_info_vram_used"), "1073741824\n");
        write(&amd.join("mem_info_vram_total"), "8589934592\n");
        write(&amd.join("hwmon/hwmon3/temp1_input"), "54000\n");
        // Intel (vendor 0x8086), a connector folder and an AMD card without counters.
        write(&root.join("card0/device/vendor"), "0x8086\n");
        write(&root.join("card0/device/gpu_busy_percent"), "5\n");
        write(&root.join("card1-HDMI-A-1/device/vendor"), "0x1002\n");
        write(&root.join("card2/device/vendor"), "0x1002\n");

        let cards = amdgpu_cards(&root);
        assert_eq!(cards, vec![amd.clone()]);
        let g = sysfs_sample(&amd).unwrap();
        assert_eq!(g.name, "AMD GPU (card1)");
        assert_eq!(g.utilization_percent, Some(37));
        assert_eq!(g.memory_used, Some(1 << 30));
        assert_eq!(g.memory_total, Some(8 << 30));
        assert_eq!(g.temperature_c, Some(54));
        // A counter that cannot be read: no sample rather than a made-up one.
        write(&amd.join("gpu_busy_percent"), "n/a\n");
        assert_eq!(sysfs_sample(&amd), None);
        assert!(amdgpu_cards(&root.join("missing")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    /// Whatever this machine has: either real numbers in range or a reason.
    #[test]
    fn probe_measures_or_says_why() {
        match GpuProbe::open().sample() {
            GpuStatus::Measured(gpus) => {
                for g in gpus {
                    assert!(!g.name.is_empty());
                    assert!(g.utilization_percent.is_none_or(|u| u <= 100));
                    if let (Some(u), Some(t)) = (g.memory_used, g.memory_total) {
                        assert!(u <= t, "{g:?}");
                    }
                    println!("measured: {g:?}");
                }
            }
            GpuStatus::Unavailable(reason) => {
                assert!(!reason.is_empty());
                println!("not measured: {reason}");
            }
        }
    }
}
