fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().compile(&["proto/node.proto"], &["proto"])?;

    Ok(())
}
