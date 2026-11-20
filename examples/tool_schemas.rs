//! Writes the hosted tool's input and output schemas into its folder, `tools/sent-mail/`, as the types state them.

fn main() -> std::io::Result<()> {
  let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").join("sent-mail");
  let (input, output) = bmo_mail::sent_mail::schemas();

  std::fs::create_dir_all(&folder)?;
  std::fs::write(folder.join("input.json"), input)?;
  std::fs::write(folder.join("output.json"), output)
}
