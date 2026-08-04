impl VM {

    fn bi_tcp_accept(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("tcp_accept() requires exactly 1 argument: listener handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_accept() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("tcp_accept() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let listener = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::TcpListener(l)) => l,
                Some(_) => return Err("tcp_accept() handle is not a TcpListener".into()),
                None => return Err(format!("tcp_accept() invalid handle {}", handle_id)),
            };
            let (stream, _addr) = listener
                .accept()
                .map_err(|e| format!("tcp_accept() failed: {}", e))?;
            let client_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(client_id, super::NetHandle::TcpStream(stream));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, client_id as i64))
        }
    }

    fn bi_tcp_accept_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "tcp_accept_timeout() requires exactly 2 arguments: listener handle, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_accept_timeout() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let timeout_ms = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_accept_timeout() expects timeout_ms int, got {}",
                args[1].type_name()
            )
        })?;
        if timeout_ms < 0 {
            return Err("tcp_accept_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, timeout_ms);
            return Err("tcp_accept_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let maybe_stream = {
                let listener = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::TcpListener(l)) => l,
                    Some(_) => {
                        return Err("tcp_accept_timeout() handle is not a TcpListener".into())
                    }
                    None => {
                        return Err(format!("tcp_accept_timeout() invalid handle {}", handle_id))
                    }
                };
                tcp_accept_with_timeout(listener, timeout_ms)?
            };

            match maybe_stream {
                Some(stream) => {
                    let client_id = self.next_net_handle_id;
                    self.next_net_handle_id += 1;
                    self.net_handles
                        .insert(client_id, super::NetHandle::TcpStream(stream));
                    let value = Value::from_int(&mut self.gc, client_id as i64);
                    Ok(wrap_option(&mut self.gc, Some(value)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_tcp_read(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_read() requires exactly 2 arguments: handle, max_bytes".into());
        }
        let handle_id = args[0]
            .as_int()
            .ok_or_else(|| format!("tcp_read() expects handle int, got {}", args[0].type_name()))?
            as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_read() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("tcp_read() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("tcp_read() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Read;
            let stream = match self.net_handles.get_mut(&handle_id) {
                Some(super::NetHandle::TcpStream(s)) => s,
                Some(_) => return Err("tcp_read() handle is not a TcpStream".into()),
                None => return Err(format!("tcp_read() invalid handle {}", handle_id)),
            };
            let mut buf = vec![0u8; max_bytes as usize];
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("tcp_read() failed: {}", e))?;
            buf.truncate(n);
            let text = String::from_utf8_lossy(&buf).into_owned();
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn bi_tcp_write(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_write() requires exactly 2 arguments: handle, data".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_write() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let data = args[1].as_str().ok_or_else(|| {
            format!(
                "tcp_write() expects data string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, data);
            return Err("tcp_write() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write;
            let stream = match self.net_handles.get_mut(&handle_id) {
                Some(super::NetHandle::TcpStream(s)) => s,
                Some(_) => return Err("tcp_write() handle is not a TcpStream".into()),
                None => return Err(format!("tcp_write() invalid handle {}", handle_id)),
            };
            stream
                .write_all(data.as_bytes())
                .map_err(|e| format!("tcp_write() failed: {}", e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_tcp_close(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("tcp_close() requires exactly 1 argument: handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_close() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("tcp_close() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.net_handles.remove(&handle_id).is_none() {
                return Err(format!("tcp_close() invalid handle {}", handle_id));
            }
            Ok(Value::NIL)
        }
    }

    fn bi_udp_bind(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("udp_bind() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_bind() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1]
            .as_int()
            .ok_or_else(|| format!("udp_bind() expects port int, got {}", args[1].type_name()))?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("udp_bind() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let socket = std::net::UdpSocket::bind(&addr)
                .map_err(|e| format!("udp_bind() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::UdpSocket(socket));
            Ok(Value::from_int(&mut self.gc, handle_id as i64))
        }
    }

    fn bi_udp_recv_from(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("udp_recv_from() requires exactly 2 arguments: socket, max_bytes".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_from() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_from() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_recv_from() invalid handle {}", handle_id)),
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_from_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_from_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_from_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err("udp_recv_from_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_from_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_from_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_recv_from_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "udp_recv_from_bytes() requires exactly 2 arguments: socket, max_bytes".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_bytes() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_from_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_from_bytes() handle is not a UdpSocket".into()),
                None => {
                    return Err(format!(
                        "udp_recv_from_bytes() invalid handle {}",
                        handle_id
                    ))
                }
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_bytes_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_from_bytes_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_from_bytes_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_bytes_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_from_bytes_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err(
                "udp_recv_from_bytes_timeout() is not supported in wasm runtime".to_string(),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_from_bytes_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_from_bytes_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_bytes_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_recv_bytebuf(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "udp_recv_bytebuf() requires exactly 2 arguments: socket, max_bytes".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_bytebuf() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_bytebuf() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_bytebuf() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_recv_bytebuf() invalid handle {}", handle_id)),
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_bytebuf_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_bytebuf_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_bytebuf_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_bytebuf_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_bytebuf_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err("udp_recv_bytebuf_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_bytebuf_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_bytebuf_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_bytebuf_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_send_to(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_to() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_to() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_to() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let data = args[3]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to() expects data string, got {}",
                    args[3].type_name()
                )
            })?
            .to_string();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, data);
            return Err("udp_send_to() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_to() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_to() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(data.as_bytes(), &addr)
                .map_err(|e| format!("udp_send_to() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_send_to_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_to_bytes() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_to_bytes() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to_bytes() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_to_bytes() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let bytes = bytes_from_list_arg(&args[3], "udp_send_to_bytes()")?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, bytes);
            return Err("udp_send_to_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_to_bytes() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_to_bytes() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(&bytes, &addr)
                .map_err(|e| format!("udp_send_to_bytes() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_send_bytebuf(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_bytebuf() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_bytebuf() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_bytebuf() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_bytebuf() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let bytes = bytes_from_bytebuf_arg(&args[3], "udp_send_bytebuf()")?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, bytes);
            return Err("udp_send_bytebuf() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_bytebuf() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_bytebuf() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(bytes, &addr)
                .map_err(|e| format!("udp_send_bytebuf() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_close(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("udp_close() requires exactly 1 argument: socket handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_close() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("udp_close() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(_)) => {}
                Some(_) => return Err("udp_close() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_close() invalid handle {}", handle_id)),
            }
            self.net_handles.remove(&handle_id);
            Ok(Value::NIL)
        }
    }

    // ── Tier 5: Runtime Queries ──

    fn bi_query_where(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(
                "query_where() requires at least 2 arguments: component type(s) and predicate"
                    .into(),
            );
        }
        let pred = *args.last().unwrap();
        let comp_names: Vec<String> = args[..args.len() - 1]
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_where() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        let mut result = Vec::new();
        for eid in entities {
            let eid_val = Value::from_entity_id(&mut self.gc, eid);
            let keep = self.call_value(&pred, vec![eid_val])?;
            if keep.is_truthy() {
                result.push(eid_val);
            }
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_query_map(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(
                "query_map() requires at least 2 arguments: component type(s) and map function"
                    .into(),
            );
        }
        let map_fn = *args.last().unwrap();
        let comp_names: Vec<String> = args[..args.len() - 1]
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_map() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        let mut result = Vec::with_capacity(entities.len());
        for eid in entities {
            let eid_val = Value::from_entity_id(&mut self.gc, eid);
            let mapped = self.call_value(&map_fn, vec![eid_val])?;
            result.push(mapped);
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_query_count(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("query_count() requires at least 1 component type argument".into());
        }
        let comp_names: Vec<String> = args
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_count() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        Ok(Value::from_int(&mut self.gc, entities.len() as i64))
    }

    fn bi_with_field(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err("with_field() requires 4 arguments: entity_list, component_type, field_name, predicate".into());
        }
        let pred = args.pop().unwrap();
        let field_name = args
            .pop()
            .unwrap()
            .as_str()
            .ok_or_else(|| "with_field() expects field name string".to_string())?
            .to_string();
        let comp_type = args
            .pop()
            .unwrap()
            .as_str()
            .ok_or_else(|| "with_field() expects component type string".to_string())?
            .to_string();
        self.sandbox_check_read(&comp_type)?;
        let entity_list = args.pop().unwrap();
        let type_name = entity_list.type_name().to_string();
        let entities = entity_list
            .into_rad_list()
            .ok_or_else(|| format!("with_field() expects entity list, got {}", type_name))?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities.into_iter() {
            let eid = entity_val.as_entity_id().ok_or_else(|| {
                format!(
                    "with_field() list must contain entities, got {}",
                    entity_val.type_name()
                )
            })?;
            if let Some(comp) = self.world.get_component(eid, &comp_type) {
                if let Some(idx) = comp.layout.iter().position(|n| n == &field_name) {
                    if let Some(field_val) = comp.values.get(idx) {
                        let keep = self.call_value(&pred, vec![*field_val])?;
                        if keep.is_truthy() {
                            result.push(entity_val);
                        }
                    }
                }
            }
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_variant_of(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("variant_of expects 1 argument, got {}", args.len()));
        }
        let arg = args.pop().unwrap();
        let gc = &mut self.gc;
        if let Some(st) = arg.as_sum_type() {
            Ok(Value::from_string(gc, st.variant.clone()))
        } else if let Some(s) = arg.as_state() {
            Ok(Value::from_string(gc, s.state.clone()))
        } else {
            Ok(Value::NIL)
        }
    }

    fn bi_sys_args(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("sys_args expects 0 arguments, got {}", args.len()));
        }
        // Program args only (what followed `--` on the CLI). Leaking the raw
        // process argv would expose the interpreter path and rad's own flags.
        let args: Vec<String> = self.sys_args.clone();
        let gc = &mut self.gc;
        let mut list = Vec::new();
        for arg in args {
            list.push(Value::from_string(gc, arg));
        }
        Ok(Value::list(gc, list))
    }
}