use ledger_guard::cli::Cli;
use ledger_guard::agent::mock::MockAgent;
use ledger_guard::agent::llm::LlmAgent;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Default config
    let mut use_mock = false;
    let mut is_stress = false;
    let mut stress_agents = 5;
    let mut stress_duration = 30;
    let mut data_path = "ledger_data.json".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mock" => use_mock = true,
            "--stress" => is_stress = true,
            "--agents" => {
                i += 1;
                if i < args.len() && !args[i].starts_with("--") {
                    stress_agents = args[i].parse().unwrap_or(5);
                } else {
                    eprintln!("Error: --agents requires a numeric value");
                    std::process::exit(1);
                }
            }
            "--duration" => {
                i += 1;
                if i < args.len() && !args[i].starts_with("--") {
                    stress_duration = args[i].parse().unwrap_or(30);
                } else {
                    eprintln!("Error: --duration requires a numeric value");
                    std::process::exit(1);
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
        let cpu_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        if stress_agents > cpu_count {
            eprintln!("⚠  {} agents on {} cores — context switching will inflate latency numbers",
                      stress_agents, cpu_count);
        }

        let accounts = vec!["Checking","Savings","External","Revenue","Expenses","Investments","Payroll","Escrow"]
            .into_iter().map(String::from).collect::<Vec<_>>();

        let mut configs: Vec<ledger_guard::stress::AgentConfig> = vec![
            ledger_guard::stress::AgentConfig { agent: Box::new(ledger_guard::agent::chaos::ValidAgent::new(accounts.clone())),    name: "ValidAgent".into() },
            ledger_guard::stress::AgentConfig { agent: Box::new(ledger_guard::agent::chaos::OverdraftAgent::new(accounts.clone())), name: "OverdraftAgent".into() },
            ledger_guard::stress::AgentConfig { agent: Box::new(ledger_guard::agent::chaos::TypoAgent::new(accounts.clone())),     name: "TypoAgent".into() },
            ledger_guard::stress::AgentConfig { agent: Box::new(ledger_guard::agent::chaos::OverflowAgent::new(accounts.clone())), name: "OverflowAgent".into() },
        ];
        for i in 4..stress_agents {
            configs.push(ledger_guard::stress::AgentConfig {
                agent: Box::new(ledger_guard::agent::chaos::ChaosAgent::new(accounts.clone())),
                name: format!("ChaosAgent-{}", i),
            });
        }

        let result = ledger_guard::stress::StressTest::new(configs, std::time::Duration::from_secs(stress_duration)).run();
        std::process::exit(if result.all_invariants_passed { 0 } else { 1 });
    }

    let default_accounts = vec!["Checking".to_string(), "Savings".to_string(), "External".to_string()];
    
    let agent: Box<dyn ledger_guard::agent::Agent> = if use_mock {
        Box::new(MockAgent { known_accounts: default_accounts.clone() })
    } else {
        match LlmAgent::new("models", default_accounts.clone()) {
            Ok(llm) => Box::new(llm),
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    };

    let mut cli = Cli::new(agent, &data_path, &default_accounts);
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