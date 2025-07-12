#![allow(dead_code)]

use chrono::Local;
use regex::Regex;
use serde_json::json;
use std::error::Error;
use std::fs::{self, read_to_string};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{Components, Disks, Networks, System};

const BLACK: &str = "#15161E";
const RED: &str = "#f7768e";
const GREEN: &str = "#9ece6a";
const YELLOW: &str = "#e0af68";
const BLUE: &str = "#7aa2f7";
const MAGENTA: &str = "#bb9af7";
const CYAN: &str = "#7dcfff";
const WHITE: &str = "#a9b1d6";

const WIFI_INTERFACE: &str = "wlp2s0";
const VPN_INTERFACE: &str = "nordlynx";
const ETH_INTERFACE: &str = "enp3s0f0";

struct NetTracker {
    last_up: u64,
    last_down: u64,
    last_time: std::time::Instant,
}
/// Read integer from a file, useful for fan speed and other metrics.

fn read_int_from_file(path: &str) -> Result<u32, Box<dyn Error>> {
    let data = fs::read_to_string(path)?;
    let number = data.trim().parse::<u32>()?;
    Ok(number)
}

/// Read system load averages from `/proc/loadavg`.
fn read_load_avg(path: &str) -> Result<(f32, f32, f32), Box<dyn Error>> {
    let data = fs::read_to_string(path)?;
    let numbers: Vec<&str> = data.split_whitespace().collect();
    let load1 = numbers
        .get(0)
        .ok_or("Missing 01 load avg")?
        .parse::<f32>()?;
    let load2 = numbers
        .get(1)
        .ok_or("Missing 05 load avg")?
        .parse::<f32>()?;
    let load3 = numbers
        .get(2)
        .ok_or("Missing 15 load avg")?
        .parse::<f32>()?;
    Ok((load1, load2, load3))
}

/// Convert bytes into a human-readable format (e.g., MB, GB).
fn readable_bytes(mut num: f32) -> String {
    for unit in ["B", "KB", "MB", "GB", "TB", "PB"].iter() {
        if num < 1024.0 {
            return format!("{num:.2}{unit}");
        } else {
            num /= 1024.0;
        }
    }
    "ERROR".to_string()
}

/// Fetch current system volume using `pactl`.
fn get_volume() -> Option<u32> {
    let output = Command::new("pactl")
        .args(&["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let re = Regex::new(r"/\s*(\d+)%").unwrap();
    re.captures(&stdout)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

/// Format the volume into a human-readable string with an icon.
fn format_volume(vol: u32) -> String {
    let icon = match vol {
        0 => "",
        //1..=30 => "",
        //31..=70 => "",
        _ => "",
    };
    format!("{}  {}", icon, vol)
}

/// Check if a network interface is enabled.
fn check_interface_enable(iface: &str) -> bool {
    read_int_from_file(format!("/sys/class/net/{}/carrier", iface).as_str()).unwrap_or(0) == 1
}

/// Check if a network interface is up.
fn check_interface_up(iface: &str) -> bool {
    read_to_string(format!("/sys/class/net/{}/operstate", iface))
        .unwrap_or_else(|_| "down".to_string())
        .trim()
        == "up"
}

/// Get the current brightness level.
fn get_brightness() -> Result<u32, Box<dyn Error>> {
    let data0 = read_to_string("/sys/class/backlight/acpi_video0/brightness")?;
    let data1 = read_to_string("/sys/class/backlight/acpi_video0/max_brightness")?;
    let brightness = data0.trim().parse::<u32>()?;
    let brightness_max = data1.trim().parse::<u32>()?;
    Ok(((brightness as f32 / brightness_max as f32) * 100.0) as u32)
}

/// Get the fan speed (in RPM) from system sensors.
fn get_fan_speed() -> Result<u32, Box<dyn Error>> {
    let path = "/sys/class/hwmon/hwmon0/device/fan1_input";
    let fan_speed = read_int_from_file(path)?;
    Ok(fan_speed)
}

/// Get the system's IP address.
fn get_ip_address() -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("ip").arg("a").output()?;
    let ip_address = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut ip = Vec::new();
    for x in ip_address.lines() {
        if x.contains("inet ") && !x.contains("127.0.0.1") {
            ip.push(format!(
                "{} {}",
                x.split_whitespace().into_iter().last().unwrap().to_string(),
                x.split_whitespace().into_iter().nth(1).unwrap().to_string()
            ))
        }
    }
    Ok(ip)
}

/// Print the system status as JSON.
fn print_status(sys: &mut System, volume: u32, tracker: &mut NetTracker) {
    let now = Local::now();
    let time = now.format("%H:%M:%S").to_string();
    let day = now.format("%A, %d %B %Y").to_string();

    sys.refresh_cpu_all();
    sys.refresh_memory();

    let disks = Disks::new_with_refreshed_list();
    let components = Components::new_with_refreshed_list();
    let networks = Networks::new_with_refreshed_list();
    let mut status = Vec::new();

    // Network
    // let wifi_up = check_interface_up(WIFI_INTERFACE);
    // let vpn_up = check_interface_enable(VPN_INTERFACE);
    // let ethernet_up = check_interface_up(ETH_INTERFACE);
    //
    // let now = std::time::Instant::now();
    // let elapsed = now.duration_since(tracker.last_time).as_secs_f32();
    //
    // if vpn_up && ethernet_up {
    //     if let Some(vpn) = networks.get(VPN_INTERFACE) {
    //         let current_up = vpn.total_transmitted();
    //         let current_down = vpn.total_received();
    //         let rate_up = if elapsed > 0.0 {
    //             (current_up - tracker.last_up) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         let rate_down = if elapsed > 0.0 {
    //             (current_down - tracker.last_down) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         tracker.last_up = current_up;
    //         tracker.last_down = current_down;
    //         tracker.last_time = now;
    //
    //         status.push(json!({
    //             "full_text": format!("   {}  {}s  {}s",
    //                 get_country_code().unwrap_or("..".to_string()),
    //                 readable_bytes(rate_up),
    //                 readable_bytes(rate_down)),
    //             "name": "net"
    //         }));
    //     }
    // } else if vpn_up {
    //     if let Some(vpn) = networks.get(VPN_INTERFACE) {
    //         let current_up = vpn.total_transmitted();
    //         let current_down = vpn.total_received();
    //         let rate_up = if elapsed > 0.0 {
    //             (current_up - tracker.last_up) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         let rate_down = if elapsed > 0.0 {
    //             (current_down - tracker.last_down) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         tracker.last_up = current_up;
    //         tracker.last_down = current_down;
    //         tracker.last_time = now;
    //
    //         status.push(json!({
    //             "full_text": format!("   {}  {}s  {}s",
    //                 get_country_code().unwrap_or("..".to_string()),
    //                 readable_bytes(rate_up),
    //                 readable_bytes(rate_down)),
    //             "name": "net"
    //         }));
    //     }
    // } else if ethernet_up {
    //     if let Some(ethernet) = networks.get(ETH_INTERFACE) {
    //         let current_up = ethernet.total_transmitted();
    //         let current_down = ethernet.total_received();
    //         let rate_up = if elapsed > 0.0 {
    //             (current_up - tracker.last_up) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         let rate_down = if elapsed > 0.0 {
    //             (current_down - tracker.last_down) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         tracker.last_up = current_up;
    //         tracker.last_down = current_down;
    //         tracker.last_time = now;
    //
    //         status.push(json!({
    //             "full_text": format!("   {}s  {}s",
    //                 readable_bytes(rate_up),
    //                 readable_bytes(rate_down)),
    //             "name": "net",
    //             "color" : BLUE,
    //         }));
    //     }
    // } else if wifi_up {
    //     if let Some(wifi) = networks.get(WIFI_INTERFACE) {
    //         let current_up = wifi.total_transmitted();
    //         let current_down = wifi.total_received();
    //         let rate_up = if elapsed > 0.0 {
    //             (current_up - tracker.last_up) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         let rate_down = if elapsed > 0.0 {
    //             (current_down - tracker.last_down) as f32 / elapsed
    //         } else {
    //             0.0
    //         };
    //         tracker.last_up = current_up;
    //         tracker.last_down = current_down;
    //         tracker.last_time = now;
    //
    //         status.push(json!({
    //             "full_text": format!("   {}s {}s",
    //                 readable_bytes(rate_up),
    //                 readable_bytes(rate_down)),
    //             "name": "net"
    //         }));
    //     }
    // }

    // Storage
    //if let Some(disk) = disks.first() {
    //    status.push(json!({
    //        "full_text": format!("󰋊 {:4.1}",
    //            ((disk.total_space() - disk.available_space()) as f32 / disk.total_space() as f32) * 100.0),
    //        "name": "storage"
    //    }));
    //}
    // Temperature
    // if let Some(temp) = components.first() {
    //     if let Some(temperature) = temp.temperature() {
    //         status.push(json!({
    //             "full_text": format!(" {}C", temperature),
    //             "name": "temperature"
    //         }));
    //     }
    // }

    // Load Average
    // status.push(json!({
    //     "full_text": format!("󰓅 {:.1}", read_load_avg("/proc/loadavg").unwrap().0),
    //     "name": "load"
    // }));

    // CPU Usage
    //status.push(json!({
    //    "full_text": format!(" {:4.1}", sys.global_cpu_usage()),
    //    "name": "cpu"
    //}));
    // Memory Usage
    //status.push(json!({
    //     "full_text": format!(" {:4.1}", (sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0),
    //     "name": "memory"
    // }));

    // Volume
    status.push(json!({
        "full_text": format_volume(volume),
        "name": "volume",
    }));

    // Brightness
    if let Ok(brightness) = get_brightness() {
        status.push(json!({
            "full_text": format!("  {}", brightness),
            "name": "brightness",
        }));
    }

    // Fan Speed
    // if let Ok(fan_speed) = get_fan_speed() {
    //     status.push(json!({
    //         "full_text": format!(" {} RPM", fan_speed),
    //         "name": "fan"
    //     }));
    // }

    // IP Address
    //if let Ok(ip) = get_ip_address() {
    //   for x in ip {
    //        status.push(json!({
    //            "full_text": format!(" {}", x),
    //            "name": "ip",
    //        }));
    //    }
    //}

    // Time & Date
    status.push(json!({
        "full_text": format!("󰥔  {} ", time),
        "name": "clock",
    }));
    // status.push(json!({
    //      "full_text": format!("  {}", day),
    //      "name": "date"
    // }));

    // Output status as JSON
    println!("{},", serde_json::to_string(&status).unwrap());
}

fn get_country_code() -> Result<String, Box<dyn Error>> {
    let output = Command::new("nordvpn").arg("status").output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("Hostname:") {
            if let Some(hostname) = line.split_whitespace().nth(1) {
                return Ok(hostname
                    .chars()
                    .map(|x| x.to_ascii_uppercase())
                    .take(2)
                    .collect());
            }
        }
    }
    Err("Hostname Line not found".into())
}

fn main() {
    println!(r#"{{ "version": 1 }}"#);
    println!("[");

    let volume = Arc::new(Mutex::new(get_volume().unwrap_or(0)));
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let mut sys = System::new_all();

    // Volume change listener thread
    {
        let volume_clone = Arc::clone(&volume);
        let pair_clone = Arc::clone(&pair);

        thread::spawn(move || {
            let mut child = Command::new("pactl")
                .arg("subscribe")
                .stdout(Stdio::piped())
                .spawn()
                .expect("Failed to run pactl subscribe");

            let stdout = child.stdout.take().expect("No stdout from pactl");
            let reader = BufReader::new(stdout);

            for line in reader.lines() {
                if let Ok(event) = line {
                    if event.contains("Event 'change' on sink") {
                        if let Some(new_vol) = get_volume() {
                            let mut vol_lock = volume_clone.lock().unwrap();
                            if *vol_lock != new_vol {
                                *vol_lock = new_vol;
                                let (lock, cvar) = &*pair_clone;
                                let mut notified = lock.lock().unwrap();
                                *notified = true;
                                cvar.notify_one();
                            }
                        }
                    }
                }
            }
        });
    }

    let mut net_state = NetTracker {
        last_up: 0,
        last_down: 0,
        last_time: std::time::Instant::now(),
    };

    // First output
    print_status(&mut sys, *volume.lock().unwrap(), &mut net_state);

    // Subsequent updates
    let (lock, cvar) = &*pair;
    loop {
        let notified = lock.lock().unwrap();
        let _ = cvar.wait_timeout(notified, Duration::from_secs(1)).unwrap();

        let vol = *volume.lock().unwrap();
        print_status(&mut sys, vol, &mut net_state);
    }
}
