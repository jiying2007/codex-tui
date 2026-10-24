use super::*;

pub fn run_cli(args: &[String]) -> Result<i32> {
    let (mut warmup, mut iterations) = (20, 200);
    let mut source = "local-diagnostic".to_string();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--warmup" | "--iterations" | "--source" => {
                index += 1;
                let value = args.get(index).context("benchmark option requires value")?;
                match arg.as_str() {
                    "--warmup" => warmup = value.parse()?,
                    "--iterations" => iterations = value.parse()?,
                    _ => source = value.clone(),
                }
            }
            "--json" => {}
            other => anyhow::bail!("unknown interaction benchmark option: {other}"),
        }
        index += 1;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&run(warmup, iterations, source)?)?
    );
    Ok(0)
}
