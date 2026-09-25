use parser_codec::{decode_varint, encode_varint};
use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let mut out = Vec::new();
    match decode_varint(&input) {
        Ok((n, _)) => {
            encode_varint(&mut out, n);
            io::stdout().write_all(&out)?;
        }
        Err(_) => {
            encode_varint(&mut out, input.len() as u64);
            io::stdout().write_all(&out)?;
        }
    }
    Ok(())
}
