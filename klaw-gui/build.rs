use vergen::Emitter;
use vergen_gitcl::GitclBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let git = GitclBuilder::default().sha(true).build()?;
    Emitter::default().add_instructions(&git)?.emit()?;
    Ok(())
}
