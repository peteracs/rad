use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const INDEX_HTML: &[u8] = include_bytes!("../../../projects/playground/index.html");

fn load_playground_pkg_asset(filename: &str) -> Result<Vec<u8>, String> {
    let path = std::env::current_dir()
        .map_err(|e| format!("failed to read current directory: {e}"))?
        .join("projects")
        .join("playground")
        .join("pkg")
        .join(filename);
    std::fs::read(&path).map_err(|e| {
        format!(
            "playground asset {} is missing ({e}). Build the wasm package into projects/playground/pkg before running rad play.",
            path.display()
        )
    })
}

pub fn execute_play(port: u16) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let addr = format!("127.0.0.1:{}", port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to {}: {}", addr, e);
                std::process::exit(1);
            }
        };

        let actual_port = listener.local_addr().unwrap().port();

        println!("Rad Playground running at http://localhost:{}", actual_port);
        println!("Press Ctrl+C to stop\n");

        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &format!("http://localhost:{}", actual_port)])
            .spawn();

        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open")
            .arg(format!("http://localhost:{}", actual_port))
            .spawn();

        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open")
            .arg(format!("http://localhost:{}", actual_port))
            .spawn();

        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => continue,
            };

            tokio::spawn(async move {
                let mut buf = [0; 1024];
                if let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }

                    let request = String::from_utf8_lossy(&buf[..n]);
                    let first_line = request.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();

                    if parts.len() >= 2 && parts[0] == "GET" {
                        let path = parts[1];

                        let asset: Result<(&str, Vec<u8>), String> = match path {
                            "/" | "/index.html" => Ok(("text/html", INDEX_HTML.to_vec())),
                            "/pkg/rad_vm.js" => load_playground_pkg_asset("rad_vm.js")
                                .map(|body| ("application/javascript", body)),
                            "/pkg/rad_vm_bg.wasm" => load_playground_pkg_asset("rad_vm_bg.wasm")
                                .map(|body| ("application/wasm", body)),
                            _ => {
                                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
                                let _ = socket.write_all(response.as_bytes()).await;
                                return;
                            }
                        };
                        let (status, content_type, body) = match asset {
                            Ok((content_type, body)) => ("200 OK", content_type, body),
                            Err(msg) => ("503 Service Unavailable", "text/plain", msg.into_bytes()),
                        };

                        let response = format!(
                            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            status,
                            content_type,
                            body.len()
                        );

                        if socket.write_all(response.as_bytes()).await.is_ok() {
                            let _ = socket.write_all(&body).await;
                        }
                    }
                }
            });
        }
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod playground_embed_tests {
    use std::collections::HashMap;

    use regex::Regex;

    use crate::wasm::RadRuntime;

    const INDEX_HTML: &[u8] = include_bytes!("../../../projects/playground/index.html");

    fn parse_examples(html: &str) -> HashMap<String, String> {
        let i = html
            .find("const EXAMPLES = {")
            .expect("const EXAMPLES block");
        let tail = &html[i..];
        let j = tail
            .find("const DEFAULT_CODE")
            .expect("const DEFAULT_CODE after EXAMPLES");
        let block = &tail[..j];
        let entry_re =
            Regex::new(r#"(?s)([A-Za-z_][A-Za-z0-9_]*)\s*:\s*`(.*?)`,"#).expect("entry regex");
        let mut out = HashMap::new();
        for cap in entry_re.captures_iter(block) {
            out.insert(cap[1].to_string(), cap[2].to_string());
        }
        out
    }

    fn default_example_key(html: &str) -> String {
        let re = Regex::new(r"const DEFAULT_CODE\s*=\s*EXAMPLES\.([A-Za-z_][A-Za-z0-9_]*)\s*;")
            .expect("DEFAULT_CODE regex");
        re.captures(html)
            .expect("DEFAULT_CODE")
            .get(1)
            .expect("key")
            .as_str()
            .to_string()
    }

    #[test]
    fn index_default_code_points_to_existing_example() {
        let html = std::str::from_utf8(INDEX_HTML).expect("utf8");
        let examples = parse_examples(html);
        let key = default_example_key(html);
        assert!(
            examples.contains_key(&key),
            "DEFAULT_CODE key {key} missing from EXAMPLES"
        );
    }

    #[test]
    fn index_hello_snippet_runs() {
        let html = std::str::from_utf8(INDEX_HTML).expect("utf8");
        let examples = parse_examples(html);
        let src = examples.get("hello").expect("hello example");
        let mut rt = RadRuntime::new();
        let out = rt.compile_and_run(src).expect("hello compiles");
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() >= 2);
        assert_eq!(lines[0], "Hello from Rad!");
        assert_eq!(lines[1], "The Three-Law Language:");
    }

    #[test]
    fn index_ecs_snippet_uses_unwrap() {
        let html = std::str::from_utf8(INDEX_HTML).expect("utf8");
        let examples = parse_examples(html);
        let src = examples.get("ecs").expect("ecs example");
        let mut rt = RadRuntime::new();
        let out = rt.compile_and_run(src).expect("ecs compiles");
        assert!(out.contains("Before:"));
        assert!(out.contains("After 3 ticks:"));
        assert!(out.lines().any(|l| l.contains("Player")));
    }
}
