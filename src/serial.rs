use crate::args::Args;
use anyhow::{bail, Context, Result};
use log::{info, warn};
use serialport5::{self, SerialPort, SerialPortBuilder, SerialPortInfo, SerialPortType};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn find_by_usb_info(args: &Args) -> Result<Option<SerialPortInfo>> {
    let ports = serialport5::available_ports().unwrap();
    for port in ports {
        let port_clone = port.clone();
        match port.port_type {
            SerialPortType::UsbPort(info) => {
                let pid = format!("{:04x}", info.pid);
                let vid = format!("{:04x}", info.vid);
                if args.product_id.clone().unwrap_or_default() == pid {
                    return Ok(Some(port_clone));
                } else if args.vendor_id.clone().unwrap_or_default() == vid {
                    return Ok(Some(port_clone));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

pub fn open_serial_port(args: &Args) -> Result<(SerialPort, String)> {
    let port_name = if args.product_id.is_some() || args.vendor_id.is_some() {
        find_by_usb_info(&args)?.map(|port_info| port_info.port_name)
    } else {
        args.port.clone()
    };

    let port_name = port_name.clone().context("Port not found!")?;
    let baud_rate = args.baud_rate.context("No baud rate specified!")?;
    info!("open port {:?} with rate of {}", port_name, baud_rate);
    let port = SerialPortBuilder::new()
        .baud_rate(baud_rate)
        .open(port_name.clone())?;

    Ok((port, port_name))
}

pub fn read_stdin_loop(port: Arc<Mutex<SerialPort>>, port_name: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let mut buffer = [0; 512];
    loop {
        let read = stdin
            .read(&mut buffer)
            .context("failed to read from sttin")?;
        if read == 0 {
            return Ok(());
        } else {
            let mut port = port.lock().unwrap();
            port.write(&buffer[..read])
                .context(format!("Failed to write to {}", port_name))?;
        }
    }
}

pub fn read_serial_loop(port: Arc<Mutex<SerialPort>>, port_name: &str) -> Result<()> {
    let mut buffer = [0; 512];
    loop {
        let mut port = port.lock().unwrap();
        match port.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => std::io::stdout()
                .write_all(&buffer[..n])
                .context("Failed to write to stdout")?,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => {
                warn!("Failed to read from {}: {}", port_name, e);
                bail!("");
            }
        }
    }
}

pub fn open_with_reconnect(args: &Args) -> Result<()> {
    let mut retry_count = 0;

    loop {
        let result = open_serial_port(&args);
        match result {
            Ok((port, name)) => {
                let port_arc = Arc::new(Mutex::new(port.try_clone()?));
                let port_arc_clone = port_arc.clone();

                // Spawn a thread to read from stdin and write to the serial port.
                let name_clone = name.clone();
                std::thread::spawn(move || {
                    if let Err(_) = read_stdin_loop(port_arc_clone, &name_clone) {
                        std::process::exit(1);
                    }
                });

                // Read from serial port and write to stdout in the main thread.
                match read_serial_loop(port_arc, &name) {
                    Ok(_) => {
                        // Successful read, break out of the loop
                        break;
                    }
                    Err(_) => {
                        // Reconnect
                        // Delay before attempting the next reconnect
                        std::thread::sleep(Duration::from_secs(1));

                        if let Some(true) = args.write_on_reconnect {
                            let _ = std::io::stdout().write("\n -- RECONNECT -- \n\n".as_bytes());
                        }

                        // Decrease the retry count
                        retry_count -= 1;

                        // Log a message or take any other necessary action
                        log::warn!("Reconnecting... Retries left: {}", retry_count);
                    }
                }
            }
            _ => {
                retry_count += 1;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Ok(())
}

pub fn open(args: &Args) -> Result<()> {
    // Connect normally without reconnection logic
    let (port, name): (SerialPort, String) = open_serial_port(&args)?;
    let port_arc = Arc::new(Mutex::new(port));

    let port_arc_clone = port_arc.clone();

    // Spawn a thread to read from stdin and write to the serial port.
    let name_clone = name.clone();
    std::thread::spawn(move || {
        if let Err(_) = read_stdin_loop(port_arc_clone, &name_clone) {
            std::process::exit(1);
        }
    });

    // Read from serial port and write to stdout in the main thread.
    match read_serial_loop(port_arc, &name) {
        Err(_) => {
            // Handle any specific error logic if needed
        }
        _ => {}
    }
    Ok(())
}
