use clap::{App, Arg, SubCommand};
use std::path::PathBuf;
use nova::semantic::api::{NovaAnalyzer, AnalysisResult};

fn main() {
    let matches = App::new("nova-analyzer")
        .version("1.0.0")
        .about("Nova Semantic Analysis Tool")
        .author("Nova Language Team")
        .arg(Arg::with_name("verbose")
            .short("v")
            .long("verbose")
            .help("Enable verbose output"))
        .subcommand(SubCommand::with_name("check")
            .about("Analyze Nova source files")
            .arg(Arg::with_name("files")
                .multiple(true)
                .required(true)
                .help("Nova source files to analyze")))
        .subcommand(SubCommand::with_name("check-dir")
            .about("Analyze all Nova files in a directory")
            .arg(Arg::with_name("directory")
                .required(true)
                .help("Directory containing Nova source files")))
        .subcommand(SubCommand::with_name("check-source")
            .about("Analyze Nova source code from stdin")
            .arg(Arg::with_name("name")
                .short("n")
                .long("name")
                .value_name("NAME")
                .help("Name for the source (for display purposes)")
                .default_value("stdin")))
        .get_matches();

    let verbose = matches.is_present("verbose");

    // Create analyzer with appropriate settings
    let mut analyzer = if verbose {
        NovaAnalyzer::new().with_verbose()
    } else {
        NovaAnalyzer::new()
    };

    match matches.subcommand() {
        ("check", Some(sub_matches)) => {
            let files: Vec<&str> = sub_matches.values_of("files").unwrap().collect();
            check_files(&mut analyzer, &files);
        }
        ("check-dir", Some(sub_matches)) => {
            let directory = sub_matches.value_of("directory").unwrap();
            check_directory(&mut analyzer, directory);
        }
        ("check-source", Some(sub_matches)) => {
            let name = sub_matches.value_of("name").unwrap();
            check_stdin(&mut analyzer, name);
        }
        _ => {
            eprintln!("Use --help for usage information");
            std::process::exit(1);
        }
    }
}

fn check_files(analyzer: &mut NovaAnalyzer, files: &[&str]) {
    println!("🚀 Analyzing {} Nova source files...", files.len());
    
    let file_paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let result = analyzer.analyze_files(&file_paths);
    
    analyzer.print_analysis_report(&result);
    
    match result {
        AnalysisResult::Success(_) => std::process::exit(0),
        AnalysisResult::Errors(diagnostics) => {
            let error_count = diagnostics.iter()
                .filter(|d| matches!(d.severity, nova::semantic::DiagnosticSeverity::Error))
                .count();
            std::process::exit(if error_count > 0 { 1 } else { 0 });
        }
    }
}

fn check_directory(analyzer: &mut NovaAnalyzer, directory: &str) {
    println!("🚀 Analyzing Nova files in directory: {}", directory);
    
    let result = analyzer.analyze_directory(directory);
    
    analyzer.print_analysis_report(&result);
    
    match result {
        AnalysisResult::Success(_) => std::process::exit(0),
        AnalysisResult::Errors(diagnostics) => {
            let error_count = diagnostics.iter()
                .filter(|d| matches!(d.severity, nova::semantic::DiagnosticSeverity::Error))
                .count();
            std::process::exit(if error_count > 0 { 1 } else { 0 });
        }
    }
}

fn check_stdin(analyzer: &mut NovaAnalyzer, name: &str) {
    println!("🚀 Analyzing Nova source from {} ...", name);
    
    // Read source from stdin
    use std::io::{self, Read};
    let mut source = String::new();
    match io::stdin().read_to_string(&mut source) {
        Ok(_) => {
            let result = analyzer.analyze_source(&source);
            analyzer.print_analysis_report(&result);
            
            match result {
                AnalysisResult::Success(_) => std::process::exit(0),
                AnalysisResult::Errors(diagnostics) => {
                    let error_count = diagnostics.iter()
                        .filter(|d| matches!(d.severity, nova::semantic::DiagnosticSeverity::Error))
                        .count();
                    std::process::exit(if error_count > 0 { 1 } else { 0 });
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to read from stdin: {}", e);
            std::process::exit(1);
        }
    }
}

// Example usage:
// 
// # Analyze specific files
// nova-analyzer check src/main.nova src/utils.nova
//
// # Analyze all files in a directory
// nova-analyzer check-dir src/
//
// # Analyze from stdin
// echo 'namespace Test; export define hello(): unit; println("Hello!"); end' | nova-analyzer check-source
//
// # Verbose output
// nova-analyzer -v check src/main.nova