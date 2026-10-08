mod capsule;
mod hash;
mod native;
mod wlf;
use std::{env, fs, path::Path};

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}
fn write_new(path: &str, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{path}: {e}"))?;
    f.write_all(data).map_err(|e| format!("{path}: {e}"))
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["wlf2vcd",input,output]=>{
            let raw=read(input)?;native::header(&raw)?;
            let body=wlf::export(&raw)?;
            write_new(output,capsule::embed(&body,&raw).as_bytes())?;
            eprintln!("Embedded original WLF for byte-exact recovery");
        },
        ["vcd2wlf",input,output]=>{
            let vcd=String::from_utf8(read(input)?).map_err(|_|"VCD is not UTF-8")?;
            let raw=capsule::restore(&vcd)?;write_new(output,&raw)?;eprintln!("Restored {} original WLF bytes",raw.len());
        },
        ["inspect",input]=>{
            let raw=read(input)?;let (magic,version,resolution)=native::header(&raw)?;
            println!("signature=0x{magic:04x} version={version} resolution={resolution} bytes={} sha256={}",raw.len(),hash::sha256(&raw));
            for (parcel,block) in native::single_parcel_blocks(&raw)?{println!("parcel={parcel} decoded_bytes={}",block.len());}
        },
        ["encode-block",input,output]=>write_new(output,&native::compress_literals(&read(input)?))?,
        ["extract",input,directory]=>{
            let blocks=native::single_parcel_blocks(&read(input)?)?;
            fs::create_dir_all(directory).map_err(|e|e.to_string())?;
            for (parcel,block) in blocks {write_new(&Path::new(directory).join(format!("parcel-{parcel}.bin")).to_string_lossy(),&block)?;}
        },
        _=>return Err("Usage:\n  vcdeez wlf2vcd INPUT.wlf OUTPUT.vcd\n  vcdeez vcd2wlf INPUT.vcd OUTPUT.wlf\n  vcdeez inspect INPUT.wlf\n  vcdeez extract INPUT.wlf NEW_DIRECTORY\n\nOutputs must not already exist. Native Rust export supports a subset of WLF v172; restore requires an unmodified vcdeez VCD with its embedded WLF capsule.".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
