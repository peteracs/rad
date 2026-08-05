#[cfg(not(target_arch = "wasm32"))]
fn tcp_accept_with_timeout(
    listener: &std::net::TcpListener,
    timeout_ms: i64,
) -> Result<Option<std::net::TcpStream>, String> {
    listener.set_nonblocking(true).map_err(|e| {
        format!(
            "tcp_accept_timeout() failed to enter nonblocking mode: {}",
            e
        )
    })?;
    let result = tcp_accept_nonblocking_loop(listener, timeout_ms);
    if let Err(e) = listener.set_nonblocking(false) {
        return Err(format!(
            "tcp_accept_timeout() failed to restore blocking mode: {}",
            e
        ));
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_accept_nonblocking_loop(
    listener: &std::net::TcpListener,
    timeout_ms: i64,
) -> Result<Option<std::net::TcpStream>, String> {
    let deadline = poll_deadline(timeout_ms, "tcp_accept_timeout()")?;
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => return Ok(Some(stream)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("tcp_accept_timeout() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct UdpPacket {
    data: Vec<u8>,
    addr: std::net::SocketAddr,
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_blocking(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
) -> Result<UdpPacket, String> {
    let mut buf = vec![0u8; max_bytes];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                buf.truncate(n);
                return Ok(UdpPacket { data: buf, addr });
            }
            Err(e) if udp_recv_error_is_transient(&e) => {}
            Err(e) => return Err(format!("udp_recv_from() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_with_timeout(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
    timeout_ms: i64,
) -> Result<Option<UdpPacket>, String> {
    socket.set_nonblocking(true).map_err(|e| {
        format!(
            "udp_recv_from_timeout() failed to enter nonblocking mode: {}",
            e
        )
    })?;
    let result = udp_recv_from_nonblocking_loop(socket, max_bytes, timeout_ms);
    if let Err(e) = socket.set_nonblocking(false) {
        return Err(format!(
            "udp_recv_from_timeout() failed to restore blocking mode: {}",
            e
        ));
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_nonblocking_loop(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
    timeout_ms: i64,
) -> Result<Option<UdpPacket>, String> {
    let deadline = poll_deadline(timeout_ms, "udp_recv_from_timeout()")?;
    let mut buf = vec![0u8; max_bytes];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let mut data = Vec::with_capacity(n);
                data.extend_from_slice(&buf[..n]);
                return Ok(Some(UdpPacket { data, addr }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) if udp_recv_error_is_transient(&e) => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) => return Err(format!("udp_recv_from_timeout() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_error_is_transient(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionReset
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_deadline(timeout_ms: i64, builtin: &str) -> Result<Option<Instant>, String> {
    if timeout_ms == 0 {
        return Ok(None);
    }
    Instant::now()
        .checked_add(Duration::from_millis(timeout_ms as u64))
        .map(Some)
        .ok_or_else(|| format!("{} timeout_ms is too large", builtin))
}

#[cfg(not(target_arch = "wasm32"))]
fn sleep_until_next_poll(deadline: Option<Instant>) -> bool {
    let Some(deadline) = deadline else {
        return false;
    };
    let now = Instant::now();
    if now >= deadline {
        return false;
    }
    let remaining = deadline.saturating_duration_since(now);
    let sleep_for = if remaining > Duration::from_millis(1) {
        Duration::from_millis(1)
    } else {
        remaining
    };
    if !sleep_for.is_zero() {
        std::thread::sleep(sleep_for);
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let data = String::from_utf8_lossy(&packet.data).into_owned();
    let data_value = Value::from_string(gc, data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_bytes_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let mut data = Vec::with_capacity(packet.data.len());
    for byte in packet.data {
        data.push(Value::from_int(gc, i64::from(byte)));
    }
    let data_value = Value::list(gc, data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_bytebuf_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let data_value = Value::bytebuf(gc, packet.data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

fn bytes_from_list_arg(value: &Value, fn_name: &str) -> Result<Vec<u8>, String> {
    let list = value.as_list().ok_or_else(|| {
        format!(
            "{} expects data list<int>, got {}",
            fn_name,
            value.type_name()
        )
    })?;
    let mut bytes = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let n = item
            .as_int()
            .ok_or_else(|| format!("{} data element {} is not an int", fn_name, i))?;
        if !(0..=255).contains(&n) {
            return Err(format!(
                "{} byte value {} out of range 0..255 at index {}",
                fn_name, n, i
            ));
        }
        bytes.push(n as u8);
    }
    Ok(bytes)
}

fn bytes_from_bytebuf_arg<'a>(value: &'a Value, fn_name: &str) -> Result<&'a [u8], String> {
    value
        .as_bytebuf()
        .map(|bytes| bytes.as_slice())
        .ok_or_else(|| format!("{} expects bytebuf, got {}", fn_name, value.type_name()))
}

fn bytebuf_index_arg(value: &Value, what: &str) -> Result<usize, String> {
    let index = value
        .as_int()
        .ok_or_else(|| format!("{} expects int, got {}", what, value.type_name()))?;
    if index < 0 {
        return Err(format!("{} must be non-negative", what));
    }
    usize::try_from(index).map_err(|_| format!("{} is too large", what))
}

fn bytebuf_u8_arg(value: &Value, what: &str) -> Result<u8, String> {
    let byte = value
        .as_int()
        .ok_or_else(|| format!("{} expects int, got {}", what, value.type_name()))?;
    if !(0..=255).contains(&byte) {
        return Err(format!("{} {} out of range 0..255", what, byte));
    }
    Ok(byte as u8)
}

fn bytebuf_write_u32_le(
    bytes: &mut [u8],
    offset: usize,
    value: u32,
    fn_name: &str,
) -> Result<(), String> {
    if offset + 4 > bytes.len() {
        return Err(format!(
            "{} offset {} out of bounds for 4-byte write (len {})",
            fn_name,
            offset,
            bytes.len()
        ));
    }
    bytes[offset] = (value & 0xff) as u8;
    bytes[offset + 1] = ((value >> 8) & 0xff) as u8;
    bytes[offset + 2] = ((value >> 16) & 0xff) as u8;
    bytes[offset + 3] = ((value >> 24) & 0xff) as u8;
    Ok(())
}

fn bytebuf_read_u32_le(bytes: &[u8], offset: usize, fn_name: &str) -> Result<u32, String> {
    if offset + 4 > bytes.len() {
        return Err(format!(
            "{} offset {} out of bounds for 4-byte read (len {})",
            fn_name,
            offset,
            bytes.len()
        ));
    }
    Ok(u32::from(bytes[offset])
        | (u32::from(bytes[offset + 1]) << 8)
        | (u32::from(bytes[offset + 2]) << 16)
        | (u32::from(bytes[offset + 3]) << 24))
}
