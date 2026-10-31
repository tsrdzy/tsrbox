use serde_json::{json, Value};
use sysinfo::{Disks, Networks, System};

fn ps_lines(script: &str) -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
        if let Ok(output) = output {
            return String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
        }
        Vec::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = script;
        Vec::new()
    }
}

fn graphics_info() -> Vec<Value> {
    ps_lines("Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }")
        .into_iter()
        .map(|name| json!({ "vendor": "", "model": name, "memory": 0 }))
        .collect()
}

fn board_info() -> Value {
    let lines = ps_lines(
        "Get-CimInstance Win32_BaseBoard | ForEach-Object { \"$($_.Manufacturer)|$($_.Product)|$($_.Version)|$($_.SerialNumber)\" }",
    );
    if let Some(line) = lines.first() {
        let parts: Vec<&str> = line.split('|').collect();
        return json!({
            "manufacturer": parts.first().copied().unwrap_or(""),
            "model": parts.get(1).copied().unwrap_or(""),
            "version": parts.get(2).copied().unwrap_or(""),
            "serial": parts.get(3).copied().unwrap_or("")
        });
    }
    json!({})
}

fn primary_ipv4() -> String {
    ps_lines(
        "Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1 -ExpandProperty IPAddress",
    )
    .first()
    .cloned()
    .unwrap_or_default()
}

pub fn collect() -> Value {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpus = sys.cpus();
    let cpu_brand = cpus
        .first()
        .map(|cpu| cpu.brand())
        .unwrap_or("")
        .to_string();
    let cpu_vendor = cpus
        .first()
        .map(|cpu| cpu.vendor_id())
        .unwrap_or("")
        .to_string();
    let cpu_speed = cpus.first().map(|cpu| cpu.frequency()).unwrap_or(0);
    let logical_cores = cpus.len() as u64;

    let mut disk_items = Vec::new();
    let mut ssd_size: u64 = 0;
    let disks = Disks::new_with_refreshed_list();
    for disk in disks.list() {
        let is_ssd = disk.kind() == sysinfo::DiskKind::SSD;
        let size = disk.total_space();
        if is_ssd {
            ssd_size += size;
        }
        disk_items.push(json!({
            "name": disk.name().to_string_lossy(),
            "type": format!("{:?}", disk.kind()),
            "size": size,
            "isSSD": is_ssd
        }));
    }

    let mut network_items = Vec::new();
    let networks = Networks::new_with_refreshed_list();
    for (interface, data) in networks.iter() {
        let mac = data.mac_address().to_string();
        if mac == "00:00:00:00:00:00" {
            continue;
        }
        network_items.push(json!({
            "interface": interface,
            "ip4": "",
            "ip6": "",
            "mac": mac
        }));
    }
    let primary_ip = primary_ipv4();
    if network_items.is_empty() {
        network_items.push(json!({
            "interface": "",
            "ip4": primary_ip,
            "ip6": "",
            "mac": ""
        }));
    } else if !primary_ip.is_empty() {
        if let Some(first) = network_items.first_mut() {
            first["ip4"] = json!(primary_ip);
        }
    }

    json!({
        "os": {
            "platform": std::env::consts::OS,
            "distro": System::name().unwrap_or_default(),
            "release": System::os_version().unwrap_or_default(),
            "kernel": System::kernel_version().unwrap_or_default(),
            "arch": std::env::consts::ARCH
        },
        "cpu": {
            "manufacturer": cpu_vendor,
            "brand": cpu_brand,
            "speed": cpu_speed,
            "cores": logical_cores,
            "logicalCores": logical_cores
        },
        "memory": {
            "total": sys.total_memory(),
            "free": sys.free_memory()
        },
        "graphics": graphics_info(),
        "disk": disk_items,
        "ssdSize": ssd_size,
        "board": board_info(),
        "network": network_items
    })
}
