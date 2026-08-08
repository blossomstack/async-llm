use async_llm::mock::MockLlmServer;

#[tokio::main]
async fn main() {
    let mut port: u16 = 0;
    let mut bind_all = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_else(|| fail("--port requires a valid u16"));
            }
            "--bind-all" => bind_all = true,
            "-h" | "--help" => {
                println!("Usage: async-llm-mock [--port <N>] [--bind-all]");
                return;
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }

    if port == 0 {
        if let Ok(value) = std::env::var("PORT") {
            if let Ok(parsed) = value.parse() {
                port = parsed;
            }
        }
    }

    let mut builder = MockLlmServer::builder().port(port);
    if bind_all {
        builder = builder.bind_all_interfaces();
    }
    let server = builder.build().await;
    println!("async-llm-mock listening on {}", server.url());

    std::future::pending::<()>().await;
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}
