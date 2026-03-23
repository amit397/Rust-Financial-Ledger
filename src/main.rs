use ledger_guard::cli::Cli;
use ledger_guard::agent::mock::MockAgent;
use ledger_guard::agent::llm::LlmAgent;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Default config
    let mut use_mock = false;
    let mut is_stress = false;
    let mut _stress_agents = 5;
    let mut _stress_duration = 30;
    let mut data_path = "ledger_data.json".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mock" => use_mock = true,
            "--stress" => is_stress = true,
            "--agents" => {
                i += 1;
                if i < args.len() {
                    _stress_agents = args[i].parse().unwrap_or(5);
                }
            }
            "--duration" => {
                i += 1;
                if i < args.len() {
                    _stress_duration = args[i].parse().unwrap_or(30);
                }
            }
            "--data" => {
                i += 1;
                if i < args.len() {
                    data_path = args[i].clone();
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if is_stress {
        println!("Stress test stubbed... (Phase 4)");
        std::process::exit(0);
    }

    let default_accounts = vec!["Checking".to_string(), "Savings".to_string(), "External".to_string()];
    
    let agent: Box<dyn ledger_guard::agent::Agent> = if use_mock {
        Box::new(MockAgent { known_accounts: default_accounts })
    } else {
        match LlmAgent::new("models", default_accounts) {
            Ok(llm) => Box::new(llm),
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    };

    let mut cli = Cli::new(agent, &data_path);
    cli.run();
}

fn print_help() {
    println!("LedgerGuard — AI-Safe Financial Ledger\n");
    println!("USAGE:");
    println!("  ledger-guard [OPTIONS]\n");
    println!("OPTIONS:");
    println!("  --mock         Use MockAgent instead of LlmAgent");
    println!("  --stress       Run stress test");
    println!("  --agents <N>   Stress test agent count (default 5)");
    println!("  --duration <S> Stress test duration in seconds (default 30)");
    println!("  --data <path>  Ledger data file (default: \"ledger_data.json\")");
    println!("  -h, --help     Print usage and exit 0");
}