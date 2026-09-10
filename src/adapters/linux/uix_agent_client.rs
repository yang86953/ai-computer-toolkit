// UIX Agent v1 已认证 JSON Lines 客户端实现。

struct AgentClient {
    reader: BufReader<UnixStream>,
    deadline: Instant,
    request_types: BTreeSet<String>,
    semantic_actions: BTreeSet<String>,
    window_actions: BTreeSet<String>,
    window_state_fields: BTreeSet<String>,
    key_names: BTreeSet<String>,
    key_modifiers: BTreeSet<String>,
    screenshot_limits: Option<uix_agent_screenshot::ScreenshotLimits>,
}

impl AgentClient {
    fn connect(descriptor: &EndpointDescriptor) -> Result<Self, Failure> {
        Self::connect_until(descriptor, Instant::now() + IO_TIMEOUT)
    }

    fn connect_until(descriptor: &EndpointDescriptor, deadline: Instant) -> Result<Self, Failure> {
        let fd = socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )
        .map_err(|_| Failure::Unavailable)?;
        let address =
            SocketAddrUnix::new(Path::new(&descriptor.endpoint)).map_err(|_| Failure::Protocol)?;
        match connect(&fd, &address) {
            Ok(()) => {}
            Err(error) if error == Errno::INPROGRESS || error == Errno::AGAIN => {
                let mut descriptors = [PollFd::new(&fd, PollFlags::OUT)];
                let connect_timeout = remaining(deadline)?.min(CONNECT_TIMEOUT);
                let timeout = Timespec {
                    tv_sec: connect_timeout.as_secs() as _,
                    tv_nsec: connect_timeout.subsec_nanos() as _,
                };
                if poll(&mut descriptors, Some(&timeout)).map_err(|_| Failure::Unavailable)? == 0 {
                    return Err(Failure::Timeout);
                }
                socket_error(&fd)
                    .map_err(|_| Failure::Unavailable)?
                    .map_err(|_| Failure::Unavailable)?;
            }
            Err(_) => return Err(Failure::Unavailable),
        }
        let stream = UnixStream::from(fd);
        stream
            .set_nonblocking(false)
            .map_err(|_| Failure::Unavailable)?;
        let timeout = remaining(deadline)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| Failure::Unavailable)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| Failure::Unavailable)?;
        let credentials = socket_peercred(&stream).map_err(|_| Failure::PermissionDenied)?;
        if credentials.uid != geteuid()
            || credentials.pid.as_raw_pid() != descriptor.process_id as i32
        {
            return Err(Failure::PermissionDenied);
        }
        let mut client = Self {
            reader: BufReader::new(stream),
            deadline,
            request_types: BTreeSet::new(),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::new(),
            window_state_fields: BTreeSet::new(),
            key_names: BTreeSet::new(),
            key_modifiers: BTreeSet::new(),
            screenshot_limits: None,
        };
        let hello = client
            .request("act-hello", "hello", json!({ "token": descriptor.token }))
            .map_err(read_request_failure)?;
        if hello.get("process_id").and_then(Value::as_u64) != Some(u64::from(descriptor.process_id))
        {
            return Err(Failure::PermissionDenied);
        }
        let request_types = hello
            .pointer("/capabilities/request_types")
            .and_then(Value::as_array)
            .ok_or(Failure::Protocol)?;
        if request_types.len() > 32 {
            return Err(Failure::Protocol);
        }
        client.request_types = bounded_capability_names(request_types)?;
        for required in ["hello", "list_windows", "snapshot"] {
            if !client.request_types.contains(required) {
                return Err(Failure::Protocol);
            }
        }
        client.screenshot_limits =
            uix_agent_screenshot::negotiated_limits(&hello, &client.request_types)?;
        if let Some(actions) = hello
            .pointer("/capabilities/semantic_actions")
            .and_then(Value::as_array)
        {
            if actions.len() > 32 {
                return Err(Failure::Protocol);
            }
            client.semantic_actions = bounded_capability_names(actions)?;
        }
        if let Some(actions) = hello
            .pointer("/capabilities/window_actions")
            .and_then(Value::as_array)
        {
            if actions.len() > 32 {
                return Err(Failure::Protocol);
            }
            client.window_actions = bounded_capability_names(actions)?;
        }
        if let Some(fields) = hello.pointer("/capabilities/window_state_fields") {
            let fields = fields.as_array().ok_or(Failure::Protocol)?;
            if fields.len() > 16 {
                return Err(Failure::Protocol);
            }
            client.window_state_fields = bounded_capability_names(fields)?;
        }
        if let Some(keys) = hello
            .pointer("/capabilities/key_names")
            .and_then(Value::as_array)
        {
            if keys.len() > 128 {
                return Err(Failure::Protocol);
            }
            client.key_names = bounded_capability_names(keys)?;
        }
        if let Some(modifiers) = hello
            .pointer("/capabilities/key_modifiers")
            .and_then(Value::as_array)
        {
            if modifiers.len() > 8 {
                return Err(Failure::Protocol);
            }
            client.key_modifiers = bounded_capability_names(modifiers)?;
        }
        Ok(client)
    }

    fn supports_window_state(&self) -> bool {
        [
            "logical_width",
            "logical_height",
            "maximized",
            "minimized",
            "fullscreen",
        ]
        .iter()
        .all(|field| self.window_state_fields.contains(*field))
    }

    fn supports_screenshot(&self) -> bool {
        self.screenshot_limits.is_some()
    }

    fn supports_activation(&self) -> bool {
        self.window_actions.contains("activate_window")
    }

    fn supports_pointer_drag(&self) -> bool {
        ["pointer_down", "pointer_move", "pointer_up"]
            .iter()
            .all(|action| self.window_actions.contains(*action))
    }

    fn list_windows(&mut self) -> Result<Vec<WireWindow>, Failure> {
        let reply = self
            .request("act-list-windows", "list_windows", json!({}))
            .map_err(read_request_failure)?;
        serde_json::from_value(reply.get("windows").cloned().ok_or(Failure::Protocol)?)
            .map_err(|_| Failure::Protocol)
    }

    fn snapshot(&mut self, window_id: u64, generation: u64) -> Result<WireSnapshot, Failure> {
        let reply = self
            .request(
                "act-snapshot",
                "snapshot",
                json!({ "window_id": window_id, "generation": generation }),
            )
            .map_err(read_request_failure)?;
        serde_json::from_value(reply.get("snapshot").cloned().ok_or(Failure::Protocol)?)
            .map_err(|_| Failure::Protocol)
    }

    fn request(
        &mut self,
        request_id: &'static str,
        request_type: &'static str,
        payload: Value,
    ) -> Result<Value, RequestFailure> {
        self.request_with_response_limit(request_id, request_type, payload, MAX_MESSAGE_BYTES)
    }

    fn request_with_response_limit(
        &mut self,
        request_id: &'static str,
        request_type: &'static str,
        payload: Value,
        response_limit: usize,
    ) -> Result<Value, RequestFailure> {
        let mut request = Map::new();
        request.insert("schema".to_owned(), json!(PROTOCOL_SCHEMA));
        request.insert("request_id".to_owned(), json!(request_id));
        request.insert("type".to_owned(), json!(request_type));
        if let Value::Object(payload) = payload {
            request.extend(payload);
        }
        let mut bytes = serde_json::to_vec(&request).map_err(|_| Failure::Protocol)?;
        if bytes.len() >= MAX_MESSAGE_BYTES {
            return Err(Failure::Protocol.into());
        }
        bytes.push(b'\n');
        self.refresh_timeouts()?;
        self.reader
            .get_mut()
            .write_all(&bytes)
            .map_err(io_failure)?;
        self.reader.get_mut().flush().map_err(io_failure)?;
        self.refresh_timeouts()?;
        let response = read_bounded_line_with_limit(&mut self.reader, response_limit)?;
        let value = serde_json::from_slice::<Value>(&response).map_err(|_| Failure::Protocol)?;
        let object = value.as_object().ok_or(Failure::Protocol)?;
        if object.get("schema").and_then(Value::as_str) != Some(PROTOCOL_SCHEMA)
            || object.get("request_id").and_then(Value::as_str) != Some(request_id)
        {
            return Err(Failure::Protocol.into());
        }
        if object.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(wire_error(object));
        }
        if object.get("type").and_then(Value::as_str) != Some(request_type) {
            return Err(Failure::Protocol.into());
        }
        Ok(value)
    }

    fn refresh_timeouts(&self) -> Result<(), Failure> {
        let timeout = remaining(self.deadline)?;
        self.reader
            .get_ref()
            .set_read_timeout(Some(timeout))
            .map_err(|_| Failure::Unavailable)?;
        self.reader
            .get_ref()
            .set_write_timeout(Some(timeout))
            .map_err(|_| Failure::Unavailable)
    }
}
