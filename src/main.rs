use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

mod command_line;
mod config;
mod deps;
mod fix;
mod gen;
mod parse;

use config::Config;
use deps::TypeDeps;
use parse::File;

fn main() -> Result<(), String> {
    // let cmd = command_line::from_env();

    let rpc_config = load_config(PathBuf::from("./starknet_api_openrpc.toml"))?;
    let rpc_document =
        load_and_fix_document(PathBuf::from("./starknet_api_openrpc.json"), &rpc_config)?;

    let trace_config = load_config(PathBuf::from("./starknet_trace_api_openrpc.toml"))?;
    let trace_document = load_and_fix_document(
        PathBuf::from("./starknet_trace_api_openrpc.json"),
        &trace_config,
    )?;

    let write_config = load_config(PathBuf::from("./starknet_write_api.toml"))?;
    let write_document =
        load_and_fix_document(PathBuf::from("./starknet_write_api.json"), &write_config)?;

    let mut deps = TypeDeps::new();

    deps.add(&rpc_config, &rpc_document);
    deps.add(&trace_config, &trace_document);
    deps.add(&write_config, &write_document);

    deps.add_edge(String::from("BlockId"), String::from("F"));

    deps.add_edge(String::from("BroadcastedInvokeTxn"), String::from("F"));
    deps.add_edge(
        String::from("BroadcastedInvokeTxn"),
        String::from("F_ImplicitDefault"),
    );

    deps.add_edge(String::from("BroadcastedDeclareTxn"), String::from("F"));
    deps.add_edge(
        String::from("BroadcastedDeclareTxn"),
        String::from("F_ImplicitDefault"),
    );

    deps.add_edge(
        String::from("BroadcastedDeployAccountTxn"),
        String::from("F"),
    );

    deps.fix_defaults();

    generate(
        &rpc_document,
        &rpc_config,
        &deps,
        PathBuf::from("./starknet_api_openrpc.rs"),
    )?;
    generate(
        &trace_document,
        &trace_config,
        &deps,
        PathBuf::from("./starknet_trace_api_openrpc.rs"),
    )?;
    generate(
        &write_document,
        &write_config,
        &deps,
        PathBuf::from("./starknet_write_api.rs"),
    )?;

    Ok(())
}

/// Print an error message to the standard error stream.
fn print_error(args: std::fmt::Arguments) -> std::io::Result<()> {
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    stderr.write_all(b"\x1B[31merror\x1B[0m: ")?;
    stderr.write_fmt(args)?;
    stderr.write_all(b"\n")?;
    stderr.flush()?;

    Ok(())
}

fn load_config(cmd_config: PathBuf) -> Result<Config, String> {
    config::load(&cmd_config).map_err(|err| format!("`{}`: {}", cmd_config.display(), err))
}

fn load_document(path: &Path) -> Result<open_rpc::OpenRpc, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let buf = std::io::BufReader::new(file);
    let document = serde_json::from_reader(buf).map_err(|e| e.to_string())?;
    Ok(document)
}

fn load_and_fix_document(cmd_document: PathBuf, config: &Config) -> Result<File, String> {
    let document = load_document(&cmd_document)
        .map_err(|err| format!("`{}`: {}", cmd_document.display(), err))?;

    let mut document = parse::parse(&document).map_err(|errs| {
        errs.iter()
            .map(|err| format!("`{}`: {}", err.path, err.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    fix::fix(&mut document, &config).map_err(|errs| errs.join("\n"))?;

    Ok(document)
}

fn generate(
    document: &File,
    config: &Config,
    deps: &TypeDeps,
    cmd_output: PathBuf,
) -> Result<(), String> {
    let mut output = std::fs::File::create(&cmd_output)
        .map_err(|err| format!("`{}`: {}", cmd_output.display(), err))?;

    gen::gen(&mut output, document, config, deps).map_err(|err| format!("{}", err))?;

    drop(output);

    if config.run_rustfmt {
        if let Err(err) = run_rustmft(&cmd_output) {
            return Err(format!("{}", err));
        }
    }
    Ok(())
}

/// Runs `rustfmt` on the provided path.
fn run_rustmft(path: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("rustfmt")
        .arg(path)
        .status()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "rustfmt failed",
        ));
    }
    Ok(())
}
