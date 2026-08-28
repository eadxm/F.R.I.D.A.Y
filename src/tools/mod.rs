use std::process::Command;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

/// Executes a PowerShell command silently in the background
pub fn run_terminal_command(command: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();

                if !stderr.is_empty() {
                    format!("Command Error:\n{}", stderr)
                } else if stdout.is_empty() {
                    "Command executed successfully with no output.".to_string()
                } else {
                    stdout
                }
            }
            Err(e) => format!("Failed to invoke PowerShell: {}", e),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        "Command execution is only implemented for Windows systems.".to_string()
    }
}

/// Launches a local executable, Windows app, or URL
pub fn open_application(target: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let formatted_cmd = format!("Start-Process '{}'", target);
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &formatted_cmd])
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    format!("Successfully launched '{}'.", target)
                } else {
                    let err = String::from_utf8_lossy(&out.stderr);
                    format!("Could not launch '{}': {}", target, err)
                }
            }
            Err(e) => format!("Failed to spawn process: {}", e),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        "Application launching is only implemented for Windows systems.".to_string()
    }
}

/// Queries current CPU usage, RAM utilization, and available disk storage
pub fn read_system_info() -> String {
    let mut sys = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );

    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();

    let cpu_usage = sys.global_cpu_info().cpu_usage();
    let total_ram = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let used_ram = sys.used_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

    let disks = Disks::new_with_refreshed_list();
    let mut disk_info = String::new();
    for disk in &disks {
        let name = disk.mount_point().to_string_lossy();
        let total = disk.total_space() as f64 / (1024.0 * 1024.0 * 1024.0);
        let available = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
        disk_info.push_str(&format!(
            "\n  - Drive {}: {:.1} GB free / {:.1} GB total",
            name, available, total
        ));
    }

    format!(
        "System Metrics:\n- CPU Usage: {:.1}%\n- Memory: {:.2} GB / {:.2} GB used ({:.1}%)\n- Disks:{}",
        cpu_usage,
        used_ram,
        total_ram,
        (used_ram / total_ram) * 100.0,
        if disk_info.is_empty() { " None detected".to_string() } else { disk_info }
    )
}