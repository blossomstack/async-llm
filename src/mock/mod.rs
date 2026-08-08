#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::wildcard_enum_match_arm
)]

mod anthropic;
mod openai;
mod responses;
mod server;

pub use server::{
    BlockHandle, MockLlmServer, MockLlmServerBuilder, MockResponse, Scenario, ScenarioConfig,
};

/// Runs the mock server as a process, driven by `--port`/`--bind-all`, and
/// parks until the parent kills it.
///
/// Lives here rather than in the binary so an out-of-process test harness in
/// another workspace can expose the same command without restating the argument
/// handling.
pub async fn run_cli() {
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

    // Some CI harnesses set $PORT instead of passing a flag.
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
    // The bound URL is printed because `--port 0` picks an ephemeral one and a
    // parent harness has no other way to learn it.
    let server = builder.build().await;
    println!("mock-llm listening on {}", server.url());

    std::future::pending::<()>().await;
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}
