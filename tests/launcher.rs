//! Exercise the actual executable with a fake agent, including signals and cleanup.
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn fixture(
    agent_script: &str,
    access_script: &str,
    persistent: bool,
) -> (tempfile::TempDir, Command, std::net::TcpListener) {
    fixture_for_agent("codex", agent_script, access_script, persistent)
}

fn fixture_for_agent(
    agent_name: &str,
    agent_script: &str,
    access_script: &str,
    persistent: bool,
) -> (tempfile::TempDir, Command, std::net::TcpListener) {
    let directory = tempfile::tempdir().unwrap();
    let codex_home = directory.path().join("codex-home");
    fs::create_dir(&codex_home).unwrap();
    fs::write(codex_home.join("config.toml"), "model = \"original\"\n").unwrap();
    fs::write(codex_home.join("models_cache.json"), "original catalog").unwrap();
    let agent = directory.path().join(agent_name);
    fs::write(&agent, format!("#!/bin/sh\n{agent_script}\n")).unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o700)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let config = directory.path().join("credential.json");
    fs::write(&config, serde_json::to_vec(&json!({
        "backends":{"test":{"url":format!("http://{}/v1", listener.local_addr().unwrap()),"protocol":"responses","model":"fixture-model","access":{
            "command":access_script,"persistent":persistent,"timeout_secs":5,
            "cleanup":format!("printf cleaned > {}",quote(&directory.path().join("cleanup")))
        }}},"agents":{agent_name:{"backend":"test"}}
    })).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_wmlm"));
    command
        .arg("--config")
        .arg(&config)
        .arg(agent_name)
        .env("CODEX_HOME", &codex_home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                directory.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    (directory, command, listener)
}

#[test]
fn codex_model_writes_do_not_change_global_defaults() {
    let (directory, mut command, _listener) = fixture(
        "printf '%s' \"$CODEX_HOME\" > \"$LAUNCH_HOME_MARKER\"\n\
         printf 'model = \"qwen\"\\n' > \"$CODEX_HOME/config.toml\"\n\
         printf 'local catalog' > \"$CODEX_HOME/models_cache.json\"\n\
         exit 23",
        "true",
        false,
    );
    let marker = directory.path().join("launch-home");
    command.env("LAUNCH_HOME_MARKER", &marker);
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original = directory.path().join("codex-home");
    assert_eq!(
        fs::read_to_string(original.join("config.toml")).unwrap(),
        "model = \"original\"\n"
    );
    assert_eq!(
        fs::read_to_string(original.join("models_cache.json")).unwrap(),
        "original catalog"
    );
    let temporary = fs::read_to_string(marker).unwrap();
    assert_ne!(std::path::Path::new(&temporary), original);
    assert!(!std::path::Path::new(&temporary).exists());
}

#[test]
fn preserves_agent_status_and_cleans_access() {
    let (directory, mut command, _listener) = fixture("exit 23", "sleep 60", true);
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("wmlm: codex exited with exit status: 23"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("cleanup")).unwrap(),
        "cleaned"
    );
    assert_eq!(
        fs::metadata(directory.path().join("credential.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[tokio::test]
async fn random_port_is_shared_by_url_credentials_and_commands() {
    use axum::{http::HeaderMap, routing::get, Json, Router};
    let (directory, mut command, listener) = fixture("exit 23", "true", false);
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let marker = directory.path().join("chosen-port");
    let cleanup = directory.path().join("cleanup");
    let config_path = directory.path().join("credential.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["random_port_candidates"] = json!(port.to_string());
    let backend = &mut config["backends"]["test"];
    backend["url"] = json!("http://127.0.0.1:{pOrT}/v1");
    backend["model"] = serde_json::Value::Null;
    backend["auth"] = json!({"type":"bearer", "token":"secret-{PORT}"});
    backend["access"]["command"] = json!(format!("printf '%s' '{{port}}' > {}", quote(&marker)));
    backend["access"]["cleanup"] = json!(format!("printf '%s' '{{PoRt}}' > {}", quote(&cleanup)));
    let saved = serde_json::to_vec(&config).unwrap();
    fs::write(&config_path, &saved).unwrap();
    let child = tokio::task::spawn_blocking(move || command.output().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read_to_string(&marker).unwrap_or_default() != port.to_string() {
        assert!(
            Instant::now() < deadline,
            "access command did not receive the selected port"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let app = Router::new().route(
        "/v1/models",
        get(move |headers: HeaderMap| async move {
            assert_eq!(headers["authorization"], format!("Bearer secret-{port}"));
            Json(json!({"data":[{"id":"random-port-model"}]}))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let output = child.await.unwrap();
    server.abort();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(cleanup).unwrap(), port.to_string());
    assert_eq!(
        fs::read(config_path).unwrap(),
        saved,
        "saved templates must not change"
    );
}

#[test]
fn exhausted_ports_stop_launch_only_when_a_placeholder_needs_a_port() {
    for location in [
        "none", "url", "token", "username", "password", "command", "cleanup", "direct", "search",
    ] {
        let (directory, mut command, listener) = fixture(
            "touch \"$AGENT_MARKER\"; exit 23",
            "touch \"$TUNNEL_MARKER\"",
            false,
        );
        let agent_marker = directory.path().join("agent-started");
        let tunnel_marker = directory.path().join("tunnel-started");
        command
            .env("AGENT_MARKER", &agent_marker)
            .env("TUNNEL_MARKER", &tunnel_marker);
        let port = listener.local_addr().unwrap().port();
        let path = directory.path().join("credential.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["random_port_candidates"] = json!(port.to_string());
        let backend = &mut config["backends"]["test"];
        match location {
            "url" | "direct" => {
                backend["url"] = json!("http://127.0.0.1:{PoRt}/v1");
                if location == "direct" {
                    backend["access"] = serde_json::Value::Null;
                }
            }
            "token" => backend["auth"] = json!({"type":"bearer", "token":"key-{port}"}),
            "username" => {
                backend["auth"] =
                    json!({"type":"basic", "username":"user-{PORT}", "password":"key"})
            }
            "password" => {
                backend["auth"] =
                    json!({"type":"basic", "username":"user", "password":"key-{PORT}"})
            }
            "command" => {
                backend["access"]["command"] = json!("touch \"$TUNNEL_MARKER\"; echo {pOrT}")
            }
            "cleanup" => backend["access"]["cleanup"] = json!("echo {port}"),
            "search" => config["web_search"] = json!({"provider":"brave", "api_key":"key-{PORT}"}),
            _ => {}
        }
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let output = command.output().unwrap();
        if location == "none" {
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(agent_marker.exists());
            assert!(tunnel_marker.exists());
        } else {
            assert_eq!(output.status.code(), Some(1), "{location}");
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(&format!(
                    "Unable to assign a port in the port range: {port}"
                )),
                "{location}: {error}"
            );
            assert!(!agent_marker.exists(), "{location}: agent must not start");
            assert!(!tunnel_marker.exists(), "{location}: tunnel must not start");
            assert!(!directory.path().join("cleanup").exists());
        }
    }
}

#[test]
fn persistent_access_restarts_until_agent_exits() {
    for exit_code in [0, 7] {
        let (directory, mut command, _listener) = fixture(
            "touch \"$AGENT_READY\"\n\
             attempts=0\n\
             while [ ! -f \"$ACCESS_STARTS\" ] || [ \"$(wc -c < \"$ACCESS_STARTS\")\" -lt 3 ]; do\n\
               attempts=$((attempts + 1))\n\
               if [ \"$attempts\" -ge 150 ]; then exit 99; fi\n\
               sleep 0.05\n\
             done\n\
             exit 23",
            &format!(
                "printf x >> \"$ACCESS_STARTS\"\n\
                 while [ ! -f \"$AGENT_READY\" ]; do sleep 0.05; done\n\
                 exit {exit_code}"
            ),
            true,
        );
        let starts = directory.path().join("access-starts");
        command
            .env("ACCESS_STARTS", &starts)
            .env("AGENT_READY", directory.path().join("agent-ready"));
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(&starts).unwrap(), "xxx");
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(fs::read_to_string(&starts).unwrap(), "xxx");
        assert_eq!(
            fs::read_to_string(directory.path().join("cleanup")).unwrap(),
            "cleaned"
        );
    }
}

#[test]
fn opencode_survives_access_death_and_failed_reconnections() {
    let (directory, mut command, _listener) = fixture_for_agent(
        "opencode",
        "if [ \"$1\" = '--help' ]; then printf '%s\\n' '--standalone'; exit 0; fi\n\
         printf '%s\\n' \"$$\" >> \"$AGENT_READY\"\n\
         attempts=0\n\
         while [ ! -f \"$ACCESS_RESTORED\" ]; do\n\
           attempts=$((attempts + 1))\n\
           if [ \"$attempts\" -ge 240 ]; then exit 99; fi\n\
           sleep 0.05\n\
         done\n\
         sleep 0.2\n\
         exit 23",
        "printf x >> \"$ACCESS_STARTS\"\n\
         case $(( $(wc -c < \"$ACCESS_STARTS\") )) in\n\
           1) while [ ! -f \"$AGENT_READY\" ]; do sleep 0.05; done; kill -KILL $$ ;;\n\
           2|3) sleep 0.1; exit 255 ;;\n\
           *) touch \"$ACCESS_RESTORED\"; exec sleep 60 ;;\n\
         esac",
        true,
    );
    let starts = directory.path().join("access-starts");
    command
        .env("ACCESS_STARTS", &starts)
        .env("ACCESS_RESTORED", directory.path().join("access-restored"))
        .env("AGENT_READY", directory.path().join("agent-ready"));
    let output = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(23), "{stderr}");
    assert_eq!(fs::read_to_string(&starts).unwrap(), "xxxx", "{stderr}");
    assert_eq!(
        fs::read_to_string(directory.path().join("agent-ready"))
            .unwrap()
            .lines()
            .count(),
        1,
        "OpenCode was relaunched during access recovery"
    );
    assert!(stderr.contains("backend access restored"), "{stderr}");
    assert!(
        stderr.contains("wmlm: opencode exited with exit status: 23"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("cleanup")).unwrap(),
        "cleaned"
    );
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(fs::read_to_string(&starts).unwrap(), "xxxx");
}

#[test]
fn opencode_stays_running_when_access_cannot_reconnect() {
    let (directory, mut command, listener) = fixture_for_agent(
        "opencode",
        "if [ \"$1\" = '--help' ]; then exit 0; fi\n\
         touch \"$AGENT_READY\"\n\
         attempts=0\n\
         while [ ! -f \"$STOP_AGENT\" ]; do\n\
           printf x >> \"$AGENT_HEARTBEAT\"\n\
           attempts=$((attempts + 1))\n\
           if [ \"$attempts\" -ge 240 ]; then exit 99; fi\n\
           sleep 0.05\n\
         done\n\
         exit 23",
        "printf x >> \"$ACCESS_STARTS\"\n\
         if [ \"$(wc -c < \"$ACCESS_STARTS\")\" -gt 1 ]; then exit 255; fi\n\
         printf '%s' \"$$\" > \"$ACCESS_PID\"\n\
         exec sleep 60",
        true,
    );
    let ready = directory.path().join("agent-ready");
    let heartbeat = directory.path().join("heartbeat");
    let starts = directory.path().join("access-starts");
    let pid_file = directory.path().join("access-pid");
    let stop = directory.path().join("stop-agent");
    command
        .env("AGENT_READY", &ready)
        .env("AGENT_HEARTBEAT", &heartbeat)
        .env("ACCESS_STARTS", &starts)
        .env("ACCESS_PID", &pid_file)
        .env("STOP_AGENT", &stop);
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() || !heartbeat.exists() {
        assert!(Instant::now() < deadline, "OpenCode did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let access_pid: i32 = fs::read_to_string(pid_file).unwrap().parse().unwrap();
    drop(listener);
    assert_eq!(unsafe { libc::kill(access_pid, libc::SIGKILL) }, 0);
    while fs::metadata(&starts).unwrap().len() < 4 {
        assert!(Instant::now() < deadline, "access was not retried");
        assert!(
            child.try_wait().unwrap().is_none(),
            "launcher exited during recovery"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let length = fs::metadata(&heartbeat).unwrap().len();
    std::thread::sleep(Duration::from_millis(150));
    assert!(child.try_wait().unwrap().is_none());
    assert!(
        fs::metadata(&heartbeat).unwrap().len() > length,
        "OpenCode stopped while access remained unavailable"
    );
    fs::write(stop, "stop").unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(23), "{stderr}");
    assert!(
        stderr.contains("could not restart backend access"),
        "{stderr}"
    );
    assert!(!stderr.contains("backend access restored"), "{stderr}");
    assert_eq!(
        fs::read_to_string(directory.path().join("cleanup")).unwrap(),
        "cleaned"
    );
}

#[test]
fn sigterm_during_preparation_runs_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("started");
    let script = format!("printf started > {}; sleep 60", quote(&marker));
    let (directory, mut command, _listener) = fixture("exit 0", &script, false);
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "access command never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(143));
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("launcher did not stop after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("cleanup")).unwrap(),
        "cleaned"
    );
}

#[test]
fn cfg_subcommand_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
        .args(["cfg", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--config"));
}

#[test]
fn sigterm_stops_agent_tool_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let heartbeat = temp.path().join("heartbeat");
    let script = format!(
        "(trap '' TERM; while :; do printf tick >> {}; sleep 0.03; done) &\nwait",
        quote(&heartbeat)
    );
    let (directory, mut command, _listener) = fixture(&script, "true", false);
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !heartbeat.exists() {
        assert!(Instant::now() < deadline, "agent tool never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(143));
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("launcher did not stop after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = fs::metadata(&heartbeat).unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        fs::metadata(&heartbeat).unwrap().len(),
        stopped,
        "TERM-ignoring tool descendant survived launcher exit"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("cleanup")).unwrap(),
        "cleaned"
    );
}

#[test]
fn interactive_agent_can_read_foreground_terminal() {
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::os::unix::process::CommandExt;
    let (_directory, mut command, _listener) = fixture(
        "printf 'agent-ready\\n'; read answer; printf 'agent-read:%s\\n' \"$answer\"",
        "true",
        false,
    );
    let mut master = -1;
    let mut slave = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    unsafe {
        libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(slave, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let mut master = unsafe { fs::File::from_raw_fd(master) };
    let slave = unsafe { fs::File::from_raw_fd(slave) };
    command
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave.try_clone().unwrap());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    drop(slave);
    use std::os::fd::AsRawFd;
    unsafe {
        libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
    }
    let mut output = Vec::new();
    let mut sent = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut buffer = [0_u8; 1024];
        while let Ok(count) = master.read(&mut buffer) {
            if count == 0 {
                break;
            }
            output.extend_from_slice(&buffer[..count]);
        }
        if !sent && String::from_utf8_lossy(&output).contains("agent-ready") {
            master.write_all(b"hello-terminal\n").unwrap();
            sent = true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{status}: {}",
                String::from_utf8_lossy(&output)
            );
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            drop(master);
            let _ = child.kill();
            panic!(
                "interactive agent terminal timeout: {}",
                String::from_utf8_lossy(&output)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        String::from_utf8_lossy(&output).contains("agent-read:hello-terminal"),
        "{}",
        String::from_utf8_lossy(&output)
    );
}

#[tokio::test]
async fn all_agents_launch_with_discovered_default_model() {
    use axum::{routing::get, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/v1/models",
        get(|| async { Json(json!({"data":[{"id":"local/first"},{"id":"local/second"}]})) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for agent_name in ["opencode", "pi", "codex", "claude"] {
        let directory = tempfile::tempdir().unwrap();
        let agent = directory.path().join(agent_name);
        let script = if agent_name == "opencode" {
            "#!/bin/sh\nif [ \"$1\" = '--help' ]; then exit 0; fi\nprintf '%s' \"$OPENCODE_CONFIG_CONTENT\"\n"
        } else {
            "#!/bin/sh\nprintf '%s\\n' \"$@\"\n"
        };
        fs::write(&agent, script).unwrap();
        fs::set_permissions(&agent, fs::Permissions::from_mode(0o700)).unwrap();
        let config = directory.path().join("credential.json");
        fs::write(
            &config,
            serde_json::to_vec(&json!({
                "backends":{"test":{"url":url,"protocol":"chat_completions"}},
                "agents":{(agent_name):{"backend":"test"}}
            }))
            .unwrap(),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_wmlm"));
        command
            .arg("--config")
            .arg(config)
            .arg(agent_name)
            .env("CODEX_HOME", directory.path().join("codex-home"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    directory.path().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        let output = tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{agent_name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if agent_name == "opencode" {
            let config: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(config["model"], "wmlm/local/first");
            assert_eq!(
                config["provider"]["wmlm"]["models"]["local/second"]["name"],
                "local/second"
            );
        } else {
            let output = String::from_utf8(output.stdout).unwrap();
            let args: Vec<_> = output.lines().collect();
            assert!(
                args.windows(2)
                    .any(|args| args == ["--model", "local/first"]),
                "{agent_name}: {output}"
            );
        }
    }
    server.abort();
}

#[test]
fn update_help_and_invalid_options_do_not_launch_agents() {
    let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
        .args(["update", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--check"));
    let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
        .args(["update", "--backend", "local"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn removed_cfg_flags_are_rejected() {
    for flag in ["-cfg", "--cfg"] {
        let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
            .arg(flag)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn cfg_accepts_config_on_either_side_and_rejects_agent_options() {
    for args in [
        vec!["--config", "/unused/settings.json", "cfg", "--help"],
        vec!["cfg", "--config", "/unused/settings.json", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wmlm"))
        .args(["--backend", "local", "cfg"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
