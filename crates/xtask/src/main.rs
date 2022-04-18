use std::{
    env, fs,
    path::{Path, PathBuf},
};

use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let task = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("No sub command"))?;
    match task.as_str() {
        "dist" => dist()?,
        _ => return Err(anyhow::anyhow!("Unexpected sub command")),
    }

    Ok(())
}

fn dist() -> Result<(), anyhow::Error> {
    let dist_dir = dist_dir();

    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    dist_binary()?;

    println!("Dist available at {}", dist_dir.display());

    Ok(())
}

fn dist_binary() -> Result<(), anyhow::Error> {
    let project_dir = project_root();
    let dist_dir = dist_dir();

    println!("project_dir = {}", project_dir.display());

    let sh = Shell::new()?;
    cmd!(sh, "cargo build --release").run()?;

    let dst = project_root().join("target/release/client");

    fs::copy(&dst, dist_dir.join("client"))?;

    cmd!(sh, "cp -R wwwroot {dist_dir}").run()?;
    cmd!(sh, "mkdir -p {dist_dir}/website").run()?;
    cmd!(sh, "cp -R website/templates {dist_dir}/website").run()?;

    Ok(())
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn dist_dir() -> PathBuf {
    project_root().join("target/dist")
}
