use vergen::Emitter;
use vergen_gitcl::GitclBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match GitclBuilder::default().sha(true).build() {
        Ok(git) => Emitter::default().add_instructions(&git)?.emit()?,
        Err(_) => println!("cargo:rustc-env=VERGEN_GIT_SHA=unknown"),
    }
    Ok(())
}
